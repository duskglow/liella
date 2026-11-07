# TVA-CONFORMANCE-ONCE: a conformance program runs once per products build

State: branch `task/tva-conformance-once`, code head `19d12d161` (cut from origin/next `5a12901d4`), role keke:~/w/r2.
Status: NOT READY - code done and pushed; gate, plants and the before/after measurement are queued on keke (slot queue
~5 jobs deep, running jobs 75+ min old at 09:45Z). Nothing below marked NOT VERIFIED has been run yet.

## What changed
- `conformance/run.sh` (D-X.38): after a program passes it writes `conformance/build/<name>.pass` = sha256 of the
  built program + sha256 of the inherited process state (`env | sort`, `ulimit -a`, `umask`, SigBlk/SigIgn/Uid/Gid/
  Groups of the shell, `ls /proc/$$/fd`, `pwd`) + the program's link key (`$key`: products' hash from
  `.products.key`, libm, headers, source with directives, compiler, flags). A later call with the same stamp counts
  the program as passed without running it and prints `conformance: N not rerun (...)` before the usual summary line.
  A failed run removes the stamp; LINK-HELPERS (reference-only) runs are never recorded; the pass file is named
  before RUN-SECURE repoints `$exe` at its `$TMPDIR` copy. `LIELLA_CONFORMANCE_RERUN=1` forces every run.
- The runner unsets `NEXTEST_*`/`CARGO_*` (after reading CARGO_TARGET_DIR) before running programs: they vary per
  test/run/package, so keyed they would defeat every skip; no program under conformance/c reads them (grep).
- No Rust helper change: the stamps live in run.sh (the brief's preferred "reuse run.sh's own stamps"), so every
  `conformance()` copy (boundary symbol suites, core clauses suites, `products/mod.rs`), `cargo xtask ctest` and
  `affected` get it without edits.
- Tests: `crates/boundary/tests/conformance_runner.rs` (fixture root in temp_dir with a copy of run.sh, the real
  products symlinked, a copied rcrt1.o): repeat skipped (and runner variables ignored, RERUN=1 honoured); new
  products hash reruns (program bytes asserted unchanged); changed environment reruns; a failed run records nothing.
- Docs: conformance/README.md paragraph; plan row D-X.38 in plan/07-decisions/x-implementation-decisions.md.

## Isolation (brief step 3)
run.sh DID pass the test process's environment, rlimits, signal mask/ignored set, umask, descriptors and cwd into
programs (plain fork/exec from the test's `sh`). All of these are now in the key, so a call from a test that changed
any of them is not skipped. Not in the key: state outside the process (files, cgroup limits, clock, load).

## Jobs (keke; re-attach: `ssh keke '~/bin/jobwait tvaco-gate tvaco-plants tvaco-measure'`)
- `tvaco-gate`: `cargo xtask clippy && cargo xtask static && cargo xtask affected --run` (direct tier; .changed base origin/next). NOT VERIFIED.
- `tvaco-plants`: `cargo xtask plants handoff/TVA-CONFORMANCE-ONCE-plants.txt --dir /dev/shm/claude/TVA-CONFORMANCE-ONCE/plants-2`; 7 plants, all with msg:. NOT VERIFIED.
- `tvaco-measure` (`~/work/TVA-CONFORMANCE-ONCE/measure.sh`; waits for tvaco-gate's products): per-call seconds of
  `run.sh s22_symbols` run vs cached; then all boundary `_symbols` suites + s00_conventions_clauses under strace,
  phase `before` (LIELLA_CONFORMANCE_RERUN=1 = old behaviour) and `after` (fresh stamps), with exec counts of
  `s22_symbols`, run.sh and all programs. Read: `grep MEASURE ~/logs/tvaco-measure.log` and test-s per phase:
  `grep -E 'MEASURE phase|PASS \[' ~/logs/tvaco-measure.log | awk '/MEASURE phase/{p=$3;next}{sub(/.*PASS \[ */,"");sub(/s\].*/,"");t[p]+=$0} END{for(k in t)print k,t[k]}'`
  Expected: after-phase `execs s22_symbols` = 1. Brief step 1 threshold (stop if < ~150 test-s/train) NOT VERIFIED:
  the queue made measuring first cost a full queue cycle, so code and measurement were run together.

## Static counts (grep of `conformance(&[` calls)
s22_threads_symbols 199 calls / 26 distinct sets (s22_symbols 102x), s12_stdio 110/10, s01_startup 36/12,
s02 18/6, s18 14/4, s17 11/3, s11 11/1, s00_conventions_clauses 11/8; 68 distinct programs over these suites.

## Next step
Collect the three jobs. Gate/plants red: fix and rerun only the failing suite. Measure: fill in -N test-s; if the
before-after difference is under ~150 test-s per train, the coordinator decides whether to keep the change.
