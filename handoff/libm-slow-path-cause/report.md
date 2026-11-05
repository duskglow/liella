# LIBM-SLOW-PATH-CAUSE: why libm calls cost 3x to 9000x the reference, and the design that fixes it

Planning story. No product code changed. Base: origin/next 6acde4ea. Evidence was measured on mei (role s1, Skylake-SP,
Xeon Platinum 8151) with a scratch harness that links `liella-math` directly (outside the repo:
`mei:~/work/libm-slow-path-cause/harness`, `run.sh` = rates, costs and profiles, `run2.sh` = reference agreement).
Jobs: `libm-slow-path-cause-percall-2` (rates, costs, profiles) and `libm-slow-path-cause-agree` (agreement probe),
both rc 0. Raw output: `mei:~/work/libm-slow-path-cause/out/result.log` and `~/logs/libm-slow-path-cause-agree.log`.

How the numbers were taken:
* **Cost per path** is `instructions:u` per call from `perf stat`: N calls minus the same run with the calls removed,
  divided by N. These are the Rust kernels called directly. The product rows also include the C entry and the MXCSR
  envelope. The `exp` and `log` product rows use the in-place `*_nearest` entries, so they are cheaper than the
  `Real::` path measured here: bench `exp` = 181, harness `Real::exp` = 283.
* **Fallback rate** comes from the `fastpath-stats` feature (`stats::record`): 10^6 calls per function on the bench
  operands (the same xorshift generator, seed and ranges as `bench/c/cbench_libm.c`) and 10^6 calls on random
  operands (random sign, log-uniform magnitude over a wide range of the domain, for example `2^[-20,16)` for the trig
  functions).
* **Accurate-path cost** of a function that has a fast tier was measured through the `binary32` generic entry
  (`trig::sin::<f32>`, `exp_log::exp::<f32>`). That entry skips the `double` fast tier and runs the same
  double-double code.
* **Profiles**: `perf record`, self time by symbol, at a 4% cut.

## 1. The answer in one paragraph

Correct rounding is not what makes Liella slow. What makes it slow is that most functions have **no cheap first
step**. Ziv's strategy is "evaluate cheaply with a proven bound, test the rounding, fall back rarely". Liella
implemented the fallback step and then, for most families, never built the cheap step. The result is three tiers:

1. **f64/f32 elementary functions with no fast tier** (atan, asin, acos, atan2, expm1, log1p, the six hyperbolic
   functions, cbrt, and every inverse-trig and hyperbolic `float` function): every call runs the double-double
   "accurate" path, at 1,000 to 1,600 instructions. hypot runs a 256-bit exact soft-float at 3,700. That is the
   10-60x class.
2. **The functions that do have a fast tier** (sin, cos, tan, exp, exp2, exp10, log, log2, log10, pow, and the
   fast32 rows): the fast tier answers 98.9% to 99.999% of calls. The tier itself is a double-double evaluation with
   Dekker products and no FMA, so it costs 220 to 480 instructions against the reference's 65 to 105. That is the
   2.7-4.6x class. In directed rounding modes, the fast-tier "success" exit runs the full exact-sum rounder
   (`elem::finish`), so sin in upward mode costs 865 instructions where round-to-nearest costs 314 (the 8.75x row).
3. **Special functions (all formats) and every long double / `_Float128` function**: every call is evaluated in
   384-bit `math::mp`. There is no fast tier, so the rate of reaching the slow path is 100% by construction. Every
   value comes from a tableless series: the erf series has about 80 terms, exp squares twelve times, atan halves
   eight times with a 384-bit sqrt and division each time. Half the time goes into software 128-by-64 division
   (`__udivti3` in `div_u64`) and normalisation (`pack`). Some constants are re-derived on every call: `mp::ln10()`
   runs the full ln series each time. That is the 300x to 9,400x class.

## 2. Requirement findings (question 2)

