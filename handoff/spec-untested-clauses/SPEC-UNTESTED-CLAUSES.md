# SPEC-UNTESTED-CLAUSES: the eight worst untested clauses, as briefs

Source: `handoff/spec-untested-clauses/untested-clauses.md`, produced by `cargo xtask trace --untested --out <file>`
(on this branch, origin/next base, 2026-11-04). It lists 62 untested clauses (1269 in the spec, 2 commentary items
not counted) across 22 of 216 sections. Most of them are section `Notes` (prose) and legacy or loader-internal surfaces.
I ranked them correctness and safety first: error paths, thread interactions, signal interactions. §27.1 (the
`dlfcn` interface, 13 of 14 clauses untested) is the largest real gap, and no `sym_dl*` test exists for
dlopen/dlsym/dlclose/dlerror.

Tag fixed in this story instead of briefed: `s24_5_c_3` (getdomainname is the NIS domain): `sym_getdomainname` in
`crates/boundary/tests/s24_system_symbols.rs` already compares getdomainname with uname's `domainname`, so it now
carries the clause ID.

Every brief below: Kind code; tests name their clause IDs (trace check); plants on the new tests are run with their
own `timeout 600` in a fresh RAM copy; final head passes `cargo xtask clippy` and `cargo xtask static`. Where the
expected result is glibc's observable behaviour, settle it with a black-box probe against the installed glibc first
and assert only what the spec and the probe agree on.

---

## DLOPEN-FAILURE-PATHS

Id: DLOPEN-FAILURE-PATHS
Priority: 80
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 50 tool calls / 100k

### The change

Clauses: `s27_1_f_0` (dlopen failure: `NULL` plus dlerror text for file not found, wrong ELF class, undefined symbol
with `RTLD_NOW`, static TLS exhaustion, version mismatch) and `s27_1_c_1` (repeat dlopen of one object returns the
same handle). Test to write: a conformance program (or a boundary test using `products::conformance`) that, for each
failure kind it can build without root (a missing path; a 32-bit ELF object if the toolchain can emit one, else
skipped with the reason printed; a helper object that references an undefined symbol, loaded with `RTLD_NOW`; a
helper that needs a version node the library lacks), checks that dlopen returns `NULL`, dlerror returns non-`NULL`
text, and a second dlerror returns `NULL`. Then that two dlopen calls of one good object return equal handles. Do
not assert the message text (the spec fixes no wording; probe glibc only to confirm `NULL` and non-`NULL`). Static
TLS exhaustion: skip unless a deterministic trigger exists, and say why. Proof: plants that (a) return a non-`NULL`
handle for an undefined symbol under `RTLD_NOW`, (b) leave dlerror `NULL` after a failure, (c) return a fresh handle
on a repeat load. All three must be CAUGHT.
JOBS: dlopen-failure-paths-gate, dlopen-failure-paths-plants

## DLERROR-THREAD-STATE

Id: DLERROR-THREAD-STATE
Priority: 78
Size: S
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 40 tool calls / 80k

### The change

Clause: `s27_1_c_5` (dlerror returns a description of the last failure on this thread and clears it; `NULL` if
there was no error). Test to write: (1) a fresh thread's first dlerror is `NULL`; (2) after a failing dlsym, dlerror
is non-`NULL` and the next call is `NULL`; (3) thread A fails a dlsym and does not read dlerror; thread B (barrier
ordered, no sleeps) then calls dlerror and sees `NULL`; A then still sees its own error. Repeat (3) a fixed number of
iterations, no wall-clock limit. Proof: plants that (a) keep one process-wide error slot, (b) skip the clear on read.
Both CAUGHT.
JOBS: dlerror-thread-state-gate, dlerror-thread-state-plants

## DLCLOSE-REFCOUNT-DTORS

Id: DLCLOSE-REFCOUNT-DTORS
Priority: 75
Size: M
Host: -
Stacks-on: DLOPEN-FAILURE-PATHS
Model: opus
Effort: high
BUDGET: 45 tool calls / 90k

### The change

