# LIBM-RETHINK: libm from first principles for modern hardware (WIP at the context checkpoint)

Story LIBM-RETHINK (RETHINK under the RUT RULE), branch `task/libm-rethink`, base `next` dd2c5b88. **State: steps 1-4 of
the brief done (inputs, black-box measurement, designs, prototype measured). Steps 5 (read the current implementation
and compare) and 6 (decision rows D-NEW, code briefs) are NOT done: the run stopped at the ~150k-context checkpoint
(thread_common BUDGET rule).** Section 6 below is a provisional recommendation from steps 1-4 only; step 5 may change it.
Hosts named here: keke = Zen 1 build host (EPYC 7551), the role `keke:~/w/r5`.

## 1. Inputs (step 1)

**Contract.** D-10.3, D-10.13, D-10.14, D-10.17, D-10.19 and the differences preamble (owner rulings #2760/#2761,
2026-11-05): every function correctly rounded (CR) in all four modes and all formats; inexact exactly when inexact,
underflow after rounding; errno per §10.0. A family whose CR fast tier stays above about 2x the reference in time may
move to a ulp bound only by an owner ruling (x87 judged in cycles). Nothing here proposes a contract change.

**Sweep (TIMING-SWEEP-LIBM-1107, time ratio Liella/glibc, medians).** Ivy Bridge (AVX, no FMA): f32 2.27, f64 3.62,
long double 114.7, binary128 10.37, complex 274; N100: 2.66 / 3.52 / 83.6 / 7.45 / 239. Representative rows (IVB
time): exp 2.90 (11-05: 1.86), log 5.18, pow 3.35, sin 2.22, expf 2.11, logf 2.60, expl 4.52, logl 10.97, sinl 112.7,
atanl 457, sqrtl 3.58, expf128 6.78, sinf128 19.9, atanf128 27.1, cexpf 621, cexp 705, clog 215, csqrt 163.

**What was tried (do not redo).** Saturation register §16 (f64 exp/log/pow gap breakdown; candidates 1, 2, 6, 7 done:
Fast2Sum assembly, 3-part Cody-Waite exp reduction, pow r³/3, log2/log10 tables; 8 done as D-10.25/D-10.50 load-time FMA
builds); §20 (384-bit mp core: div_u64, pack, constants; -15..-75% per mp row, still 10^4-10^5 instructions); §21 (x87
exp/log double-double tier: 28-66x fewer instructions than mp, still 4-11x glibc cycles; expl's 1039 split: kernel 54%,
generic `float::round::<F80>` 16%, rounding decision 15%, wrapper 7%); §22-24 (f64 invtrig/hypot, expm1/log1p/
hyperbolic/cbrt fast tiers); f32 double-kernel stories (tgammaf, sinhf, tanhf now faster than glibc; j0f/j1f parity).
Every one of these tuned a tier of the existing structure; none changed the structure (the RUT).

**Hardware.** Build hosts: Zen 1 (EPYC 7551: AVX2, FMA, BMI2, ADX), Skylake-SP (Platinum 8151: AVX-512F/DQ/VL, no
IFMA), Haswell-EP (E5-2697 v3: FMA, AVX2, BMI2, no ADX), Broadwell-EP (E5-2699 v4: + ADX). Timing: Ivy Bridge (E3-1265L
v2: AVX, no FMA/AVX2/BMI2/ADX), N100 (Gracemont: FMA, AVX2, BMI2, ADX, no AVX-512). No host has AVX-512 IFMA, so a
52-bit-limb IFMA design cannot be tested here; designs below need only SSE2, optionally AVX (VEX), FMA, BMI2/ADX.

## 2. Where the time goes, black-box on Liella (step 2)

Job `libm-rethink-percall` (keke, `validation/wl_percall.sh --spec proto/step2_percall.c`; 64 operands in the cbench
ranges, long double and binary128 operands with full significands), instructions:u per call, deterministic (sd 0),
including about 4 of the caller's loop:

