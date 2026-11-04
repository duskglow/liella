# MATH-M6-F64-GAP-BREAKDOWN handoff (2026-10-24): measurement and plan only, COMPLETE

**State:** branch `task/math-m6-f64-gap-breakdown` (cut from `task/math-m6-f64-fast-entry-3`, e7dfac98); head in
handoff/reports/MATH-M6-F64-GAP-BREAKDOWN.json. Role keke:~/w/n3 (`--base origin/task/math-m6-f64-fast-entry-3`).
No library code changed. Plan rows: M6-1/M6-4/M6-5, D-10.13 (unchanged).

**Result:** docs/saturation-register.md section 13 (13.1 exp, 13.2 exp2, 13.3 log/log2/log10, 13.4 pow, 13.5 ranked
candidates). This is the plan the coordinator briefs stories from. (The brief asked for "the style of section 14.1";
there is no 14.1 on this branch or on next, so the section follows the 12.x style and takes the next free number, 13.)

**Method (also in the section):** exact instructions:u per call from perf stat at two rep counts (it reproduces the
bench: exp 344.0, log 295.4, pow 761.5; glibc on the same probe 67.0 / 63.0 / 141.0); per function, perf record
sampling to mark executed basic blocks in the objdump of the static probe, counted once per call (91-94% of the exact
totals attributed); inside a function, a split by source operation count (estimates, marked ~). Rejected: per-sample
shares as counts (Zen 1 skid piles samples after stmxcsr; perf also throttled the first run to about 1.5% of samples),
and a fall-through static walk (the hot direction is not the fall-through; it walked log into the accurate path).

**Findings:** in the static libm.a the kernel pieces are separate calls and about a third of the executed instructions
are loads/stores/moves; `exp_log::reduce` alone is ~106 of exp's 344 (full double-double chain, `x.lo = 0` not used);
double-double assembly (accurate `Dd::add`, Dekker products) is the largest phase in the logs (~89-133).
The minimum with FMA sits at glibc's totals (exp ~62); without FMA about 1.35x glibc.

**Ranked next stories (13.5):** 1 Fast2Sum-based assembly (~345 summed), 2 three-part exp reduction (~220),
3 fused inlined fast function (~180), 4 inline MXCSR read + cold fallback (~165), 5 precomputed rounding-test constant
(~110, first count the fallback rate), 6 pow r³/3 (~40), 7 log2/log10 scaled tables (~72), 8 FMA path (~335, RULING
NEEDED on run-time dispatch).

**Open / NOT VERIFIED:** the Ziv fallback rate per function (part of the 21-49 unattributed); whether libm.so inlines
differently from libm.a (the bench links libm.a); the projections in 13.5.

**Not to redo:** the probe and scripts are on keke `~/work/math-m6-f64-gap-breakdown/` (probe.c, run.sh, run2.sh,
phase.py, exec.py, path.py). Rerun run2.sh after a code story to get its per-function counts.

**Jobs (keke):** `math-m6-f64-gap-breakdown-so` (cargo xtask so, rc 0), `-phases` (exact totals, rc 0),
`-blocks` (executed-block counts, rc 0), `-gate` (affected --run on the docs commit).

**Next step:** brief candidate 2 (exp reduction) and 1 (assembly) as the first stories; count the fallback rate in
the sweep before 5.