Clause: `s27_1_c_4` (dlclose decrements the count; at zero it runs destructors and unmaps, unless `NODELETE` or the
object has unique symbols). Test to write: a helper object whose constructor and destructor write to a pipe or a
caller-provided counter. dlopen it twice, dlclose once: no destructor. dlclose again: destructor ran exactly once.
Then dlopen with `RTLD_NODELETE`, dlclose: no destructor, and the object is still found by `RTLD_NOLOAD`. Whether an
unloaded object's address range is unmapped: probe glibc and assert only if probe and spec agree; otherwise leave it
out and note it. Proof: plants (a) run destructors on every dlclose, (b) ignore `RTLD_NODELETE`. Both CAUGHT.
JOBS: dlclose-refcount-dtors-gate, dlclose-refcount-dtors-plants

## DLSYM-SPECIAL-HANDLES

Id: DLSYM-SPECIAL-HANDLES
Priority: 72
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 45 tool calls / 90k

### The change

Clauses: `s27_1_c_3` (dlsym, `RTLD_DEFAULT`, `RTLD_NEXT`) and `s27_1_c_1_3` (`dlopen(NULL)` returns the global scope).
Test to write: a conformance program linked against two helper objects that both define `f`. Check that `dlsym(RTLD_DEFAULT, "f")` is the
first in load order; `dlsym(RTLD_NEXT, "f")`, called from the first helper, is the second; `dlsym(dlopen(NULL, RTLD_NOW),
"main_sym")` finds an exported symbol of the executable; and a missing name gives `NULL` plus non-`NULL` dlerror.
Proof: plants (a) make `RTLD_NEXT` start at the caller's own object, (b) make `dlopen(NULL)` return a handle that
excludes the executable. Both CAUGHT.
JOBS: dlsym-special-handles-gate, dlsym-special-handles-plants

## DLOPEN-MODE-VISIBILITY

Id: DLOPEN-MODE-VISIBILITY
Priority: 70
Size: M
Host: -
Stacks-on: DLSYM-SPECIAL-HANDLES
Model: opus
Effort: high
BUDGET: 45 tool calls / 90k

### The change

Clause: `s27_1_c_1_2` (modes: `RTLD_LAZY`/`RTLD_NOW`, `RTLD_GLOBAL`/`RTLD_LOCAL`, `RTLD_NODELETE`, `RTLD_NOLOAD`).
Test to write: `RTLD_NOLOAD` on an unloaded object returns `NULL` and loads nothing (its constructor did not run);
on a loaded one it returns its handle. An object loaded `RTLD_LOCAL` is not found by `dlsym(RTLD_DEFAULT, ...)`. Reloading
it with `RTLD_NOLOAD|RTLD_GLOBAL` promotes it, and then it is found. With `RTLD_LAZY`, an object with an unresolvable
function reference loads; with `RTLD_NOW` it fails. Proof: plants (a) make `RTLD_NOLOAD` load the object, (b) treat
`RTLD_LOCAL` as global, (c) ignore the promotion. All CAUGHT.
JOBS: dlopen-mode-visibility-gate, dlopen-mode-visibility-plants

## TLS-GET-ADDR-LATE-MODULE

Id: TLS-GET-ADDR-LATE-MODULE
Priority: 76
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 50 tool calls / 100k

### The change

Clause: `s27_3_c_0` (`__tls_get_addr` returns the calling thread's address of a TLS variable; the block is allocated
on first access for modules loaded after the thread started). Test to write: start N threads and park them on a
barrier, then dlopen a helper with an initialised `__thread` array (non-zero `.tdata` plus a `.tbss` tail). Release
the threads. Each one reads the initial image (exact bytes, `.tbss` zero), writes a thread-unique pattern, and
reports its address. Check that the addresses are distinct, each thread sees only its own pattern, and a thread
created after the dlopen sees the initial image. Fixed iterations, barrier-ordered, no sleeps. Proof: plants (a)
share one block between threads, (b) skip copying `.tdata` on lazy allocation, (c) leave `.tbss` uninitialised (fill
with 0xAA). All CAUGHT. (`s27_3_f_0`, abort on allocation failure, needs OOM injection: leave it out and say so.)
JOBS: tls-get-addr-late-module-gate, tls-get-addr-late-module-plants

## PTHREAD-ERRNO-UNTOUCHED

Id: PTHREAD-ERRNO-UNTOUCHED
Priority: 68
Size: S
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 40 tool calls / 80k

