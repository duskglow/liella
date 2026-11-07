# STATIC-PIE-BACKTRACE-BIAS-TEST(-2): backtrace in a static PIE (load bias) tested and planted

**State:** branch `task/static-pie-backtrace-bias-test-2` (base origin/next; stacks on
`task/static-pie-backtrace-bias-test`), head: see `git log -1`. Code commits: `90f554a4a` tests + plants file,
`d4a6f41e8` clippy fix (unused `c_run` wrapper), `d7e3818e2` link kind passed by argument (run -2). Role keke:~/w/n4.
Decisions in force: D-27.12 (one source for the static PIE's bias), D-27.10 (rcrt1 start), D-2.7 (frame texts).
No library code changed: no test found a library defect (the PIE build passed every check in every run).

## What is there
* `conformance/c/s02_backtrace_static_pie.c` (`LINK-STATIC-PIE: relr`, `RUN-ARGS: pie`). The builder names the link
  kind in the only argument, `pie` or `fixed`; the program checks the file's ELF type (ET_DYN / ET_EXEC) and the bias
  (non-zero / zero). The bias is found without the library: `getauxval(AT_ENTRY)` minus `e_entry` of
  /proc/self/exe; the image extent is the file's PT_LOAD span moved by it. Then: every `backtrace()` address inside
  the extent; frames 0..3 are leaf f2 f1 driver (`.text.sorted.*` layout); `backtrace_symbols` text =
  `argv0(+<addr - bias>) [<addr>]` and the offset + bias names the known function; `backtrace_symbols_fd` writes
  `argv0(+off)[addr]\n` lines, first offset names `leaf`.
* `crates/core/tests/s02_diagnostics_clauses.rs`: `c_run_linked(cell, name, link, args)` with
  `Link::{Static, StaticPie}`; `c_clause_linked` passes the kind; `clause_passed` is the shared verdict. Six tests
  `s2_7_c_{1,2,3}_static_pie` / `s2_7_c_{1,2,3}_fixed_address`.
* Plants `handoff/STATIC-PIE-BACKTRACE-BIAS-TEST-plants.txt` on `crates/boundary/src/backtrace.rs`: drop-bias,
  bias-twice, offset-unbiased, each via `cargo xtask affected --only conformance:s02_backtrace_static_pie
  --with-exports --run` (the PIE build; the fixed build has bias 0 and cannot catch them by design).

## Run -2: why lint2 failed and the fix
lint2 (rc 101): `s2_7_c_1_fixed_address` FAILED "bias != 0" with bias 0. Cause: the program chose the expected
bias from `__PIE__`, but the host compiler builds `-fPIE` by default, so `__PIE__` is defined in the `-static`
(fixed-address, ET_EXEC, bias 0) build too. Test bug, not a library defect. Fix `d7e3818e2`: the kind comes from
the harness (argument), and the ELF type check makes a wrongly-linked build fail instead of passing vacuously.

## Jobs (keke, run -2, final head)
* `static-pie-backtrace-bias-test-2-lint` rc 0: clippy, static, so, run.sh (2 passed), `s2_7_c_` (9 passed).
  Script ~/work/STATIC-PIE-BACKTRACE-BIAS-TEST-2/lint.sh.
* `static-pie-backtrace-bias-test-2-gate` rc 0: `affected --run --base origin/next` under ~/bin/gate: 88 passed,
  conformance 1 passed, peak 1.2G.
* `static-pie-backtrace-bias-test-2-plants` rc 0: 3 CAUGHT (drop-bias, bias-twice, offset-unbiased), 0 SURVIVED,
  0 WARN-RC-ONLY, 0 WARN-NO-MSG; one WARN-DEV-SHM (the RAM dir is under /dev/shm, as the brief prescribes for this
  host; no test here mounts its own /dev/shm). RAM dir /dev/shm/claude/static-pie-backtrace-bias-test-2/plants-1.
* Report json: handoff/reports/STATIC-PIE-BACKTRACE-BIAS-TEST-2.json.
* Superseded: run 1's lint (rc 1), lint2 (rc 101, the test bug above), gate/plants (killed before start),
  gate2/plants2 (old head; stopped by me with kill-label in run -2 to free slots, plants2 had a passing baseline).

## Open / next step
None for this story once the plants job is green. Plan rows: D-27.12, D-27.10, D-2.7.
