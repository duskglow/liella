# GATE-REMEZ-CHECK-CACHE: parallel `remez --check` with a content-keyed verdict cache

State: branch `task/gate-remez-check-cache-2` (GATE-REMEZ-CHECK-CACHE-2; continues `task/gate-remez-check-cache`,
origin/next 5a12901d4 merged in); head: see `git log -1` / handoff/reports/GATE-REMEZ-CHECK-CACHE-2.json.
Decision in force: **D-10.58** (plan/07-decisions/s10-mathematics.md; was D-10.54, which belongs to
LIBM-F64-EXP-QUICK-2), on top of D-10.12 / D-10.3.

## What changed (both runs)
- `crates/mathtools/src/bin/remez.rs`: fits run on up to `available_parallelism()` threads (`std::thread::scope`,
  shared index); each fit's stdout/stderr is buffered and printed in command-line order, followed by its
  `step: remez/<fit> <seconds>s` line (XTASK-TEST-STEP-TIMES contract; the time is measured in the worker, so a
  replayed verdict prints ~0.0s and a failing fit is timed too); exit 1 if any fit fails or goes unreported.
- `crates/mathtools/src/check_cache.rs`: std-only SHA-256 (FIPS 180-4), `check_key` over tagged, length-prefixed
  fields (process image = `current_exe` + every file in `/proc/self/maps`; mode; fit argument; display path; fit
  bytes; output path; coefficient bytes or "absent"); stamps in `<target>/remez-check/<key>` (temp + rename).
  Only `--check` uses it; a failing fit stores nothing; `LIELLA_REMEZ_NO_CACHE=1` off; `LIELLA_REMEZ_CACHE_DIR` moves it.
- `crates/mathtools/tests/check_cache.rs`: FIPS vectors, key-per-input, the binary on three small fits (order,
  cold/warm/uncached equality, failing last/first fit, no stamp on failure, generate never stamps) and, since -2,
  the step lines: one per fit in command-line order, each right after its fit's verdict (cold and warm), one per
  fit with a failing fit, `<seconds>` with one decimal. Step lines are left out of the cold/warm report comparison
  (their times differ between runs; keeping them in would be a flake).
- `xtask/src/main.rs`: comment only (the per-fit times overlap, so "step time less theirs = tool build" no longer holds).

## -2 run (merge with next, renumber, re-prove)
- Merge conflict was only the step-line loop vs the parallel driver; resolution above. D-10.54 -> D-10.58 in the
  row and its 6 citations; the row now records the step lines.
- Jobs (keke r1, merged head): `gate-remez-check-cache-2-gate` (clippy, full-features clippy, static, then
  `cargo xtask affected --run --base origin/next`), `gate-remez-check-cache-2-plants` (handoff/GATE-REMEZ-CHECK-CACHE-2-plants.txt,
  a-e as before plus f/g/h on the step lines, from RAM snapshot src-plants, dir .../plants-1),
  `gate-remez-check-cache-2-time` (cold then warm `/usr/bin/time -v cargo xtask remez --check crates/math/fits/*.fit`
  in RAM snapshot src-time with a new cache dir; also checks step-line order and cold==warm reports).
  Results: see the report json / hand-back (RESULTS below are filled in when read).
- lint0 (clippy+static on the inherited head, step 0 of the brief) waited 45 min in keke's queue and was cancelled
  (kill-label); the same checks run first in the gate job on the merged head, which contains the inherited head.

RESULTS (code tree 2426f3b2a, keke 7-CPU slots, load 15-20):
- gate-remez-check-cache-2-gate rc 0: clippy, full-features clippy, static clean; direct tier 32 suites ok, 11121
  passed, 0 failed, 1245 ignored; conformance 125 passed; peak 13G, oom_kill=0.
- gate-remez-check-cache-2-plants rc 0: 8 plants, 8 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (WARN-DEV-SHM only).
- gate-remez-check-cache-2-time (rc 1, SUPERSEDED): cold 7:26 wall (446 s, 294% CPU), 22 up to date, 0 cached;
  warm 0.19 s, 22/22 cached; 22 step lines each; cold == warm reports. Slowest steps cold: fast32_tgamma 412.6s,
  fast32_lgamma 277.5s, fast32_erfcx2 133.1s; warm all 0.0s. Its rc 1 was the script's own order check: the expected
  list came from `ls` (locale sort puts fast32_erf elsewhere) while the command's glob is byte order; the step lines
  matched the glob order and the binary's verdict order exactly (checked by hand). time2.sh takes the reference
  from the same glob; job gate-remez-check-cache-2-time2 reruns cold/warm with a new cache dir.
- gate-remez-check-cache-2-time2 rc 0: cold 7:27.75 (448 s, 293% CPU), 22 up to date, 0 cached; warm 0.19 s,
  22/22 cached; cold == warm reports; step lines in command-line order cold and warm.

## Earlier measurements (run 1, keke 7-CPU slot, load 22-35; before the merge)
serial 1261 s -> parallel cold 488 s (293-297% CPU) -> uncached 444 s -> warm 0.19 s; `tables --check` 5.7 s (not
cached). Cold bound = longest fit (fast32_tgamma). Run-1 gate grcc-gate and plants grcc-plants-2 (5 CAUGHT) were on
the pre-merge head f1021777e.

## Open / informational
- `cargo xtask affected` does not select `remez --check` for a change under `crates/mathtools/src/`; recommendation
  (follow-up story): map `crates/mathtools/src/{remez.rs,bin/remez.rs,check_cache.rs,mp/}` to `remez --check <all
  fits>` (costs a full parallel re-fit, ~8 min on a 7-CPU slot, per change to the tool, since the key covers it).
- Any reader that sums `step: remez/*` lines to get the remez step's time now over-counts (the fits overlap); the
  gate's own `step: remez-check` line is the wall time.
- Next step: none required once the three jobs are green.
