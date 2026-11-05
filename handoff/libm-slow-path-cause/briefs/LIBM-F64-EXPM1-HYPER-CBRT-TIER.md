# LIBM-F64-EXPM1-HYPER-CBRT-TIER: a fast tier for expm1, log1p, the six hyperbolic functions and cbrt

Id: LIBM-F64-EXPM1-HYPER-CBRT-TIER
Kind: code
Priority: 85
Size: L
Host: -
Stacks-on: LIBM-F64-FAST-KERNELS (reuses its exp and log kernels)
Model: opus
Effort: high
BUDGET: 90 tool calls / 180k

## The change

**What was measured** (handoff/libm-slow-path-cause/report.md §3.4): none of these functions has a fast tier, so
100% of calls run the double-double path.

| Function | Cost (instructions) | Reference | Where the time goes |
|---|---|---|---|
| expm1 | 1,339 | 86 | 55% `exp_reduced` |
| log1p | 1,306 | 82 | |
| sinh, cosh, tanh | about 1,510 | 128-141 | |
| asinh, acosh, atanh | about 1,300-1,500 | 87-135 | |
| cbrt | 1,025 | 148 | seven Newton steps, each with a division |

**Build the fast tier**, each with a proven bound of about 2^-66, a rounding test in every mode, and the existing
path as the fallback:
* **expm1**: the exp kernel, with the −1 folded in before the last double-double add. For small \|x\|, a direct
  polynomial in double with a double-double head.
* **log1p**: the log kernel on `1 + x` split exactly (`two_sum`).
* **sinh, cosh, tanh**: one shared exp reduction (`e^x` and `e^-x` from one table lookup), and expm1 for small
  \|x\|.
* **asinh, acosh, atanh**: from the fast log1p and log, in forms without cancellation.
* **cbrt**: an exponent / 3 bit-trick seed, a degree 2-3 polynomial refinement, one division-free Newton step in
  double, one double-double correction, and the perfect-cube check kept.

Use the FMA variant through dispatch: bit-identical by construction, so test both.

**Overlap:** PERF-LIBM-EXPM1-LOG1P and PERF-LIBM-HYPERBOLIC are tuning the same rows. Read their handoffs, keep what
was verified, and replace the rest by this design.

**Target:** at most 1.05x with FMA. Otherwise record the residual in docs/saturation with the count.

**JOBS:**
* the gate (direct tier);
* plants:
  * the bound;
  * the folded −1;
  * the `1 + x` split;
  * the cbrt seed constant;
  * the perfect-cube check;
* the accuracy check (random sweeps against the accurate path in all modes, plus hard cases);
* counts before and after (`cbench_libm`: expm1, log1p, the hyperbolic rows and cbrt, f64 and f32).