| Source | What it says | Forces the slow path? |
|---|---|---|
| `glibc-functional-spec.md` §10.0 "Accuracy" (lines 1684-1689) | "It guarantees correct rounding only for the IEEE-required operations: `sqrt`, `fma`, the narrowing arithmetic of §10.4, conversions, `fmod`, `remainder`, and the rounding and classification functions. Other functions are 'faithful or better, with documented bounds'. A reimplementation must at least match those bounds" | No. The spec asks for the reference's ulp bounds on transcendental and special functions, not correct rounding |
| Spec Appendix C.11 | "libm accuracy bounds ... Accuracy targets are set by the implementation plan's own tests (decision D-10.3)" | No |
| D-10.3 (`plan/07-decisions/s10-mathematics.md`) | "correctly rounded where the contract requires it. For every other function, a documented ULP bound per function and format ... at least as tight as the published bounds". Goal: "results identical to glibc's wherever glibc is correctly rounded" | No. It allows a ulp bound |
| D-10.13 (f32/f64 elementary, milestone 2) | "rounded once ... so every result is correctly rounded in every mode" | A design **choice**, recorded as a decision. For `double` it is not proved: `elem.rs` says the double-double path is "correctly rounded except where the exact value lies within about 2^-100 of a rounding boundary" |
| D-10.15 / D-10.20 (special functions) | "Evaluated in a 384-bit binary floating point ... so every result is correctly rounded in every mode" | A choice. The 384 bits are sized for the Bessel power series' 150-bit cancellation and then used for everything, erf included |
| D-10.19 (long double elementary) | "Each function is evaluated in `math::mp` (384 bits) and rounded once, so every result is correctly rounded" | A choice. x87 needs about 64 + 64 bits in the worst case, not 384 |
| docs/PRINCIPLES.md §4 | "Correct rounding where it is claimed is non-negotiable ... it uses a proven retry at higher precision (Ziv's strategy)" and "glibc's speed is still the bar for the common path; correctly rounded fast paths are expected to come close to it" | It requires proof where correct rounding is claimed, and it expects the fast path that most families lack |
| Owner rulings #1719 / #1758 | `_Float128` complex lands only with the full Ziv step (ZIV-C) | Scope: f128 complex rounding proofs. They do not extend correct rounding to every real function |

**Conclusion.** Correct rounding everywhere is a written decision (D-10.13/15/19/20, PRINCIPLES §4), not a spec
requirement: the spec asks only for the reference's documented ulp bounds. No written requirement forces the slow
path on the common call. The same documents that claim correct rounding also require a near-parity fast path, and
that fast path was never built for most families.

**Reference accuracy (black-box probe, `libm-slow-path-cause-agree`, round to nearest, Liella taken as the correctly
rounded side per its oracle-tested D-10.3 suites).** The reference differs from Liella on:

| Functions | Inputs where the reference differs | Largest difference |
|---|---|---|
| exp, log, exp10 | 0.01-0.1% | 1 ulp |
| sin, tan, atan, asin, atan2 | 0.03-0.25% | 1 ulp |
| hypot | 0.1-0.6% | 1 ulp |
| expm1, log1p | 1.5-6.8% | 1 ulp |
| sinf, asinf, atanf | 0.8-7.3% | 1 ulp |
| erf, erff | 2-10% | 1 ulp |
| tanh | 5-22% | 2 ulp |
| sinh | 21-26% | 2 ulp |
| lgamma, lgammaf | 29-32% | 4 ulp |
| tgamma | 33-47% | 4 ulp |
| cbrt | 46-54% | 3 ulp |
| j0 | 28-54% | 791 ulp (near a zero) |

So the reference is not correctly rounded on any of these functions. Liella cannot match its last bits in any
case: D-10.3 already says its particular approximations are not reproduced. A ulp-bound contract would therefore not
buy bit-identity either. It would only buy speed.

## 3. Per-family tables (questions 1, 3, 4)