| case | glibc | Liella | ratio | | case | glibc | Liella | ratio |
|---|---:|---:|---:|---|---|---:|---:|---:|
| expf | 42.0 | 73.0 | 1.74 | | expl | 87.0 | 1054.1 | 12.1 |
| logf | 46.0 | 80.0 | 1.74 | | logl | 52.1 | 1161.1 | 22.3 |
| sinf | 45.1 | 93.0 | 2.06 | | sinl | 1119.0 | 80261.7 | 71.7 |
| powf | 78.0 | 243.0 | 3.11 | | atanl | 26.0 | 160922.7 | 6189 |
| exp | 70.0 | 135.0 | 1.93 | | cbrtl | 161.0 | 36718.8 | 228 |
| exp (upward) | 70.0 | 368.4 | 5.26 | | expf128 | 6672.6 | 33152.5 | 4.97 |
| log | 66.0 | 189.6 | 2.87 | | logf128 | 7779.8 | 70647.5 | 9.08 |
| sin | 104.6 | 152.0 | 1.45 | | sinf128 | 7161.8 | 80450.3 | 11.2 |
| atan | 81.2 | 145.7 | 1.79 | | atanf128 | 5908.7 | 161470.1 | 27.3 |
| pow | 143.0 | 319.4 | 2.23 | | cexpf | 162.2 | 116553.8 | 719 |
| sqrtl | 13.0 | 13.0 | 1.00 | | cexp | 239.0 | 116568.8 | 488 |
| | | | | | clog / csqrt | 332.5 / 143.9 | 72366.6 / 40966.9 | 218 / 285 |

(glibc's atanl is one microcoded `fpatan`: 26 instructions but over 100 cycles; x87 rows are judged in cycles.)

Job `libm-rethink-trace` (`validation/wl_trace.sh`, call 11, Liella only; rc 1 because the `sqrtl` case had no library
call: gcc -O2 inlines `sqrtl` as `fsqrt`, a probe artifact, see below). Which tier runs on EVERY call:

* `sinl` 75 496 instructions, `atanl` 163 886, `expf128` 34 344, `sinf128` 75 569, `cexpf` 134 484, `cexp` 134 506, `csqrt`
  40 483: 95-99% in `mp::Mp::{mul, add, div_u64}` and `mp::pack`, i.e. **the 384-bit route runs on every call; there is no
  fast tier** for long double outside exp/log, for binary128, or for complex (and `float` complex costs exactly what
  `double` complex does: it runs the same 384-bit route).
* `expl` 1039: the double-double tier of §21 (no mp call on the common path).
* `exp` 125, `expf` 63: the fast path; each call makes an out-of-line `liella_machine_mxcsr_get` call. `exp` in upward
  rounding 363: `fast64::exp` 144, the switched envelope `elem_switched` 52, `fast64::settle` 47, `fpu::raise` 45 (flags
  raised in software), `exp_log::exp` 44, MXCSR get/set 18: about a third of a directed-mode call is envelope.
* f64/f32 rows on IVB are slower in time than in instructions (log 2.87x instructions, 5.18x time; logf 1.74 / 2.60;
  sqrtl 1.00 / 3.58): something on those paths is slow per instruction (MXCSR access, division or a dependency chain);
  the trace cannot tell, timing per variant must (step 5/6 item).

## 3. Designs (step 3)

### 3.0 Cross-cutting principles (apply to every class)

* **P1 Ziv tiers sized by expected cost** [Ziv, ACM TOMS 17(3) 1991; Daramy-Loirat, de Dinechin, Defour, Gallet, Gast,
  Lauter, Muller, CRlibm report 2006 (quick/accurate phases); de Dinechin, Lauter, Muller, RAIRO-ITA 41 2007; Muller et
  al., Handbook of Floating-Point Arithmetic, 2nd ed., 2018]. Average cost = C_quick + p·C_next with the
  fallback probability p ≈ 2^(prec+1)·ε for a quick phase of relative bound ε. A quick phase need not be "almost
  exact": at ε = 2^-61.5 (plain double products) p ≈ 0.5%, and with the next tier at ~150 instructions the fallback
  adds under one instruction per call. The current design makes every fast tier near double-double accuracy (about
  2^-100, p ≈ 2^-45), i.e. it pays the accurate phase's cost on every call. Prototype (section 4) confirms the model.
* **P2 Envelope off the hot path.** One inline `stmxcsr`, one mask/compare (round-to-nearest, all exceptions masked;
  FZ/DAZ ignored where every hot value is normal); no MXCSR write, no software flag raising: the arithmetic raises
  inexact itself, which is exactly right for transcendental results (exact cases are filtered first). Directed modes,
  unmasked traps, specials and errno go to cold out-of-line code. For f32, and for the integer designs below, the hot
  path is mode-independent and needs no switch at all.
* **P3 ISA variants through the existing load-time dispatch (D-10.50).** SSE2 baseline (2-operand forms cost about 15
  extra register moves per exp, measured below), AVX/VEX encoding without FMA (Sandy/Ivy Bridge: no moves), FMA (exact
  products in 2 instructions: tighter bounds, lower p). Results bit-identical across variants (each is CR).
* **P4 Tables generated, small.** <= 2-4 KiB per family (L1-resident beside real code), from `cargo xtask tables`
  (oracle, recorded derivation); Tang tables [Tang, TOMS 15(2) 1989; TOMS 16(4) 1990]; Gal's accurate tables where they
  save the low part [Gal & Bachelis, TOMS 17(1) 1991].
* **P5 Integer fixed point for the wide formats.** x87's 64-bit and binary128's 113-bit significands map onto
  64-bit limbs: u64×u64→u128 (`mul`/`mulx`, 3-4 cycles, one per cycle on every target) gives exact 128-bit products, the
  rounding test is an integer mask test, the four rounding modes are four integer roundings (mode-independent, no
  MXCSR/x87 control-word traffic), and the x87 unit is touched only to load the argument and the result. Fixed-point
  and multi-limb evaluation of elementary functions: [Brent & Zimmermann, Modern Computer Arithmetic, 2010]; the
  resume should add a published fixed-point CR design to the decision row if one is found (literature search item).

### 3.1 binary32

* **A. One evaluation in double, mode-independent, proven exhaustively.** Polynomial/table evaluation in double with
  bound ~2^-45..-50, then one `cvtsd2ss` in the caller's mode; no MXCSR read on the hot path (an evaluation in a
  directed mode keeps a bound twice as large, still far below 2^-24·2^-20). Correctness: the 2^32 inputs × 4 modes
  compared with the CR reference (the current implementation or the oracle) prove it; the few inputs whose double
  result lies within the bound of a float boundary go to a small exception table or a check. ISA: SSE2. Est. expf
  25-30 instructions (glibc 42), logf 25-30 (46), sinf 30-35 (45). Build cost: small (the tooling exists; exhaustive
  run ~1 min on 60 vCPU).
