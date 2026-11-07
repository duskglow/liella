# STATIC-PIE-BACKTRACE-BIAS-TEST: backtrace in a static PIE (load bias) tested and planted

**State:** branch `task/static-pie-backtrace-bias-test` (base origin/next), head: see `git log -1` (code commits
`90f554a4a` tests + plants file, then a clippy fix dropping the unused `c_run` wrapper). Role keke:~/w/n3.
Decisions in force: D-27.12 (one source for the static PIE's bias), D-27.10 (rcrt1 start), D-2.7 (frame texts).
No library code changed (no test found a defect so far: NOT VERIFIED until lint2 reports).

## What was added
* `conformance/c/s02_backtrace_static_pie.c` (`LINK-STATIC-PIE: relr`, so `conformance/run.sh` runs it as a static
  PIE from liella-rcrt1.o). The bias is found without the library: `getauxval(AT_ENTRY)` minus `e_entry` of
  /proc/self/exe; the image extent is the file's PT_LOAD span moved by it. Checks: bias != 0 under `__PIE__`, 0
  otherwise; every `backtrace()` address inside the extent; frames 0..3 are leaf f2 f1 driver (functions laid out in
  `.text.sorted.*`); `backtrace_symbols` text = `argv0(+<addr - bias>) [<addr>]` and the offset + bias names the known
  function; `backtrace_symbols_fd` writes `argv0(+off)[addr]\n` lines, first offset names `leaf`.
* `crates/core/tests/s02_diagnostics_clauses.rs`: `c_run`/`c_clause` generalised to `c_run_linked`/`c_clause_linked`
  with `Link::{Static, StaticPie}` (StaticPie: `-static-pie -fPIE -Wl,-z,pack-relative-relocs`, liella-rcrt1.o).
  Six tests: `s2_7_c_{1,2,3}_static_pie` and `s2_7_c_{1,2,3}_fixed_address` (same source, bias 0).
* Plants `handoff/STATIC-PIE-BACKTRACE-BIAS-TEST-plants.txt` on `crates/boundary/src/backtrace.rs`: drop-bias
  (`bias: 0`), bias-twice (`exe_bias(..).wrapping_mul(2)`), offset-unbiased (`Executable::offset` returns `addr`);
  each `msg:conformance FAILED: s02_backtrace_static_pie:` via `cargo xtask affected --only
  conformance:s02_backtrace_static_pie --with-exports --run`.

## Jobs (keke)
* `static-pie-backtrace-bias-test-lint` (SUPERSEDED): rc 1, clippy `-D dead-code` on the old `c_run` wrapper; fixed.
* `static-pie-backtrace-bias-test-gate`, `-plants` (SUPERSEDED): ended by me with kill-label before they started
  (queued on the pre-fix tree).
* `static-pie-backtrace-bias-test-lint2`: clippy, static, so, run.sh s02_backtrace_static_pie s02_backtrace, cargo
  test `s2_7_c_` (the six new tests and the old three). Queued at hand-back.
* `static-pie-backtrace-bias-test-gate2`: clippy, static, `affected --run --base origin/next` under ~/bin/gate. Queued.
* `static-pie-backtrace-bias-test-plants2`: `cargo xtask plants` on the file above, RAM dir
  /dev/shm/claude/static-pie-backtrace-bias-test/plants-2. Queued.
* Re-attach: `ssh keke ~/bin/jobwait static-pie-backtrace-bias-test-lint2 static-pie-backtrace-bias-test-gate2
  static-pie-backtrace-bias-test-plants2`. Time went to the keke slot queue (~35 min for the first lint job).

## Open / next step
1. Read the three jobs. If lint2 shows a FAIL in s02_backtrace_static_pie: decide whether the test or the library is
   wrong (a static-PIE-only failure with a correct test is a library defect in the bias use: fix it and say so).
2. If a plant survives: the fixed-address checks cannot catch any of them (bias 0), only the PIE build must.
Plan rows: D-27.12, D-27.10, D-2.7.