Columns:
* **Fast share**: the share of calls the fast tier answers. "none" means there is no fast tier, so 100% of calls run
  the slow path. Values are bench / random operands.
* **Cost**: instructions per call, harness measurement.
* **Ref**: the reference's instructions per call (bench counters, `parity-libm-1105`).
* **Estimated after**: a first-principles estimate, explained in §4. It is **not measured**.

### 3.1 f64 trig (sin, cos, tan): a fast tier exists but is a double-double kernel

| | sin | cos | tan |
|---|---|---|---|
| Path | `trig::fast` (double-double reduction and evaluation, rounding test), else the double-double accurate path | same | same |
| Fast share, nearest | 99.36% / 99.42% | 99.38% / 99.29% | 98.86% / 98.87% |
| Fast share, upward | 98.72% / 98.83% | 98.74% / 98.60% | 97.72% / 97.71% |
| Cost, whole call | 314 (upward: 865) | 320 | 481 |
| Cost, accurate path | 1,591 (via the f32 entry) | similar | similar |
| Ref | 100 | 100 | 105 |
| Cause | The fast kernel computes a full double-double with Dekker `two_prod` (about 17 flops each, no FMA). Its bound is loose (fails ≈ 2^-7.3, implying ε ≈ 2^-61). In directed modes, a passing test still calls `elem::finish`: the 256-bit `arith::sum` plus `float::round`, about 550 more instructions | | |
| Proposed | Reduce with Cody-Waite in double plus one double-double correction, evaluate the polynomial in double (Estrin) on `r` with only `r·(1 + …)` kept in double-double, use a ~2^-66 bound, and use a directed-mode test as cheap as round-to-nearest (sign and magnitude of `yl`). FMA variant through the dispatch mechanism | | |
| Estimated after | 80-110 (FMA), 110-140 (no FMA) | same | 120-160 |

### 3.2 f64 exp, exp2, exp10, log, log2, log10, pow: a fast tier exists

| | exp | log | exp10 |
|---|---|---|---|
| Fast share | 99.9987% / 99.9989% | 99.9988% / 99.9990% | 99.998% / 99.998% |
| Cost (Real path / fast kernel / accurate) | 283 / 221 / 1,393 | 291 / 233 / (accurate path not measured separately) | 400 / (not measured) / (not measured) |
| Product row (in-place entry) | 181 | (see bench) | 7.48x |
| Ref | 67 | 84 | 53 |
| Cause | Double-double throughout the kernel (Dekker products). The bound is already tight (2^-70), so fallbacks cost nothing. docs/saturation/16 counts the floor at 1.35x without FMA | | |
| Proposed | Same as §3.1: double-only polynomial, one double-double step, FMA dispatch. `exp10` gets the same kernel as `exp` (a `log10(2)/64` split), not a double-double multiply by `ln 10` | | |
| Estimated after | 70-90 | 80-100 | 75-95 |

### 3.3 f64 inverse trig and hypot: no fast tier

| | atan | asin | atan2 | hypot |
|---|---|---|---|---|
| Path | Always `trig::atan_dd` (double-double: `atan(j/64)` table plus series, **two double-double divisions** when \|x\| > 1), then `elem::finish` | `atan_dd(a/√(1−a²))` (double-double sqrt and division) | quotient in double-double, then `atan_dd` | Always `arith::hypot_exact`: a 256-bit exact square root in soft float |
| Fast share | none (100% slow path) | none | none | none |
| Cost | 1,345 / 1,276 | 1,517 / 1,506 | 1,577 / 1,575 | 3,713 / 3,705 |
| Profile | 73% `atan_dd`, about 9% `arith::sum` + `float::round` | | | 85% in the inlined exact sqrt |
| Ref | 79 | 73 | 163 | 64 |
| Proposed | Fast tier: a 64-entry table, a degree-7 to 9 odd polynomial in double on the reduced argument, one double-double add, a ~2^-66 bound, and a rounding test. hypot: the sum of squares in double-double (FMA or Dekker), hardware `sqrtsd`, one correction step, and an exactness check. That pattern is in progress on `task/perf-libm-invtrig`, unverified | | | |
| Estimated after | 80-110 | 90-120 | 150-190 | 35-60 |

