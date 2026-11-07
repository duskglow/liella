# LIBM-RETHINK: libm from first principles for modern hardware (WIP at the context checkpoint)

Story LIBM-RETHINK (RETHINK under the RUT RULE), branch `task/libm-rethink`, base `next` dd2c5b88. **State: steps 1-4 of
the brief done by LIBM-RETHINK; LIBM-RETHINK-2 timed the prototype on Ivy Bridge and the N100 (4.1), did step 5 (the
comparison, section 5) and step 6 (section 6, three D-NEW rows, four code briefs). The plan is complete.**
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
### 4.1 Timing (LIBM-RETHINK-2, booked hosts)

Harness `proto/proto_time.c` (built on the build host by `proto/build_time.sh`: `t_liella` static against Liella,
`t_glibc` against the system glibc dynamically with `-z now`; run by `proto/run_time.sh`, 5 alternations of the two
binaries × 7 interleaved rounds of 10^6 calls; medians; spread under 1%). Operands as the count (4096 uniform in
[-10, 10]; log: 10·|x| + 2^-10). tput = independent calls (the cbench shape), lat = each argument depends on the
previous result. Every call goes through one function pointer (`ident` = that cost). New variants LE (SSE2), LEV
(LE in VEX) and E (FMA): the same arithmetic restructured for latency (k from the bits of the shifted sum, Estrin's
scheme, the result zh·2^m beside the test; `PROTO_BODY_E`); their bound is not re-derived for Estrin's order (CHECK:
0 mismatches with Liella's CR exp over the operands; fallback 0.659% L/LE, 0.024% D/F/E). Jobs `kotori:libm-rethink-
2-time-ivb` (Ivy Bridge E3-1265L v2, idle) and `nozomi:libm-rethink-2-time-n100` (N100, idle), ns per call:

| row | IVB tput | IVB lat | N100 tput | N100 lat |
|---|---:|---:|---:|---:|
| glibc exp | 13.79 | 24.35 | 7.98 | 23.40 |
| Liella exp today | 40.79 (2.96x) | 53.67 (2.20x) | 27.04 (3.39x) | 45.53 (1.95x) |
| proto L (SSE2) / LV (VEX) | 20.30 / 20.35 | 42.2 / 42.0 | 16.15 | 36.03 |
| proto D (Dekker) | 22.98 | 42.6 | - | - |
| proto F (FMA) | - | - | 16.80 | 40.61 |
| **proto LE (SSE2, latency form)** | **19.19 (1.39x)** | **33.58 (1.38x)** | 16.38 | 30.19 |
| **proto E (FMA, latency form)** | - | - | **16.03 (2.01x)** | **31.29 (1.34x)** |
| glibc log | 11.66 | 26.8 | 9.25 | 27.0 |
| Liella log today | 61.6 (5.28x) | 73.4 (2.74x) | 50.1 (5.42x) | 68.6 (2.54x) |
| ident (call + loop) | 3.62 | 3.62 | 5.02 | 2.62 |

Readings. (1) The quick phase halves exp's time on both classes (Ivy Bridge 40.8 -> 19.2 ns, N100 27.0 -> 16.0) and
cuts latency by 30-37%. (2) The Liella/glibc exp ratios here (2.96x Ivy Bridge, 3.39x N100) match the sweep's (2.90,
3.5): the harness measures what the sweep measures. (3) VEX encoding gains nothing in time on Ivy Bridge (move
elimination), Dekker's exact product loses (22.98 vs 20.30): confirmed. (4) The latency form wins 20-25% of latency
at equal throughput. (5) On the N100 the prototype is still 2.0x in throughput at 77 instructions against 70: that
core is bound by FP operations (about 33 per call in E against far fewer in a non-CR evaluation), so the kernel brief
targets the FP-operation count (the one-sided `near` test saves 3, a minimax degree 5 saves 2, dropping the separate
`1 + rh` product 1-2). (6) Liella's log overlaps consecutive calls far less than glibc's (tput/lat 0.84 Ivy Bridge,
0.73 N100; glibc 0.44 and 0.34): a serialising step on its path, to be found before any log design (5.4).
The N100 job was started at 03:50:24 while booking 105 still showed "settling" (active from 03:54:23); the host ran no
other job (load 0.00 at the start), so the numbers stand, but the order of steps was wrong (handoff).

## 5. Comparison with the current implementation (step 5, LIBM-RETHINK-2)

Read by `grep -n` and line ranges only: `boundary/src/posix/s10_math_gen.rs` (`exp`), `boundary/src/mathret.rs`
(`elem_fast`, `elem_fast_cold`, `elem_switched`), `boundary/src/hot/exp_log.rs`, `boundary/src/dispatch.rs` (the FMA
rule), `rt/src/fpu.rs` (`peek`), `math/src/fast64.rs` (`exp_kernel`, `reduce_ln2`, `near`, `finish`, `exp_near`),
`math/src/wide.rs` (`sin`), `math/src/wide_dd.rs` (module doc), `math/src/complex.rs` (module doc), `complex_body.rs`
(`cexp`).

### 5.1 binary64 exp per call today (round to nearest, in place)

1. `exp` -> `mret::elem_fast`: `fpu::peek` is an out-of-line call to the assembly `liella_machine_mxcsr_get` plus a
   compare with the default control word; `core::hint::black_box(x)` sends the argument through the stack;
   `hot::exp_nearest` tests `dispatch::fma()` (a load and a branch per call) and calls the FMA build
   (`fast64::fused::exp_nearest`, a `#[target_feature]` function, so a real call) or inlines the SSE2 build.