* **B. Same evaluation plus an online rounding test** (|result − nearest float boundary| > bound, else fallback): no
  exception table, a proof by bound, exhaustive run as a check only. +3-5 instructions; preferred where a mode-dependent
  exception list would be long (pow, the special functions).

### 3.2 binary64

* **A. Cheap quick phase + rounding test + the existing CR path as the next tier (prototyped).** Tang reduction,
  Cody-Waite, short polynomial in double, only the product that dominates the bound made exact when FMA is free;
  rounding test on a proven bound; fallback = today's fast64/dd path (itself falling back to mp). Measured for exp:
  91.8 (SSE2) / 77.0 (FMA) instructions vs 135 now and 70 for glibc (section 4). Extends to exp2/exp10/expm1 (same
  table), log (Tang log table, quick bound ~2^-62), sin/cos (Cody-Waite + Payne-Hanek kept, quick polynomial), atan,
  pow (quick log in "double + low part" ~2^-68 then quick exp; its fallback is today's pow).
* **B. Single-phase double-double (today's structure, reference point)**: ~2^-100 bound, p ≈ 2^-45, 135 instructions
  for exp after the §16 tuning; its floor without FMA is ~90 (§16 "first-principles minimum").
* **C. Larger or two-level tables to shrink |r|** (N = 256-1024) so the plain product's error falls (p 0.25%-0.06%):
  rejected as the main line: p already costs < 1 instruction, and 8-16 KiB tables hurt real workloads (P4).
* **D. Integer fixed-point quick phase** (u64/u128): one multiplier port vs two FP ports; no gain for 53 bits. Not
  pursued for f64 except as P5's building block.

### 3.3 long double (x87, 64-bit significand)

* **A. Integer fixed-point quick phase (P5) at ~2^-85..-90, integer rounding test, fallback = today's tiers.**
  Unpack the 80-bit operand to (u64, exponent); exp: k = round(x·128/ln2) by one 64×64 product, r = x − k·ln2/128 in
  128-bit fixed point (two products), 2^(j/128) as a 128-bit table entry (2 KiB), polynomial degree ~7 in 64-bit fixed
  point for the high terms and 128 bits for the last two, one truncated 128×128 product, round the integer in the
  caller's mode (4 cases), pack, `fld`. Est. expl 95-120 instructions and ~40-60 cycles (glibc 87 instructions but
  about 200 cycles on IVB, §21: f2xm1/fyl2x are microcoded). sinl/cosl/tanl: Payne-Hanek with a 2/π bit table over
  the x87 exponent range (~2 KiB) in the same limbs, Tang-style sin/cos table to |r| < 2^-6, est. 150-250 (glibc
  1119 instructions). atanl/asinl/acosl: atan table (j/64) + short odd polynomial, asin/acos via atan2-form, est. 150-250.
  p ≈ 2^65·2^-88 = 2^-23. ISA: SSE2 + `mul`; `mulx`/`adx` variants optional. Cost to build: L per family group
  (exp/log, trig, inverse trig, hyperbolic), the integer kernels shared with binary128.
* **B. "Double-double-lite" in SSE2/FMA at ~2^-80** (only the leading 2-3 terms in double-double, the rest in double),
  rounding the double-double straight to the x87 bits instead of the generic rounder (§21 found 31% of expl in the
  rounder and decision). Est. expl 250-400; good as a refinement of the existing exp/log tier, weaker than A elsewhere.
* **C. x87 transcendental instructions as the quick phase (fsin, fpatan, fyl2x, f2xm1)**: rejected. Their results are
  rounded to 64 bits (at best faithful), so a rounding test to 64 bits fails on about every call, and they are
  microcoded (50-150+ cycles).

### 3.4 binary128 (113-bit significand)

* **A. Integer fixed point with three 64-bit limbs (192-bit) at ~2^-135..-150**, p ≈ 2^-20 or less, the same kernels
  as 3.3 A with one more limb; `mulx`+`adcx/adox` where ADX exists (Broadwell+, Zen, N100) and `mul`/`adc` otherwise
  (Haswell, Ivy Bridge). Est. expf128 300-600 instructions (glibc 6673), sinf128 500-900 (7162), atanf128 500-900
  (5909): faster than glibc's software binary128. Fallback = today's 384-bit route.
* **B. Triple-double in FP registers** [Lauter, "Basic building blocks for a triple-double intermediate format",
  research report, 2005]: ~2^-150, but each triple-double product is ~3 exact products (Dekker without FMA: ~50 flops); est.
  800-1500. Weaker than A on every target; considered and set aside.

### 3.5 complex

* **A. Compose the fast real quick phases with a per-part rounding test** (P1): cexp = e^x·(cos y + i sin y) from one
  quick exp and one shared sin/cos reduction, each part a product with a bound ε_exp + ε_trig + 2^-p, rounding test per
  part, fallback to today's 384-bit route (which stays the proof of last resort). float complex in double (bounds
  ~2^-48, p ≈ 2^-23). Regimes with cancellation use the formulas the current code already proves (clog near |z| = 1 as
  ½·log1p((x−1)(x+1) + y²) with exact products; csqrt from an accurate hypot; inverse functions by Hull, Fairgrieve
  and Tang [ACM TOMS 20(2) 1994; TOMS 23(3) 1997]; branch cuts per Kahan [1987]). Est. cexp 180-250 (glibc 239),
  cexpf 60-90 (162), csqrt 60-90 (144), clog 150-250 (333).
