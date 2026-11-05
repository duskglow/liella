# LIBM-F64-INVTRIG-HYPOT-TIER: a fast tier for atan, asin, acos, atan2 and hypot

Id: LIBM-F64-INVTRIG-HYPOT-TIER
Kind: code
Priority: 90
Size: M
Host: -
Stacks-on: LIBM-MP-CORE-SPEED
Model: opus
Effort: high
BUDGET: 80 tool calls / 160k

## The change

**What was measured** (handoff/libm-slow-path-cause/report.md §3.3): these functions have no fast tier, so every
call runs the double-double path.

| Function | Cost (instructions) | Reference | Where the time goes |
|---|---|---|---|
| atan | 1,345 | 79 | 73% in `atan_dd`; two double-double divisions when \|x\| > 1 |
| asin | 1,517 | 73 | |
| atan2 | 1,577 | 163 | |
| hypot | 3,713 | 64 | a 256-bit exact soft-float sqrt on every call |

**Build Ziv's first step**, falling back to the existing paths unchanged:
* **atan**: a 64-entry `atan(j/64)` table and a degree 7-9 odd polynomial in double. Avoid the reciprocal for
  \|x\| > 1 by using `atan(x) = π/2 − atan(1/x)` with a hardware division and the error in the bound. Then one
  double-double add, and a bound of about 2^-66.
* **asin and acos**: through the same kernel, using a hardware `sqrtsd` on the exact `(1 − a)(1 + a)`.
* **atan2**: the quotient's error enters the bound.
* **hypot**: the sum of squares in double-double, `sqrtsd`, one correction step, and an exact-root check.
* **FMA variant** through dispatch. The results are bit-identical, so test both.

**Start from task/perf-libm-invtrig.** It holds an unverified `invtrig.rs` with exactly these kernels and an error
sketch. Verify its proof, wire the rows (`s10_math.tsv`, then `cargo xtask math-gen`), and test.

**Target:** at most 1.05x with FMA (atan, asin, acos, atan2, hypot, and their f32 rows if they are free here).
Otherwise record the residual in docs/saturation with the first-principles count.

**JOBS:**
* the gate (direct tier);
* plants:
  * the bound;
  * a table entry;
  * the \|x\| > 1 branch;
  * the hypot exact-root check;
* the accuracy check (a random sweep against the accurate path in all modes, plus hard cases);
* counts before and after (`cbench_libm`: atan, asin, acos, atan2, hypot, with and without FMA).