### 3.4 f64 expm1, log1p, cbrt, hyperbolic: no fast tier

| | expm1 | log1p | cbrt | sinh, cosh, tanh | asinh, acosh, atanh |
|---|---|---|---|---|---|
| Path | `expm1_small` series below 0.34, else `exp_dd` minus 1, in double-double | `log1p_dd` | 7 Newton steps **with a division each**, then a double-double Newton step and a perfect-cube check | built from double-double `expm1`/`exp` | built from double-double `log1p` |
| Fast share | none | none | none | none | none |
| Cost | 1,339 / 1,284 | 1,306 / 1,212 | 1,025 | 1,507 to 1,510 (tanh rand 1,342) | (not measured; bench 15-17x ≈ 1,300-1,500) |
| Profile | 55% `exp_reduced` (the double-double exp kernel), 15% final rounding | | 71% Newton loop, 20% final rounding | | |
| Ref | 86 | 82 | 148 | 128-141 | 87-135 |
| Proposed | Fast tier in double with a 2^-66 bound, reusing the §3.2 kernels (`expm1` = the exp kernel with the `−1` folded into the table step; `log1p` = the log kernel on an exact `1 + x` split). cbrt: a bit-trick seed, two division-free polynomial or Newton steps in double, one double-double correction | | | from the fast `expm1`/`exp` (one shared reduction for cosh and sinh) | from the fast `log1p` |
| Estimated after | 80-100 | 80-100 | 45-70 | 90-150 | 90-140 |

### 3.5 f32 functions: double evaluation is enough, but most rows run the double-double path

