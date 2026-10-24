# Testing notes

See also `nextest.md` (running the suites) and `testing-filesystems.md` (tests that depend on a
file system's capabilities).

## Making allocations fail (`liella::failinject`)

Hoarding memory from outside the process (an address-space limit, a memory hog) does not make
Liella's internal allocations fail where a test needs them to, so `ENOMEM` paths deep inside a
function (a directory walker's node, a stream's buffer, `qsort`'s scratch array) cannot be
reached that way. Test builds have a per-thread hook instead (ALLOC-FAIL-INJECT, coordinator
#2020):

| Call | Effect on the calling thread |
|------|------------------------------|
| `liella::failinject::fail_nth(n)` | the `n`-th allocation from now fails (1: the next), once; then off |
| `liella::failinject::fail_at_least(k)` | every allocation of at least `k` bytes fails, until `off` |
| `liella::failinject::off()` | allocations succeed again (also disarms an unspent `fail_nth`) |
| `liella::failinject::injected()` | how many allocations the hook has failed on this thread so far |

It covers both allocation domains: the internal fallible collections (`TryVec` growth, including
`with_capacity`, `reserve`, `reserve_exact` and every growth through them, `TryBox::new`) fail with
`ENOMEM`, and the caller-owned `malloc`/`calloc`/`realloc` wrappers (`strdup`, `FTSENT` blocks,
`scandir` entries, ...) return NULL with `errno` set to `ENOMEM`, exactly what exhausted memory
produces. Only allocations that reach the allocator count: a `reserve` its spare capacity already
covers is not one. Other threads are not affected. A few places that call `try_reserve` on a
plain `Vec` directly (outside `liella::mem`) are not covered.

The hook exists only with the `selftest` feature, which `liella-boundary`'s own dev-dependency
enables for the test suites and no product ever does: the shipped libraries contain none of its
code (`crates/core/tests/failinject_products.rs` checks that its name appears in no product), and
there is no environment variable or exported symbol.

Example (a test in `crates/core/tests` or `crates/boundary/tests`):

```rust
#[test]
fn fts_open_without_memory() {
    // Trace: <the clause the ENOMEM path implements>
    liella::failinject::fail_at_least(1);   // every allocation fails
    let fts = call_fts_open(dir);           // the function under test
    liella::failinject::off();              // before any assertion, so a failing test does not
                                            // leave the thread's allocations failing
    assert!(fts.is_null());
    assert_eq!(liella_boundary::errno::get(), Errno::ENOMEM.get());
}
```

To find every allocation a function makes, step `fail_nth(1)`, `fail_nth(2)`, ... until the call
succeeds with `injected()` unchanged; each step checks that one failure point's error path.