* **B. Complex kernels in integer fixed point** (shared limbs for both parts). Higher build cost, no clear gain over A
  for f32/f64 where A's parts are already double-based. Kept as the route for long double complex if 3.3 A lands.

## 4. Prototype: binary64 exp, design 3.2 A (step 4)

Files: `handoff/libm-rethink/proto/` (`proto_exp.c` the kernel and its sources, `accuracy.c`, `proto_percall.c`,
`run_accuracy.sh`). C at -O2 (the product would be Rust; LLVM's code should be close). Not wired into the product.

**Method.** x = (128m + j)·ln2/128 + r, k = round(x·128/ln2) by the 1.5·2^52 shift; r = (x − k·L1) − k·L2 with L1 of
36 bits (k·L1 exact for |k| < 2^17, x − k·L1 exact by Sterbenz), the low part of the subtraction kept; e^r − 1 − r by
Taylor degree 6; result T_j·(1 + r + q) with T_j = 2^(j/128) as a double pair (from binary128 exp2), assembled by two
Fast2Sums; rounding test `zh + (zl − b) == zh + (zl + b)` with b = ε·zh. Hot domain 2^-54 <= |x| < 512, round to
nearest, exceptions masked; all else falls back to the library's exp (a product extends the domain to the overflow and
subnormal thresholds with a scaled final multiply).