| | sinf | asinf, acosf, atanf, atan2f | expm1f | hyperbolic f | erff, lgammaf, tgammaf, Bessel f |
|---|---|---|---|---|---|
| Path | `fast32` double kernel, rounding test | `Real for f32` → `trig::*::<f32>` (double-double, no f32 kernel) | `fast32` | `hyper::*::<f32>` (double-double) | `special::*::<f32>` (384-bit mp, the same cost as double) |
| Fast share | 100% / 100% | none | 100% bench / 99.99% random (12.6% of random operands are handled before the kernel) | none | none |
| Cost | 142 | 1,309 to 1,556 | 130 / 222 | about 1,500 (bench 10-20x) | erff 228,016 / 81,583, lgammaf 594,203 / 425,828 |
| Ref | 35 | 53-95 | 74 | 76-84 | 145-280 |
| Cause | The kernel works but is heavy (140; the reference's is about 35) | no f32 kernel | | no f32 kernel | the f64 384-bit design reused unchanged |
| Proposed | One double polynomial with a ~2^-45 bound and a rounding test, failing about 2^-20 of calls. This is the standard binary32 design (published correctly rounded binary32 work reports reference-class speed: not verified here) | same | | same | double-only piecewise polynomial plus rounding test, falling back to the f64 fast tier, then mp |
| Estimated after | 35-45 | 35-60 | 50-70 | 40-70 | 50-120 (lgammaf near its zeros: higher) |

### 3.6 Special functions, f64 (question 3: yes, every call is multiprecision)

| | erf / erfc | lgamma / tgamma | j0, j1, y0, y1, jn, yn |
|---|---|---|---|
| Path | Always `erf_mp`: the 384-bit Taylor series (`term·2x²/(2n+1)`, about 80 terms at x ≈ 1, more at larger x), then `mp::exp(−x²)`, then three mp products. `erfc` at a ≥ 10: continued fraction of depth `40 + 60000/a²` | Stirling's series in 384 bits with argument shifting (products of 384-bit factors) and the `mp::ln_atanh` series. Reflection (`sin πx` in mp) for x < 0 | Power series in 384 bits below 100 (sized for 150 bits of cancellation), Hankel above; Ziv second step in 768 bits (`mpw`) for Bessel only |
| Fast share | none (100% mp) | none | none |
| Cost | erf 228,029 / 81,596; erfc 228,748 / 82,314 | lgamma 592,426 / 424,064; tgamma 631,999 / 463,892 | j0 149,669 / 303,982 |
| Profile (erf) | 48% `__udivti3` (the u128/u64 division in `Mp::div_u64`, 7 per term), 18% `pack`, 14% `erf_mp`, 12% `Mp::add`, 4% `Mp::mul` | | |
| Primitive costs (384-bit) | mul ≈ add + 80; `div_u64` ≈ 350; `div` ≈ 8,500; `sqrt` ≈ 11,400; `mp::exp` ≈ 40,000; `ln_atanh` ≈ 84,000; `mp::atan` ≈ 205,000 | | |
| Why every call is mp | D-10.15 chose one 384-bit evaluator for all special functions so that a single error analysis covers the worst cancellation (Bessel, lgamma near its zeros), with no separate tier for the easy majority of operands | | |
| Ref | 159 | 140-280 | 145-450 |
| Proposed | Piecewise minimax polynomials (project Remez tool, D-10.3 owner addition) in double with a double-double head: erf on about 8 intervals of [0, 6], erfc as `e^(−x²)·P(1/x)` beyond. ~2^-66 bound, rounding test, fallback to the existing mp path | lgamma: Stirling in double-double above 8, a shifted polynomial on [1, 3), and dedicated expansions around the two positive zeros and the first negative ones (the cancellation regions); mp fallback. tgamma = `exp(lgamma)` with a double-double exp, plus an exact product for small integers | Polynomials around each of the first N zeros (tabulated zeros, the oracle), Hankel's asymptotic form with a double-double phase above; mp fallback near any zero beyond the table |
| Estimated after | 150-220 (about 1.0-1.4x) | 250-500 (1.5-2.5x; near the zeros, more fallbacks) | 300-800 (2-4x; zeros dominate) |

### 3.7 long double (x87) and `_Float128`: every call is 384-bit

| | expl, exp2l, exp10l, expm1l | logl, log2l, log10l, log1pl | sinl, cosl, tanl | atanl, asinl, acosl, atan2l | hyperbolic l | special l |
|---|---|---|---|---|---|---|
| Path | `mp::exp` (Taylor series of `r/2^12`, squared twelve times) | `mp::ln_atanh`; **log10l also recomputes `ln 10` by the full series on every call** (`mp::ln10()`), so two ln series plus an mp division | 384-bit reduction and `sin_cos_small` | `mp::atan`: eight halvings, each a 384-bit sqrt and division, then about 24 series terms | from `mp::exp` and `ln1p` | §3.6 |
| Fast share | none | none | none | none | none | none |
| Cost | expl 41,092 / 34,336 | logl 88,955; log10l 195,076 | sinl 99,716 / 76,456 | atanl 206,407 / 119,557 | (bench 400-1,500x) | (bench 60-1,300x) |
| Profile | expl: 30% `__udivti3`, 28% `pack`, 18% mul, 12% add | | | atanl: 32% mul, 32% `pack`, 20% add, 8% `__udivti3` | | |
| Ref | 83 (x87 `f2xm1`) | 400-1,500 | 120-130 | 21 (one `fpatan`) | 130-480 | 85-580 |
| Proposed | A fast tier in double-double or triple-double (about 106 to 150 bits against x87's 64), table driven, with a bound of about 2^-100: the rounding test then fails about 2^-35 of the time, essentially never. Fallback to mp. `_Float128` needs about 2^-140, so triple-double or a 192-bit integer kernel, then mp | | | | | |
| Estimated after | 200-300 | 250-350 | 250-350 | 300-450 | 300-500 | 3-10x the f64 fast tier |

For x87, an instructions ratio is the wrong yardstick: the reference uses microcoded x87 transcendental instructions
(`fpatan` is one instruction counting about 21 and taking about 48 ns). Compare nanoseconds or cycles on x87 rows.
Correctly rounded x87 results cannot come from those instructions, because their error is not bounded below one ulp
of 64 bits. So no correctly rounded design reaches 1.05x **instructions** on atanl or expl. On time, 1-3x is
plausible.

## 4. The 1.05x design (question 4)

**Three-step Ziv chain per function, every step proven:**

1. **Fast step (new for most families).** The kernel computes `y = yh + yl` with a proven relative bound `ε`.
   * f32: evaluate in plain double, `ε ≈ 2^-45`.
   * f64: a double polynomial whose leading terms are in double-double, `ε ≈ 2^-66`.
   * x87: double-double or triple-double, `ε ≈ 2^-100`.
   * f128: triple-double, `ε ≈ 2^-140`.

   Then a rounding test that costs the same in every mode: for round-to-nearest, `|½ulp − |yl|| > ε·|y|`; for a
   directed mode, `|yl| > ε·|y|` together with its sign.
2. **Accurate step**: the existing double-double path (f32/f64) or a tighter mp. Its result must be proven or
   oracle-checked. This is what runs today.
3. **Last step**: 384 or 768-bit mp (exists).

**Fallback rate the bounds allow:** a random operand fails the round-to-nearest test with probability about
`2·ε·2^p`, where `p` is the precision. The measurement agrees: exp's bound of 2^-70 predicts 2^-16, and 1.2e-5 was
measured. sin's 0.64% implies a 2^-61 bound.

| Format | Bound | Expected fallback rate | Notes |
|---|---|---|---|
| f32 | ε = 2^-45 | ≈ 2^-20 | |
| f64 | ε = 2^-66 | ≈ 2^-12 | A 1,500-instruction fallback adds about 0.4 instructions per call: negligible |
| x87 | ε = 2^-100 | ≈ 2^-35 | |

**Fast-step cost from first principles** (an instruction is roughly one flop or integer op, with the FMA variant):

| Function class | Instructions |
|---|---|
| f32 reduction + 32/64-entry table + degree 4-6 polynomial + test | 30-50 |
| f64 exp/log class: reduction (3-5) + table (2) + degree 5-7 Estrin polynomial (12-16) + one double-double add or product (2-6 with FMA, 15-25 Dekker) + test (6-8) + reconstruction (4) | 45-70 kernel, 70-100 with entry and envelope |
| f64 trig with Cody-Waite (\|x\| < 2^20) | about 90-110 |
| f64 atan family | about 80-120 |

All of these are within 1.0-1.3x of the reference rows. They reach 1.05x only with FMA (a correctly rounded result
is path-independent, so the FMA and non-FMA variants are bit-identical **by construction**, which satisfies the
"never a different result" hardware rule for free).

**Families where the design does not reach about 1.05x:**

| Family | Why |
|---|---|
| lgamma and tgamma | Near the zeros of lgamma and for negative x, the fast tier needs per-region expansions and its fallback rate rises. Estimate 1.5-2.5x |
| Bessel (j0 … yn) | Infinitely many zeros. Relative accuracy near each needs per-zero expansions. Estimate 2-4x; the reference itself is far from correctly rounded there (791 ulp observed) |
| x87 long double in instructions | The reference's microcoded `fpatan` / `f2xm1` count about 1 instruction each. Measure in time instead |
| `_Float128` | The reference is itself a soft-float implementation. Ratios of 4-34x now; a triple-double fast tier should reach 1.5-3x |

**Cheap wins that do not need the redesign** (worth doing first; they cut every mp row, including complex and f128):

* `Mp::div_u64` divides through `__udivti3` (u128/u64 software division, 48% of erf). All divisors are below 2^32,
  so split each limb into two 32-bit halves with native `u64` division, or multiply by a precomputed reciprocal.
* `pack` re-normalises after every operation (18-32% of samples): skip it when the top limb is already normalised.
* `mp::ln10()` recomputes `ln 10` per call: store it as a table constant, as `MP_LN2` is. This halves log10l
  (195k → about 100k).
* `elem::finish` rounds every double-double result through the 256-bit `arith::sum` and `float::round` (9-20% of
  the double-double paths, and the whole 550-instruction gap of sin in upward mode): round a normalised
  double-double into double directly (about 10 instructions).

Expected effect: about 2-3x on every mp row and 10-20% on the double-double rows. Not near parity: that needs the
fast tiers.

## 5. RULING NEEDED (question 5)

**R1. The accuracy contract per family.** The written decisions (D-10.13/15/19/20) claim correct rounding in every
mode for every function. The spec requires only the reference's ulp bounds. Options:

| Option | Contract | Cost (estimated, instructions) | Accuracy |
|---|---|---|---|
| **A** | Correct rounding everywhere (now), with the fast tiers of §4 | f32 and f64 elementary: 1.0-1.3x (1.05x with FMA dispatch). erf: 1.0-1.4x. lgamma/tgamma: 1.5-2.5x. Bessel: 2-4x. x87: 3-15x in instructions, 1-3x in time | Best; the reference differs on 0.01-54% of inputs |
| **B** | Correct rounding for f32 and f64 elementary functions and erf/erfc. For lgamma, tgamma, Bessel and long double: a documented ulp bound no looser than the reference's measured bound (≤ 4 ulp for lgamma/tgamma; Bessel's documented bounds) | Those families at about 1.0-1.5x | Loses correct rounding where the reference does not have it either |
| **C** | Faithful everywhere (≤ 1 ulp, or the reference's bounds) | About 1.0x everywhere; no Ziv machinery | Loses the D-10.13 claim; results still differ from the reference's bits on many inputs |

**Recommend A for f32/f64 elementary functions and erf/erfc.** It is feasible near parity, and correct rounding
makes FMA dispatch bit-identical for free. For lgamma, tgamma, Bessel and x87 long double, recommend **A with a
recorded residual**: keep correct rounding, build the fast tiers, and record the remaining ratio in the saturation
register with the first-principles bound. Switch a family to B only if, after its fast tier, it stays above about 2x
in time. For x87 rows, the owner should confirm that time, not instructions, is the parity yardstick (R2).

**R2. The yardstick for x87 rows.** Instruction counts compare against one microcoded instruction. Recommend
measuring x87 parity in cycles or ns on a booked timing host.

## 6. Briefs

One per redesign group, in `handoff/libm-slow-path-cause/briefs/`:

| Brief | Group |
|---|---|
| `LIBM-MP-CORE-SPEED.md` | The cheap wins of §4, all mp and double-double rows |
| `LIBM-F64-FAST-KERNELS.md` | The §3.1/3.2 kernels: double-only evaluation, FMA dispatch, cheap directed test |
| `LIBM-F64-INVTRIG-HYPOT-TIER.md` | §3.3 (continues the unverified PERF-LIBM-INVTRIG work; replaces the held INVTRIG-2) |
| `LIBM-F64-EXPM1-HYPER-CBRT-TIER.md` | §3.4 |
| `LIBM-F32-DOUBLE-KERNELS.md` | §3.5 |
| `LIBM-SPECIAL-FAST-TIER.md` | §3.6 (replaces the held PERF-LIBM-SPECIAL) |
| `LIBM-WIDE-DD-TIER.md` | §3.7 |

## 7. What was not verified

* No glibc or libm source was read.
* The "estimated after" costs are first-principles estimates, not measurements.
* The agreement probe assumes Liella's results are correctly rounded. That is backed by the D-10.3 oracle tests,
  not re-checked here.
* asinh, acosh and atanh costs come from the bench ratios, not the harness.
* The published-results remark in §3.5 is not verified here.