### The change

Clause: `s22_n_2` (pthread functions return error numbers and do not set `errno`; `sem_*` use `errno`). First grep
`crates/*/tests` and `conformance/c/s22_*.c` for tests that already check this. If one does, add the clause ID to it
(tag fix) and stop. Otherwise, test to write: for a table of cheap failing calls (`pthread_mutex_trylock` on a
locked mutex -> `EBUSY`, `pthread_join` of self -> `EDEADLK`, `pthread_mutex_unlock` of an unowned errorcheck mutex
-> `EPERM`, `pthread_attr_setstacksize` too small -> `EINVAL`, `pthread_kill` with an invalid signal -> `EINVAL`),
set errno to a sentinel, call, and check that the return value is the error number and errno is still the sentinel.
Then `sem_trywait` on a zero semaphore returns -1 with errno `EAGAIN`. Probe glibc for each row first. Proof: plants
(a) make one pthread entry store its error in errno too, (b) make `sem_trywait` return the error number. Both CAUGHT.
JOBS: pthread-errno-untouched-gate, pthread-errno-untouched-plants

## PROFIL-SIGPROF-HISTOGRAM

Id: PROFIL-SIGPROF-HISTOGRAM
Priority: 62
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 45 tool calls / 90k

### The change

Clause: `s28_1_c_3` (profil samples the PC into a histogram on every `ITIMER_PROF` tick; sprofil takes several
regions). Test to write: profil a buffer over the test's own spin function, then spin for a fixed instruction-count
loop (not wall time) until at least one bucket is non-zero, with a hang guard from `support/timeout.rs`. Check that the
non-zero buckets map into the spin function's range under the given offset and scale. Then check that `profil(NULL, 0,
0, 0)` (or scale 0) stops sampling: the histogram is unchanged after another spin. Also check signal interaction:
a caller's own `SIGPROF` handler installed after profil is stopped stays installed, and profil does not leave
`ITIMER_PROF` armed. Probe glibc for the stop semantics first. sprofil: one extra region, same checks. Proof: plants
(a) bucket by the wrong scale formula, (b) leave the timer armed on stop. Both CAUGHT.
JOBS: profil-sigprof-histogram-gate, profil-sigprof-histogram-plants

---

## SPEC GAP (owner)

- `s27_1_c_8` (dl_iterate_phdr: "the loader lock is held during the call"). The spec does not say what a callback may
  do under that lock: call dlopen, dlclose or dlsym, or reach `__tls_get_addr` of a late module. It says neither
  "deadlocks" nor "is allowed", so the observable contract that could be tested is open. The existing
  `sym_dl_iterate_phdr` runs conformance program `s27_dl_iterate_phdr`: check whether it covers the per-object
  callback part, and if so tag it `s27_1_c_8` once the lock part is ruled.

## INFORMATIONAL (not briefed)

- 27 of the 62 are `Notes` fields: section-wide prose (`s4_n_*`, `s6_n_*`, `s8_n_*`, `s11_n_*`, `s13_n_*`,
  `s15_n_*`, `s22_n_*`). One of them, `s22_n_2`, is briefed above. Most of the others are not separately testable. Recommend that `cargo xtask stubs` emit `Notes` items as
  `commentary!` (or that the spec mark them COMMENTARY), so the list keeps only behaviour.
- Not briefed by design: `s3_4_*` (alloca is a compiler builtin, D-3.8); `s22_10_*` (libthread_db: first a
  "should Liella have it" ruling); `s27_2_c_0` (LD_AUDIT), `s27_4_c_0` (r_debug), `s27_5_c_0` (CPU features),
  `s27_1_c_2` (dlmopen), `s27_1_c_7` (dlinfo), `s27_1_c_6` (dladdr), `s27_1_c_1_1` (path search), `s27_1_g_0`: loader
  work, rank after the eight; `s28_1_c_4`, `s28_3_c_0` (empty hooks/fentry); `s29_*` (legacy RPC/NIS, see
  `handoff/legacy-rpc-nis.md`); `s0_6_c0_6_0` and `s10_0_c2_0`: probably covered by the ABI/version and
  `_finite` symbol tests, which lack the tag (check and tag; not verified here).