2. `exp_near`: domain 2^-60 <= |x| < 708; k = nearest(x·64/ln2); `reduce_ln2` + Fast2Sum (3-part Cody-Waite, r = rh +
   rl within 2^-106.5); `exp_kernel`: |rh| <= 0.006 (64-entry double-double table), the r³..r⁸ terms by Horner (6
   coefficients, a serial chain), two exact products (rh² and th·ph: Dekker's product in the SSE2 build, one FMA in
   the FMA build), a TwoSum and two Fast2Sums; `near` (one multiply, one add, one compare; c from RHO_EXP =
   1.125·2^-70); on a miss `settle` (the exact test), then the cold accurate path.
3. Cost: one call of the FMA build on Zen 1 is 125 instructions (entry and envelope 21 = 17%, the kernel about 100);
   the SSE2 build, which Ivy Bridge runs, is 189 per call in the loop (5.4). The tier is an accurate phase on every
   call (2^-70, exact products, a degree-8 series): design 3.2 B. Prototype 3.2 A keeps its proofs as tier 2.
4. Directed modes (`elem_switched`): MXCSR switched to the default, `Real::exp` -> `exp_log::exp` -> `fast64::exp`
   with `settle` in the caller's mode, the flags raised in software (`fpu::raise`), MXCSR restored. **Envelope
   finding:** an upward exp is 363 instructions, of which the envelope is 128 (35%: entry 13, `elem_switched` 52,
   MXCSR get/set 18, `fpu::raise` 45); about a third of every directed-mode call is MXCSR switching and software flag
   raising, whatever the function.

### 5.2 long double, _Float128 and complex per call today

* `wide::sin`, `cos`, `tan`, `atan`, ... (generic over F80 and F128): specials and the tiny-argument shortcut, then
  `sin_cos_of` in the 384-bit `mp` arithmetic and `approx` rounding. **No tier precedes mp** (sinl 80 262, atanl
  160 923 instructions per call).
* `wide_dd` (§21, D-10.19 option A): the double-double tier of exp/exp2/exp10/expm1 and the logarithms, x87 only. Its
  reduction is already 128-bit fixed point in integers; then a degree-11 double-double polynomial and a 64-entry
  table, bound 2^-103 (b = 100): an accurate phase on every call (expl 1039). `decide` refuses 113-bit formats, so
  every `_Float128` call takes mp.
* complex (`complex_body.rs`, generic over `Real`): every finite operand off the axes runs `b.trig()` (mp sin/cos),
  `exp_mp`, the products and `pair(…, mode)` in 384-bit arithmetic. **`float` complex costs what `double` complex
  costs** (cexpf 116 554, cexp 116 569 instructions): the generic route does not depend on the precision. On the axes
  (y = 0, x = 0) cexp already calls the real functions and their fast tiers.

### 5.3 What the new design keeps

