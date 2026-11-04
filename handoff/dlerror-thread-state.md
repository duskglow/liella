# DLERROR-THREAD-STATE: handoff (BLOCKED, no code change)

State: branch `task/dlerror-thread-state`, cut from `origin/next` at c3424c99. The only commit adds this
note and `handoff/reports/DLERROR-THREAD-STATE.json`. No code, test or generated file was changed.

## Why it is blocked

The story asks for a test of clause `s27_1_c_5` (dlerror per-thread state) plus two plants on the
implementation: (a) one process-wide error slot, (b) no clear on read. Liella has no implementation
to test or plant yet:

* `xtask/src/compat.rs` `DEFERRED` lists `dlerror`, `dlsym`, `dlopen`, `dlclose`, `dlvsym`, ...: deferred
  to phase 14.
* `crates/boundary/src/posix/s27_dynlink.rs` and every `crates/ld/src/*.rs` module say "Implementation
  phase: 14 ... Not yet implemented". The only §27 entry points are `dl_iterate_phdr`, `_dl_find_object`
  and `__x86_get_cpuid_feature_leaf`.
* `validation/results/libc-a-missing-manifest-symbols.txt` lists `dlerror` and `dlsym` as missing.
* Planned design: `plan/ph14/design.md` (dlerror: per-thread state in a Liella thread-block slot, text
  built at failure; tests "dlerror per thread"), decision D-27.2 (`plan/07-decisions/s26-29.md`).

A test against Liella cannot link or would only skip, and the plants have no code to mutate, so the
brief's "both CAUGHT" cannot be met. A glibc-only probe would not prove anything about Liella. The
§22 clause file keeps phase-14 clauses as stubs ("Pending: phase 14"); `s27_dynlink_clauses.rs` is
still `@generated` (no specified tests), so it was left alone.

## RULING NEEDED

Re-queue this story after the phase-14 story that implements `dlerror`/`dlsym` (D-27.2). Recommended:
fold the test design below into that story's acceptance criteria rather than keep it as a separate
story. The alternative, a minimal `dlerror` + failing `dlsym` before the loader exists, adds public
symbols ahead of phase 14 and needs the owner's ruling. Not recommended.

## Test design to carry forward (not written, nothing run)

In `crates/core/tests/s27_dynlink_clauses.rs`, replace the `s27_1_c_5` stub with a real `#[test]` of the
same name that runs a C program linked against the Liella products (and the same source against glibc
as the oracle):
1. A fresh thread's first `dlerror()` is `NULL`.
2. After `dlsym(RTLD_DEFAULT, "<name that does not exist>")`, `dlerror()` is non-`NULL` and the next call
   is `NULL`.
3. Thread A fails a `dlsym` and does not read `dlerror`. Barrier. Thread B calls `dlerror()` and sees
   `NULL`. Barrier. A calls `dlerror()` and sees its own (non-`NULL`) text. Run a fixed number of
   iterations (for example 1000), with no wall-clock limit and no sleeps.
Plants for that story: (a) the per-thread slot replaced by one static slot (step 3 fails: B sees A's
error), (b) the clear on read removed (step 2 fails). Each plant run under `timeout 600`.

## Next step

The coordinator decides between re-queueing after phase-14 `dlerror` and folding into it. Nothing to
redo here.
