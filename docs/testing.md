# Testing notes

See also `nextest.md` (running the suites) and `testing-filesystems.md` (tests that depend on a
file system's capabilities).

## Making allocations fail (`liella_boundary::selftest::failinject`)

Hoarding memory from outside the process (an address-space limit, a memory hog) does not make
Liella's internal allocations fail where a test needs them to, so `ENOMEM` paths deep inside a
function (a directory walker's node, a stream's buffer, `qsort`'s scratch array) cannot be
reached that way. Test builds have a per-thread hook instead (ALLOC-FAIL-INJECT, coordinator
#2020), in `liella_boundary::selftest::failinject` (`fi` below):

| Call | Effect on the calling thread |
|------|------------------------------|
| `fi::fail_nth(n)` | the `n`-th allocation from now fails (1: the next), once; then off |
| `fi::fail_at_least(k)` | every allocation of at least `k` bytes fails, until `off` |
| `fi::off()` | allocations succeed again (also disarms an unspent `fail_nth`) |
| `fi::injected()` | how many allocations the hook has failed on this thread so far |

It covers both allocation domains:

* The internal heap (`liella::mem`'s `TryVec` / `TryBox`, and every other `try_reserve`): the test
  binary declares `fi::FailAlloc` as its global allocator (one line, below). `FailAlloc` is the
  host allocator with the thread's plan in front of it; a selected allocation returns null, so the
  collection takes its ordinary failure path and reports `ENOMEM`. A test binary without that line
  is unaffected by the plan for this domain.
* The caller-owned `malloc`/`calloc`/`realloc` wrappers (`strdup`, `FTSENT` blocks, `scandir`
  entries, ...) return NULL with `errno` set to `ENOMEM`, in any test binary.

That is exactly what exhausted memory produces. Every allocation the thread makes while armed
counts, the test's own included, so arm the plan just around the call under test. Only allocations
that reach an allocator count: a `reserve` its spare capacity already covers is not one, and a
shrinking `realloc` is never failed. Other threads are not affected.

The hook exists only with the `selftest` feature, which `liella-boundary`'s own dev-dependency
enables for the test suites and no product ever does. Nothing in the product code paths checks for
it: the core crates are unchanged, and the one call site (`caller_alloc`) compiles to nothing
without the feature, so the shipped libraries are byte-identical with and without the hook
(`crates/boundary/tests/failinject_products.rs` also checks that its name appears in no product).
There is no environment variable or exported symbol.

Example (an integration test in `crates/boundary/tests`, or any crate with that dev-dependency):

```rust
use liella_boundary::selftest::failinject::{self as fi, FailAlloc};

#[global_allocator]
static ALLOC: FailAlloc = FailAlloc;

#[test]
fn fts_open_without_memory() {
    // Trace: <the clause the ENOMEM path implements>
    fi::fail_at_least(1);         // every allocation fails
    let fts = call_fts_open(dir); // the function under test
    fi::off();                    // before any assertion, so a failing test does not
                                  // leave the thread's allocations failing
    assert!(fts.is_null());
    assert_eq!(liella_boundary::errno::get(), Errno::ENOMEM.get());
}
```

To find every allocation a function makes, step `fail_nth(1)`, `fail_nth(2)`, ... until the call
succeeds with `injected()` unchanged; each step checks that one failure point's error path.
