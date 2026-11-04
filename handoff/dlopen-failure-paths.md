# DLOPEN-FAILURE-PATHS: blocked on phase 14 (M14-7), no product code to test

**State:** branch `task/dlopen-failure-paths`, cut from `origin/next` at `c3424c99`; the only commit adds this note
and `handoff/reports/DLOPEN-FAILURE-PATHS.json`. No code or test change.

## Why the story cannot be done now

The brief asks for a conformance program (or boundary test) that calls `dlopen`/`dlerror` against Liella for
`s27_1_f_0` and `s27_1_c_1`, plus three plants on the `dlopen` code. Liella has no `dlopen` family yet:

* `crates/ld/src/api.rs` is a three-line placeholder ("Not yet implemented", phase 14).
* `xtask/src/compat.rs` lists `dladdr dladdr1 dlclose dlerror dlinfo dlmopen dlopen dlsym dlvsym` as `DEFERRED`;
  `xtask/src/symcov.rs` asserts `dlopen` is a phase-14 gap; `docs/spec-symbol-gaps.md` row (a) lists the `dlopen`
  family as missing from `libc.so.6`; README: phase 14 "Not done".
* No Rust or C source in the tree defines `dlopen` or `dlerror` (a whole-tree search finds only tests, probes and xtask
  lists). A conformance program calling `dlopen` would not link against `libc.a`, and plants (a) to (c) have no code to
  mutate.
* `plan/ph14/milestones.md` M14-7 ("dlopen family ... the 13 s27_1 clause tests") owns these tests; it depends on
  M14-6, M14-5b, ..., M14-2.

## Behaviour already settled (do not re-probe)

`spec/behaviour/dynlink.md` (+ `dynlink.glibc.txt`, from `tools/probe/dl/dlprobe.c errors|tls`) already records
everything this test needs: NULL return and a dlerror text for a missing file, `EI_CLASS` 32-bit, an undefined symbol under
`RTLD_NOW`, a missing version node and static-TLS exhaustion; after every error a second `dlerror()` returns NULL;
opening one object twice returns the same handle. The probe's corpus builder (`tools/probe/dl/run.sh`) already
shows how to make each failing object without root (a 32-bit object by patching `EI_CLASS`, so no `-m32` toolchain is
needed).

**Static TLS:** a deterministic trigger exists: one `dlopen` of an object whose initial-exec TLS block is larger than
the surplus. R14-4 recommends a larger surplus for Liella than the reference's, so the test object must be sized to
exceed Liella's surplus (decided at M14-6), not the reference's measured value.

## Exact next step

Re-queue this story after M14-7 lands (or fold it into M14-7, whose exit already lists the s27_1 clause tests). Then:
a conformance program `conformance/c/s27_dlopen_failure.c` plus helper objects built like `tools/probe/dl/run.sh`,
wired from `crates/core/tests/s27_dynlink_clauses.rs` (`s27_1_f_0`, `s27_1_c_1`), and the three plants from the brief.
Plan rows to read: `plan/ph14/milestones.md` M14-6, M14-7; `plan/ph14/open-questions.md` R14-4.

## Jobs

* `dlopen-failure-paths-gate` (shiki, role sh1): clippy + static + direct-tier affected on this head (handoff-only
  change). No plants job: there is no code to plant.