**Bound** (relative to zh; |r| <= 2^-8.528): rounding of T·r_h 2^-61.524 (variant L only; exact in D by Dekker's
product with Veltkamp's split, in F by one FMA), evaluation of the low sum ~2^-68.2, polynomial evaluation 2^-69.4,
neglected r_l·r_h 2^-70.5, truncation 2^-72, reduction 2^-76.5. Total L 2^-61.50 (test uses 2^-61.19), D/F 2^-67.4
(test uses 2^-67); the slack covers the test's own roundings (< 2^-104·zh).

**Accuracy** (job `libm-rethink-proto-accuracy`, static against Liella's products of dd2c5b88): 10^8 operands in each
of three distributions (uniform [-10, 10]; uniform bit patterns over the hot domain; uniform [-745.2, 709.8]) × 3
variants, the 432 hard operands of `fast64.rs` (e^(k·2^-53) next to midpoints), 900 000 directed-mode calls: **0
mismatches with the CR exp. Fallback rate: L 0.49-0.51%, D/F 0.0086-0.0091%** (the model predicts 2^-7.5..-8.5 and
2^-13.4). Measured max relative error of the quick result against binary128 exp over 3·10^5 operands: L 2^-61.56
(bound 2^-61.50), D 2^-68.69, F 2^-68.84 (bound 2^-67.4). Directed modes: every call fell back (guard correct).

**Counts** (job `libm-rethink-proto-count`, keke Zen 1, instructions:u per call, 4096 operands in [-10, 10], the
fallback included; the "glibc" column of the prototype rows links glibc's non-CR exp as fallback and differs in
linking (dynamic, PIE), so only the Liella-linked column is the prototype's figure):

| row | Liella-linked | vs glibc exp (70.0) |
|---|---:|---:|
| exp today | 135.0 | 1.93 |
| proto L (SSE2, plain products, p 0.5%) | 91.8 | 1.31 |
| proto D (SSE2, Dekker product, p 0.009%) | 109.0 | 1.56 |
| proto F (FMA, VEX, p 0.009%) | 77.0 | 1.10 |

L carries about 15 more instructions than F although its arithmetic is one instruction shorter: the 2-operand SSE2
forms need register copies, which VEX removes (and which Ivy Bridge's move elimination makes nearly free in time); an
AVX-without-FMA build of L (Ivy Bridge, Sandy Bridge) should land near 77. D shows that buying p down with exact
products costs more than the fallbacks it saves. **Remaining gap to glibc's count** (L −15 by VEX, then ~7): the
outer-loop fallback guard on the full domain, the two-sided test (a one-sided test `zh == zh + zl·e` saves 3-4
[CRlibm report, rounding test theorem]), a minimax degree-5 polynomial instead of Taylor 6 (−2), and Rust's codegen.
**Timing pending**: no booking was made in this run (checkpoint); the instruction mix changes (fewer double-double
steps, no MXCSR call), so the verdict needs an Ivy Bridge and an N100 timing (developer-flow exception list).

## 5. Comparison with the current implementation (step 5): NOT DONE

To do on resume: read `crates/math/src/fast64.rs` (exp path, `settle`, `exp_log::reduce`), `wide_dd.rs`,
`mathret` envelope, the complex entry (`complex::n::cexp`), by `grep -n` + ranges only (files > 40 KB); answer: what
each call does, which parts design A keeps (the dd path as tier 2, mp as tier 3, the tables tooling, Payne-Hanek, the
exactness detectors of complex), why the f64/f32 IVB time exceeds the instruction ratio.

## 6. Provisional recommendation (before step 5) and plan sketch

| class | recommended | expected end state vs glibc (est.) | first step |
|---|---|---|---|
| binary64 | 3.2 A: quick phase + test, today's fast path as tier 2 | exp/log/sin ~1.0-1.3x instructions | exp family quick phase (kernel + wiring briefs) |
| binary32 | 3.1 A (exhaustive) / B for pow and special | 0.6-0.9x | expf/logf/sinf quick, exhaustive proof job |
| long double | 3.3 A integer fixed point; keep §21 tier as tier 2 | faster than glibc in cycles (x87 microcode) | expl/logl integer kernel, then sinl/atanl |
| binary128 | 3.4 A three-limb fixed point | 0.1-0.2x (glibc is software) | shared limb kernels with long double |
| complex | 3.5 A compose real quick phases | 0.5-1.1x | cexp/cexpf, then csqrt/clog |

Order by value: complex and long double/binary128 remove 100-700x outliers (each row moves by two orders of
magnitude), binary64 exp/log/pow/sin are the most called. No ulp-bound ruling is needed by this plan: every class keeps
CR with a fast tier. Decision rows (D-NEW) and code briefs are step 6 (not written).
