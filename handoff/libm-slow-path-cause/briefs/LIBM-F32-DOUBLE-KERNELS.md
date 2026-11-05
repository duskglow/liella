# LIBM-F32-DOUBLE-KERNELS: every float function from one double-precision kernel and a rounding test

Id: LIBM-F32-DOUBLE-KERNELS
Kind: code
Priority: 90
Size: L
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 90 tool calls / 180k

## The change

**What was measured** (handoff/libm-slow-path-cause/report.md §3.5):
* `float` inverse trig and hyperbolic functions have no `float` kernel. `Real for f32` sends them to the
  double-double path: asinf 1,556 and atanf 1,379 instructions, against the reference's 53-95.
* erff, lgammaf and tgammaf run the 384-bit mp, the same cost as `double`: erff 228,016 instructions.
* sinf has a kernel, but at 142 instructions against the reference's 35.

**The design.** A `float` result needs about 2^-45 relative in double: one evaluation in plain double with a proven
bound, then `fast32::round`. That fails about 2^-20 of the time and falls back to the existing path.

**Add kernels to `fast32`** for:
* asinf, acosf, atanf, atan2f;
* sinhf, coshf, tanhf, asinhf, acoshf, atanhf;
* erff, erfcf;
* lgammaf, tgammaf: piecewise; the regions near lgamma's zeros may need a double-double step;
* j0f, j1f, y0f, y1f: a polynomial around each zero up to a table limit, then Hankel's form in double. Beyond the
  limit or near an untabulated zero, fall back.

**Shrink sinf/cosf/tanf** to a single double reduction with a degree 5-6 polynomial.

**Rules:**
* Coefficients come from the project Remez tool (D-10.3 owner addition), with the derivation recorded.
* Exhaustive sweeps are possible for binary32 (2^32 operands): run one per function as an ignored test in its own
  job.

**Target:** at most 1.05x per `float` row (the special-function rows may record a residual near zeros).

**JOBS:**
* the gate (direct tier);
* plants:
  * the bound;
  * a coefficient;
  * a reduction edge;
  * a Bessel zero-table entry;
* the accuracy check (the exhaustive binary32 sweep per new kernel, against the accurate path, in all four modes);
* counts before and after (`cbench_libm`: all `float` rows touched).
