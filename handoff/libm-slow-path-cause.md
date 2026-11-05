# LIBM-SLOW-PATH-CAUSE handoff (planning; no product code changed)

**State.** Branch task/libm-slow-path-cause, cut from origin/next 6acde4ea. The documents are in
handoff/libm-slow-path-cause/ (report.md and briefs/).

**Result.** Correct rounding is not the cause. Most families have no Ziv fast step, so every call runs the slow
path:

| Class | Functions | Path | Cost (instructions) |
|---|---|---|---|
| 10-60x | f64/f32 atan family, expm1, log1p, hyperbolic, cbrt | double-double on every call | 1,000-1,600 |
| 10-60x | hypot | 256-bit exact on every call | 3,700 |
| 3-5x | sin/cos/tan, exp/log/pow | fast step exists (98.9-99.999% of calls), but it is a Dekker double-double kernel; directed modes pay the 256-bit `elem::finish` | 220-480 |
| 300-9,400x | special functions and every long double / f128 call | 384-bit mp on every call, half the time in `__udivti3` and `pack`; `ln10()` recomputed per call | 34,000-630,000 |

**Requirement.** The spec (§10.0 Accuracy) asks only for the reference's ulp bounds. Correct rounding everywhere is
a decision (D-10.13/15/19/20, PRINCIPLES §4), and PRINCIPLES §4 also requires near-parity fast paths. The black-box
probe shows the reference is not correctly rounded on any measured function: it differs on 0.01-54% of inputs, by up
to 4 ulp, and by 791 ulp for j0.

**Decisions in force.** D-10.3, D-10.13, D-10.15, D-10.19, D-10.20, D-10.24. No new row was written: the contract
change is RULING NEEDED R1 and R2 in report §5.

**Open questions.** R1, the contract per family: recommend A, correct rounding with fast tiers, recording a residual
for lgamma/tgamma/Bessel/x87. R2: measure x87 rows in time.

**Do not redo.** The measurement harness is at mei:~/work/libm-slow-path-cause/harness. `run.sh` produces the rates,
costs and profiles; `run2.sh` runs the agreement probe. The output is in mei:~/work/libm-slow-path-cause/out/ and
~/logs/libm-slow-path-cause-{percall-2,agree}.log. The jobs and their logs are listed in report.md.

**Next step.** The coordinator gets R1 and R2 ruled, then dispatches LIBM-MP-CORE-SPEED first (no ruling needed),
then LIBM-F64-FAST-KERNELS and LIBM-F32-DOUBLE-KERNELS. LIBM-F64-INVTRIG-HYPOT-TIER and
LIBM-F64-EXPM1-HYPER-CBRT-TIER absorb the running PERF-LIBM-INVTRIG / -EXPM1-LOG1P / -HYPERBOLIC work. After that
come LIBM-SPECIAL-FAST-TIER (replacing PERF-LIBM-SPECIAL) and LIBM-WIDE-DD-TIER.

**Plan rows to read.** D-10.3, D-10.13, D-10.15, D-10.19, D-10.20 in plan/07-decisions/s10-mathematics.md;
docs/PRINCIPLES.md §4.