| part | today | new design |
|---|---|---|
| fast64 kernels with `near` + `settle`, their proofs, both builds (D-10.25, D-10.50) | tier 1 | tier 2, unchanged, run on the quick phase's miss (0.66% L/LE, 0.024% F/E on the cbench operands) |
| `near` (one-sided test, one multiply and one add) | tier 1's test | also tier 1's test, c from the quick bound (the prototype's two-sided test costs 3 more FP operations) |
| 384-bit mp route | the only tier outside exp/log for long double, binary128, complex | the last tier everywhere (proof of last resort) |
| `elem_fast` (peek, in place, cold fallback) | kept | kept; its per-call costs (out-of-line MXCSR read, `black_box` spill, FMA flag, the `#[target_feature]` call) are measured items of the wiring brief |
| `wide_dd` exp/log | x87 tier 1 | tier 2; its fixed-point reduction is 3.3 A's building block |
| complex exactness detectors, special-value tables, Hull-Fairgrieve-Tang formulas | kept | kept; the new tier 1 composes the real kernels' (double-double, scale, bound) results |
| `cargo xtask tables` + oracle | kept | the 128-entry exp table and later tables come from it (P4) |

### 5.4 Why time exceeds instructions (open question 1): two measured causes

1. **Ivy Bridge runs the SSE2 builds, which the Zen 1 counts never saw.** Job `libm-rethink-2-percall-sse2` (build
   host, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA` for both libraries, instructions per call, sd 0): exp 189.0 / glibc
   80.0 (2.36x; FMA builds 1.93x), log 225.6 / 81.0 (2.79x), pow 494.4 / 193.0 (2.56x), sin 232.0 / 126.9 (1.83x),
   atan 216.1 / 111.1 (1.94x); f32 has no FMA builds: expf 73 / 46, logf 80 / 53, sinf 93 / 54, powf 243 / 93.
2. **Floating-point operations on the critical path set the time, not the instruction count.** On Ivy Bridge (4.1)
   the prototype L runs 20.3 ns against glibc's 13.8 at 92 against about 80 instructions; restructuring the same
   arithmetic for latency (LE: Estrin, k from the bits, the result beside the test) gives 19.2 ns throughput and 33.6
   against 42.2 ns latency. Liella's log barely overlaps consecutive calls (61.6 ns throughput, 73.4 latency; glibc
   11.7 and 26.8): a serialising step on log's path (an MXCSR write, a division chain or a store-forwarding stall) to
   find before any log redesign.

Consequence for the briefs: count instructions to work, and also state the FP-operation count and the critical-path
latency of each kernel as design targets; judge by timing (booked) since the instruction mix changes.

## 6. Recommendation (step 6, LIBM-RETHINK-2) and plan

Decision rows: three `D-NEW` rows appended to `plan/07-decisions/s10-mathematics.md` (two-phase `double` functions;
integer fixed-point quick tier for x87 and `_Float128`; complex from the real quick phases); the coordinator assigns
the ids. Code briefs in this directory, in order: `LIBM-F64-EXP-QUICK.md` (kernel, proof, tests), then
`LIBM-F64-EXP-QUICK-WIRE.md` (wiring + the envelope items), `LIBM-CPLX-EXP-TIER.md` (cexp/cexpf), `LIBM-LD-TRIG-INT.md`
(sinl/cosl integer kernel, shared later with `_Float128`). Each brief names its accuracy proof, tests, plants and a
booked timing check.

| class | recommended | expected end state vs glibc | first step |
|---|---|---|---|
| binary64 | 3.2 A: quick phase + `near` test, today's fast path as tier 2 | exp measured: Ivy Bridge 1.39x, N100 2.0x throughput, 1.34-1.38x latency (today 2.96x / 3.39x); target after the FP-operation cuts about 1.3x / 1.6x | exp quick phase (kernel + wiring briefs) |
| binary32 | 3.1 A (exhaustive) / B for pow and special | 0.6-0.9x | expf/logf/sinf quick, exhaustive proof job |
| long double | 3.3 A integer fixed point; keep §21 tier as tier 2 | faster than glibc in cycles (x87 microcode) | expl/logl integer kernel, then sinl/atanl |
| binary128 | 3.4 A three-limb fixed point | 0.1-0.2x (glibc is software) | shared limb kernels with long double |
| complex | 3.5 A compose real quick phases | 0.5-1.1x | cexp/cexpf, then csqrt/clog |

Order by value: complex and long double/binary128 remove 100-700x outliers (each row moves by two orders of
magnitude), binary64 exp/log/pow/sin are the most called. No ulp-bound ruling is needed now: every class keeps CR
with a fast tier. One watch item: the N100 throughput of the CR exp quick phase is 2.0x (the owner's 2x line for a
ulp-bound question); the kernel brief's FP-operation cuts are expected to bring it to about 1.6x, and the question
is raised only if the wired tier stays above 2x in the booked timing. Also open: the serialising step in Liella's
`log` (5.4, 4.1 reading 6) before the log quick phase; directed modes in place (the envelope finding, 5.1 item 4) as
a later brief once the quick phases exist (a test evaluated in the caller's mode needs its own proof).
