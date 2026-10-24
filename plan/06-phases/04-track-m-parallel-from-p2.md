<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Track M (parallel from P2): Mathematics (`libm`)

**Schedule (owner ruling 2026-10-04):** Track M was never started in parallel as planned (no
check compared pending work with the schedule; `cargo xtask status --schedule` now does). It runs
now as its own thread (branch `track/libm`). Target: correctness-complete (every §10 clause and
symbol) before the pause gate before phase 14 if feasible; otherwise it continues in parallel
with phase 14 and must be complete and benchmarked before phase 14 exits, never later.
Validation results that used stand-ins for missing math functions (the `modf` stand-in for `find`,
and any others) are provisional and are re-run when `libm` lands. FMA, vector and assembly paths
fall under the existing owner-ruling rule for unsafe and assembly (P10 (6)).

* **Scope:** §10 (1,163 symbols: 184 families plus 142 reserved-name entry points).
* **Clauses:** 56.
* **Needs:** P2 (IP-18 for `nan()`/payload tests, IP-26), plus `rt::fpu` (MXCSR/x87
  control).
* **Internal API shape:** a `Float` trait over binary32, binary64, x87-80 and binary128. One
  generic implementation per operation returns `(value, Status)`, where `Status` holds the
  IEEE flags. `fenv` is a safe API with SSE and x87 kept in step (spec §10.6 ⚑ HIST).
* **Translation:** one internal function exported under all aliases (`f32` = `float`,
  `f64` = `double`, `f64x` = `long double`, and so on). `errno` is set per
  `math_errhandling` in the translation (D-10.2). The 107 `__*_finite` names are compat-only
  in glibc 2.39 and belong to phase 15 with the other compat-only names (ruling #800); Track M
  does not export them.
* **Test surface:**
  * Exhaustive binary32 sweeps for every unary function.
  * Randomised plus hard-case corpora for the wider formats, checked against a test-only
    arbitrary-precision oracle written in this project.
  * The Annex F/G special-case tables (every row of §10.1–10.5 is a clause).
  * The flag behaviour of each function under each rounding mode.
* **Exit:** every family specified. Accuracy targets met (D-10.3): correctly rounded for
  `sqrt`/`fma`/narrowing/`fmod`/`remainder`/rounding/classification, and a documented,
  tested ULP bound for each other function.
* **Survey** (milestone 1, 2026-10-04). §10 has 1,163 names: 184 families (1,021 names) and
  142 reserved-name entry points; `libm.so.6` exports 1,037 default and 147 compat versions,
  and 13 math names live only in `libc.so.6` (`isinf`, `isnan` and their `__` forms) while 24 are
  in both (`frexp`, `ldexp`, `modf`, `scalbn`, `copysign`, `finite`, `__finite*`, `__signbit*`).
  Formats: binary32 (`f`, `f32`), binary64 (none, `f64`, `f32x`), x87 extended (`l`, `f64x`),
  binary128 (`f128`). Correctly rounded by contract: `sqrt`, `fma`, the narrowing operations,
  `fmod`/`remainder`/`remquo`, the rounding and classification functions and the rest of §10.2.
  The published bounds of the others (the manual's "Errors in Math Functions", x86_64 column)
  are `spec/abi/libm-ulps.tsv`: for example 1 ulp for `exp`/`log`/`sin`/`pow` in binary64, 2 for
  `sinl`, 8–9 for `tgamma`, 9 for `j0f`, `5 + i 2` for `cpowf`.
* **Schedule** (owner ruling 2026-10-04): correctness-complete (every §10 clause and symbol)
  before the pause gate before phase 14 if feasible; otherwise Track M continues in parallel
  with phase 14 and is complete and benchmarked before phase 14 exits. FMA, vector and assembly
  paths are each an owner ruling (P10 (6)); the safe scalar path comes first and is complete on
  its own.
* **Milestones** (proposed in milestone 1; each ends with a report and the full gate):
  1. *Foundations* (done 2026-10-04, see Delivered).
  2. *Elementary functions, binary32 and binary64* (done 2026-10-05, see Delivered): `exp`/`exp2`/`exp10`/`expm1`, the
     logarithms, `pow`, `cbrt`, `hypot`, trigonometric and inverse (Payne–Hanek reduction),
     `sincos`, hyperbolic and inverse; coefficients from `cargo xtask remez`. Accuracy: binary32
     correctly rounded (exhaustive sweeps, 2^32 inputs per unary function), binary64 within the
     published bound and aiming at correct rounding on the random and hard-case corpus. About
     3 sessions; sweeps on an earlier machine (Zen 1 for the long ones) under `~/bin/shared`.
  3. *Special functions, binary32 and binary64*: `erf`/`erfc`, `tgamma`/`lgamma`/`lgamma_r`/
     `gamma`/`signgam`, Bessel `j0`/`j1`/`jn`/`y0`/`y1`/`yn`. Within the published bounds
     (finding #14's `y0` underflow settled by probe). Done 2026-10-05 (see Delivered; design
     D-10.15, conventions D-10.16): every result correctly rounded instead of within the bounds.
  4. *Complex* (§10.5) in every format, Annex G special-case tables, `__clog10*`. About 2
     sessions.
  5. *x87 and binary128* elementary and special functions (software arithmetic, generic over the
     same code where the algorithms allow), the final measurement table, `cargo xtask status`
     s10 at 0 pending except the phase-15 compat names. About 3 sessions.
  6. *Speed* (coordinator decision 2026-10-05, after correctness-complete): a plain-`double`
     fast path with an error bound and a rounding test, the double-double path only when the
     test fails (Ziv's strategy), with results bit-identical to the current ones (the
     differential and exhaustive tests are its gate); the MXCSR save and restore only when the
     caller's mode is not the default, if results stay identical. If correctness-complete
     slips past the pause gate, the fast path for `exp`, `log`, `pow`, `sin`, `cos` and their
     `float` forms before phase 14 exits. FMA stays an owner ruling.
* **Delivered** (milestone 1, 2026-10-04):
  * **Foundations:** the `Float` trait over the four formats with decoding, packing and one
    rounding routine (tininess after rounding, as x86 detects it); `Status` (flags and the
    `errno` report, D-10.2); exact sum, product, quotient, square root and fused multiply-add on
    256-bit intermediates rounded once into any format (D-10.9); `rt::fpu` (§10.6, D-10.5).
  * **Functions:** every family of §10.1, §10.2, §10.4 and §10.6 plus `sqrt`, in every format:
    557 libm and 37 libc exports (`cargo xtask math-gen`, D-10.8). Their behaviour equals glibc's
    on the 115,000-line differential test except the recorded differences.
  * **Products:** `libm.a` and `libm.so.6` (D-10.7), checked by `nm` and `readelf`.
  * **Tooling (D-10.12):** the oracle and the Remez tool (`crates/mathtools`), the measurement
    harness `cargo xtask mathcmp`, the published bounds `spec/abi/libm-ulps.tsv`.
  * **Spec corrections** (three-way rule): §10.0 errno rule (per-function exceptions; `ERANGE`
    only for ±∞/±0 results), §10.2 `fromfp` (integer results in 2.39) and `canonicalize`
    (invalid encodings), §10.6 return values of the trap functions; Appendix B's six truncated
    family names (`cei`, `crea`, `er`, `fmu`, `isin`, `mod`) merged into their families.
* **Delivered** (milestone 2, 2026-10-05):
  * **Functions:** `exp`, `exp2`, `exp10`, `expm1`, `log`, `log2`, `log10`, `log1p`, `pow`,
    `cbrt`, `sin`, `cos`, `tan`, `sincos`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`,
    `tanh`, `asinh`, `acosh`, `atanh` for `float` and `double` under every alias (`f`, `f32`,
    `f64`, `f32x`), and `hypot` in every format (D-10.13, D-10.14). Correctly rounded in every
    rounding direction (no Remez fit was needed: tables and series coefficients come from the
    oracle through `cargo xtask tables`).
  * **Tests:** clauses s10_3_c_0 and s10_3_c_r1 to r5 (oracle checks in every mode, the Annex F
    cases), `fam_hypot` and the `float`/`double` members of the other families
    (`s10_math_symbols.rs`; their family tests wait for the `long double` and `_Float128`
    members of milestone 5), and the differential test `elem_products.rs` (20,196 lines against
    glibc, every difference checked against the oracle; `spec/behaviour/libm-elem.md`).
  * **Tooling:** `cargo xtask tables` (checked in the gate); `mathcmp` prints Liella's wrong
    results and flag-difference samples; the oracle evaluates `tanh`, `expm1` and `hypot` near
    their limits at a representative operand (beyond its exponent range otherwise).
  * **Fix to milestone 1:** `hypot` of `long double` and `_Float128` lost the sticky bit of a
    negligible smaller operand (no inexact; wrong directed rounding); the squares and their sum
    are now kept exactly in 256 bits (`arith::hypot_exact`).
  * **Measurements:** `docs/libm-accuracy.tsv`, summarised in `docs/differences-from-glibc.md`.
  * **Speed** (an earlier machine, `~/bin/timing`, 2026-10-05, ns per call on arguments in [−10, 10]):
    Liella 340–700 ns (`exp` 361, `log` 414, `sin` 417, `pow` 700, `expf` 377) against glibc
    4–26 ns, 15–95 times slower. The profile shows the double-double evaluation itself (Dekker
    products without FMA in every step) and about 10% in the two MXCSR switches; the gap is the
    design (always the accurate path), not a local defect. A fast path (plain `double`
    evaluation with an error bound and a rounding test, the double-double path only when that
    test fails) is milestone 6 (coordinator decision 2026-10-05).
* **Delivered** (milestone 3, 2026-10-05):
  * **Functions:** `erf`, `erfc`, `tgamma`, `lgamma`, `gamma`, `lgamma_r`, `j0`, `j1`, `jn`,
    `y0`, `y1`, `yn` for `float` and `double` under every alias; the data objects `signgam` and
    `__signgam` (libm). Correctly rounded in every mode, evaluated in the new 384-bit
    `math::mp` (D-10.15); glibc's conventions for NaN signs, the sign of Γ(x) and zero limits
    (D-10.16).
  * **Tests:** clauses s10_3_c_r6 and r7; `fam_lgamma_r`, `fam_lgammaf_r`, `fam_lgammaf32_r`,
    `fam_lgammaf32x_r`, `fam_lgammaf64_r` and the `float`/`double` members of the other
    families (`s10_math_symbols.rs`); `mp_kernels.rs` (the 384-bit kernels against the oracle),
    `special_cr.rs` (every function and mode against the oracle, `jn`/`yn` at orders −7 to 200)
    and the differential test `special_products.rs` (every line where Liella differs from glibc
    checked against the oracle; `spec/behaviour/libm-special.md`).
  * **Tooling:** oracles for `erfc` (continued fraction with bracketing convergents),
    `lgamma`/`tgamma` (Spouge's formula), `j0`/`j1`/`y0`/`y1`/`jn`/`yn` (series, Hankel,
    recurrences, with the precision raised by the bits cancelled); `cargo xtask tables` makes
    the `mp` constants (Bernoulli numbers exactly, γ by Brent–McMillan); `math-gen` maps
    `lgammaf_r` and the like to the family `lgamma_r`; `cargo xtask so` exports `signgam`.
  * **Measurements:** `docs/libm-accuracy.tsv` (5,000 random operands per function, format
    and mode; every Liella result correctly rounded). Speed is not yet measured under
    `~/bin/timing`: the 384-bit path runs on every call and is far slower than the elementary
    functions; milestone 6 covers it.
* **Delivered** (milestone 4a, 2026-10-05):
  * **Functions:** `cabs`, `carg`, `creal`, `cimag`, `conj`, `cproj`, `cexp`, `clog`, `clog10`
    and `__clog10`, `cpow`, `csqrt`, `csin`, `ccos`, `ctan`, `csinh`, `ccosh`, `ctanh`, `casin`,
    `cacos`, `catan`, `casinh`, `cacosh`, `catanh` for `float` and `double` under every alias
    (`f`, `f32`, `f64`, `f32x`): each part correctly rounded in every mode (D-10.17), the
    special values as glibc (D-10.18). `math-gen` maps `complex float`/`complex double` to
    `mathret::Complex`.
  * **Tests:** clause s10_5_c_0 (every function against the oracle in every mode, Annex G rows,
    branch cuts, exact cases); `complex_float_and_double_members` (`s10_math_symbols.rs`: every
    member exported, the C ABI pair, flags and `errno`; the family tests wait for milestone 5's
    `long double` and `_Float128` members); the differential test `complex_products.rs`
    (130,096 grid lines in every direction and 68,000 random ones against glibc, every
    differing line checked against the oracle; `spec/behaviour/libm-complex.md`).
  * **Tooling:** the complex oracle `liella_mathtools::cplx`.
  * **Measurements:** `docs/libm-accuracy.tsv` (2,000 random operands per function and format,
    round-to-nearest; every Liella part correctly rounded; glibc bit-identical 11–96%).
    Speed not measured (milestone 6).
  * **Left for milestone 5:** the `long double` and `_Float128` members (complex forwarders in
    `math-gen`), then the family tests; a wider fallback precision for the remaining asymptotic
    regimes of D-10.17.
* **Delivered** (milestone 5a, 2026-10-05):
  * **Functions:** `exp`, `exp2`, `exp10`, `expm1`, `log`, `log2`, `log10`, `log1p`, `pow`,
    `cbrt`, `sin`, `cos`, `tan`, `sincos`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`,
    `tanh`, `asinh`, `acosh`, `atanh` for `long double` (`l`, `f64x`): correctly rounded in
    every mode (D-10.19). `math::real::Real` selects the implementation per format.
  * **Tests:** s10_3_c_0 (every function against the oracle in every mode, `long double`
    included), s10_3_c_r3/r4 (exact powers and roots, Payne–Hanek up to the largest finite
    value), `elementary_float_double_and_long_double_members` (`s10_math_symbols.rs`), and the
    differential test `elem_ld_products.rs` (25,176 lines against glibc: 5,512 differ, 4,690
    results where only glibc is not correctly rounded, the rest flags and `errno` by D-10.14;
    `spec/behaviour/libm-elem-ld.md`). The family tests wait for the `_Float128` members.
  * **Tooling:** the oracle's `pow` decides (−1)^y for integers of any size; `mathcmp`'s probe
    calls the `long double` elementary functions on the Liella side.
  * **Measurements:** `docs/libm-accuracy.tsv` (20,000 random operands per function in
    round-to-nearest, 5,000 per directed mode): every classified Liella result correctly
    rounded; glibc bit-identical 73.5–99.9% in round-to-nearest. Speed not measured (milestone 6;
    the 384-bit path runs on every call).
  * **Left for milestone 5:** the `_Float128` elementary
    and special functions, the complex `long double`/`_Float128` members, then the family tests.
* **Delivered** (milestone 5b, 2026-10-05):
  * **Functions:** `erf`, `erfc`, `tgamma`, `lgamma`, `gamma`, `lgamma_r`, `j0`, `j1`, `jn`, `y0`, `y1`, `yn`
    for `long double` (`l`, `f64x`): correctly rounded in every mode (D-10.20).
  * **Fixes:** `pow` of `float`/`double` computes every dyadic result of at most 128 bits exactly
    (`wide::exact_power`; `pow(9, 1.5)` = 27 raised inexact before); `jn` of tiny arguments with a
    dyadic leading term in every format (`jnl(2, 2^-300)` downward gave 2^-603, D-10.20).
  * **Tests:** s10_3_c_r3 (exact fractional powers in every mode), s10_3_c_r6/r7 (`long double`
    members against the oracle, the 80-bit range limits), `special_cr.rs` (every function and mode against
    the oracle, the 64-bit operands around the zeros of `lgamma` and of `j0`, `j1`, `y0`, `y1` below 30,
    factorials up to 100 and near the overflow at 1755), `special_float_double_and_long_double_members`,
    `fam_lgammal_r`, `fam_lgammaf64x_r` (`s10_math_symbols.rs`), and the differential test
    `special_ld_products.rs` (`spec/behaviour/libm-special-ld.md`).
  * **Measurements:** `docs/libm-accuracy.tsv` (`mathcmp` now calls the `long double` special functions
    on the Liella side).
  * **Left for milestone 5:** the `_Float128` elementary and special functions, the complex
    `long double`/`_Float128` members, then the family tests.
* **Milestone 5 remainder: stories** (ruling #808, planned by MATH-M5-PLAN 2026-10-22). The list is
  `cargo xtask symcov` class m: 105 default-version `libm` names, all missing from both `libm.so.6`
  and `libm.a`. They are 69 complex names (23 functions × `l`, `f64x`, `f128`), `__clog10l`, 24
  `_Float128` elementary names and 11 `_Float128` special names. glibc 2.39 has no `gammaf128`.
  The seven product stories M5-1 to M5-7 cover the 105 names exactly once.
  * **Facts every story uses** (read from the code on 2026-10-22):
    * `math::wide` (D-10.19) and `math::special` (D-10.20) are already generic over `Float`. Their
      thresholds come from `F::PREC`/`F::EMAX`, and `mp::reduce_wide` takes significands up to 113 bits.
    * `math::complex` (D-10.17) is bound to `elem::Small` (f32/f64 widened to f64), so it needs work
      before it can serve x87 `long double` or `_Float128`.
    * `math::real::Real` has no `F128` impl yet.
    * `math-gen` refuses complex `long double`/`_Float128` signatures ("need forwarders, not
      generated yet", `xtask/src/math.rs`).
    * `mp` has a fixed width of 384 bits (`LIMBS = 6`).
    * The oracle and `cmp` already know `Fmt::F128` (`'q'`).
    * The published bounds have `f128` and `l` columns (`spec/abi/libm-ulps.tsv`).
  * **ABI** (System V x86-64 psABI): `long double _Complex` is class COMPLEX_X87. It is returned in
    `st(0)` (real part) and `st(1)` (imaginary part), and passed as an argument in memory.
    `_Float128 _Complex` (32 bytes) is passed and returned in memory. Rust cannot name either type,
    so both get C forwarders (OD-C4, as for the real `long double` members). The C compiler then
    handles the convention, and the Rust side receives and returns a `repr(C)` pair of
    `LdBits`/`F128Bits`. No assembly is needed.
  * **Proof, the same for every story:**
    * Accuracy against glibc: a differential test in `crates/math/tests/`, built against the host
      C library and statically against Liella's products, on keke. It runs every function in every
      rounding direction on special, boundary and random operands. Every differing line is checked
      against the `liella-mathtools` oracle: Liella must be correctly rounded, and invalid,
      division-by-zero, NaNs and `errno` must follow D-10.14/D-10.18.
    * Accuracy target: correctly rounded in every mode, the same as the family D-rows (D-10.17,
      D-10.19, D-10.20). The `libm-ulps.tsv` bound of the format is the ceiling that may never be
      exceeded, and any known limit is recorded with its regime.
    * Exports: the member checks in `s10_math_symbols.rs` (exported, C ABI, flags, `errno`). After
      the story, symcov class m is smaller by exactly the story's names.
    * Measurements: `mathcmp` rows in `docs/libm-accuracy.tsv`, which give the glibc bit-identical
      fraction (D-10.3).
    * Plants on the new code: thresholds, part order, the forwarder's part swap.
    * Speed is not measured (milestone 6).
  * **M5-1 `MATH-M5-CPLX-FWD`** (size M, no dependencies; 17 names):
    * Names: `creal`, `cimag`, `conj`, `cproj` and `cabs` in `l`, `f64x` and `f128`, plus `cargl`
      and `cargf64x`.
    * Work: `math-gen` emits the complex forwarders into `csys/src/adapter/math_libm.c` and a
      `mathret` pair type for them. `cabs` uses the existing `hypot` (every format). `carg` uses
      the existing x87 `atan2`.
    * Proof: an ABI test from a host-compiled C caller covering the `st(0)`/`st(1)` return and the
      memory passing, including a sentinel against swapped parts. The exact functions must be
      bit-identical to glibc, and `cproj` must match glibc's NaN and infinity rows (D-10.18).
    * Writes D-10.21 (complex forwarders and their ABI).
  * **M5-2 `MATH-M5-CPLX-LD-1`** (size L, after M5-1; 11 names):
    * Names: `cexp`, `clog`, `clog10`, `cpow` and `csqrt` in `l` and `f64x`, plus `__clog10l`.
    * Work: lift `math::complex` from `Small` to any `Float`. That means rounding a `Part` from `Mp`
      into the format, and deriving its format-dependent cut-offs (the 2^-150/2^-300 short-dyadic
      tests, the exact-root candidates of 120 bits) from `PREC`/`EMAX`.
    * Proof: `complex_ld_products.rs` (the grid and random lines of milestone 4a over the x87
      exponent range) and s10_5_c_0 for `long double`.
    * Extends D-10.17.
  * **M5-3 `MATH-M5-CPLX-LD-2`** (size M, after M5-2; 24 names):
    * Names: `csin`, `ccos`, `ctan`, `csinh`, `ccosh`, `ctanh`, `casin`, `cacos`, `catan`, `casinh`,
      `cacosh` and `catanh` in `l` and `f64x`.
    * Work: the trigonometric, hyperbolic and inverse bodies on the generic core of M5-2. Check the
      Hull–Fairgrieve–Tang regions and the "±(1 + δ) far out" cut-off of `ctan`/`ctanh` against
      the x87 exponent range.
    * Proof: the same as M5-2 (the branch cuts and the Annex G rows included).
  * **M5-4 `MATH-M5-CPLX-F128`** (size M, after M5-3; 18 names):
    * Names: the 17 functions of M5-2 and M5-3 in `f128`, plus `cargf128`.
    * Work: the 113-bit cut-offs. Exact-root candidates need up to 226-bit squares; the
      short-dyadic tests leave less margin inside 384 bits. This story derives each cut-off again
      rather than reusing the `long double` values. `cargf128` takes `atan2` from `mp`.
    * Proof: `complex_f128_products.rs`; s10_5_c_0 for `_Float128`.
    * If a regime cannot be rounded within 384 bits, stop and queue M5-W (do not ship a guess).
  * **M5-5 `MATH-M5-F128-ELEM-1`** (size M, no dependencies; 10 names):
    * Names: `expf128`, `exp2f128`, `exp10f128`, `expm1f128`, `logf128`, `log2f128`, `log10f128`,
      `log1pf128`, `powf128` and `cbrtf128`.
    * Work: `impl Real for F128` by `math::wide` (all of `Real`; only these ten are exported here).
      Re-derive the exact-result ranges for 113 bits: `exp10`/`log10` exact while 5^k < 2^113
      (k ≤ 48), the dyadic `pow` results, and the perfect cubes.
    * Proof: `elem_f128_products.rs`, s10_3_c_0 and r3 for `_Float128`, and the `mathcmp` probe
      calling the `f128` functions on the Liella side.
    * Extends D-10.19.
  * **M5-6 `MATH-M5-F128-ELEM-2`** (size M, after M5-5; 14 names):
    * Names: `sinf128`, `cosf128`, `tanf128`, `sincosf128`, `asinf128`, `acosf128`, `atanf128`,
      `atan2f128`, `sinhf128`, `coshf128`, `tanhf128`, `asinhf128`, `acoshf128` and `atanhf128`.
    * Work: check glibc's conventions for these forms by probe (the NaN signs, as the `long double`
      forms did in `libm-elem-ld.md`).
    * Proof: s10_3_c_r4 (Payne–Hanek up to the largest `_Float128`) and the M5-5 test extended to
      these functions.
  * **M5-7 `MATH-M5-F128-SPECIAL`** (size M, no dependencies; 11 names):
    * Names: `erff128`, `erfcf128`, `tgammaf128`, `lgammaf128`, `lgammaf128_r`, `j0f128`, `j1f128`,
      `jnf128`, `y0f128`, `y1f128` and `ynf128`.
    * Work: `math::special` on `F128`. Settle by probe whether `lgammaf128` writes `signgam` (D-10.16).
    * Precision bound first: the Bessel series cancels up to about 150 of the 384 bits, which leaves
      about 234 bits, and 113-bit operands next to the zeros of `lgamma`, `j0`, `j1`, `y0` and `y1`
      cancel further. The story writes the worst case per regime into D-10.20 before claiming
      correct rounding. Where a regime falls short, it stops that regime and queues M5-W.
    * Proof: `special_f128_products.rs`, `special_cr.rs` for `_Float128` (the operands around the
      zeros, factorials, the overflow of `tgamma` near 1755), and `fam_lgammaf128_r`.
  * **M5-W `MATH-M5-MP-WIDE`** (size L, queued only on a finding of M5-4 or M5-7; no names):
    * A wider second step for `mp` (Ziv's strategy) with constants from `cargo xtask tables`.
    * It also removes the known limit of D-10.17 (directed modes in the remaining asymptotic
      regimes).
  * **M5-8 `MATH-M5-CLOSE`** (size S, after M5-1 to M5-7):
    * The family tests of `s10_math_symbols.rs` that wait for the `long double`/`_Float128` members
      (elementary, special, complex).
    * The final measurement table in `docs/libm-accuracy.tsv` and its summary in
      `docs/differences-from-glibc.md`.
    * `cargo xtask status`: s10 at 0 pending except the phase-15 compat names. symcov class m at 0.
    * The README coverage line and the milestone 5 Delivered entry.
  * **Order:** M5-1, M5-5 and M5-7 can start in parallel. They touch `s10_math.tsv` and
    `real.rs` in separate rows, so the merges are textual only. Then M5-2 → M5-3 → M5-4 and
    M5-5 → M5-6, and finally M5-8. Names: 17 + 11 + 24 + 18 + 10 + 14 + 11 = 105.
* **Milestone 6: speed, stories** (planned by MATH-M6-PLAN 2026-10-23; owner 2026-10-23: glibc's speed is the bar to
  reach per function, not a ceiling, and correct rounding in every mode stays, D-10.13/D-10.15/D-10.17).
  * **Where we stand** (keke, AMD EPYC 7551 (Zen 1, FMA and AVX2), idle (load 1.3 to 2.8 on 80 threads), not booked;
    origin/next 42e682a3; job `math-m6-plan-bench-2`). The harness is a scratch C program
    (`~/work/math-m6-plan/libm_ns.c` on keke). It is built the way `cargo xtask mathcmp` builds: `-O2 -fno-builtin`,
    statically against `target/liella/{libm,libc}.a`, and against host glibc 2.39 both statically and dynamically.
    It measures throughput: 256 independent uniform operands in the range given, round-to-nearest, pinned to one CPU.
    Each function is run 5 times for ≥ 20 ms; the median is taken, and the whole thing runs twice per library. Cells
    show Liella / glibc-static ns per call (×ratio). glibc-dynamic is within 0.1 to 0.5 ns of static except where
    noted. † = noisy: the two runs' medians differ by more than 10%. All other cells agree within 10% (most within 3%).
    Ranges: exp family, trig, atan, hyperbolic, tgamma/lgamma [−10, 10]; logs [0.01, 100]; log1p [−0.9, 10];
    asin/acos [−1, 1]; atanh [−0.99, 0.99]; acosh [1, 100]; erf/erfc [−5, 5]; j [−20, 20]; y (0.01, 20]; jn/yn order 3;
    pow x [0.01, 10], y [−10, 10]; hypot [−100, 100]²; atan2 [−10, 10]²; cbrt [−100, 100]; sqrt [0, 100];
    complex z in [−3, 3]², cpow exponent 1.5+0.25i. `csinl` is not exported yet (M5-3).

| Family | float | double | long double | _Float128 |
|---|---|---|---|---|
| `exp` | 325 / 3.9 (×84) | 319 / 6.7 (×48) | 7,009 / 33.3 (×211) | 7,062 / 1,105 (×6.4) |
| `exp2` | 310 / 3.9 (×79) | 305 / 4.0 (×75) | 7,105 / 26.9 (×264) | 7,138 / 1,200 (×5.9) |
| `exp10` | 343 / 4.0 (×87) | 335 / 5.7 (×59) | 30,419 / 33.5 (×908) | 30,398 / 2,203 (×14) |
| `expm1` | 339 / 18.7 (×18) | 335 / 16.8 (×20) | 7,154 / 29.7 (×241) | 7,109 / 1,454 (×4.9)† |
| `log` | 350 / 4.0 (×88) | 345 / 6.0 (×58) | 23,517 / 34.0 (×692) | 23,489 / 1,133 (×21) |
| `log2` | 362 / 3.8 (×95) | 360 / 6.2 (×58) | 24,638 / 33.7 (×731) | 24,602 / 1,073 (×23) |
| `log10` | 364 / 9.0 (×41) | 376 / 12.1 (×31) | 48,033 / 34.1 (×1408) | 48,078 / 1,199 (×40) |
| `log1p` | 363 / 14.8 (×25) | 358 / 14.0 (×26) | 23,526 / 37.1 (×633) | 23,346 / 1,314 (×18) |
| `pow` | 621 / 8.9 (×70) | 618 / 19.8 (×31)† | 30,207 / 423 (×71) | 30,297 / 3,026 (×10) |
| `cbrt` | 293 / 18.7 (×16) | 291 / 21.0 (×14) | 5,829 / 79.7 (×73) | 5,908 / 920 (×6.4) |
| `hypot` | 277 / 4.1 (×68) | 475 / 10.4 (×46) | 569 / 33.5 (×17)† | 914 / 1,095 (×0.8)† |
| `sqrt` | 213 / 3.9 (×55) | 453 / 3.8 (×118) | 563 / 10.6 (×53) | 941 / 545 (×1.7) |
| `sin` | 384 / 7.4 (×52) | 376 / 15.5 (×24) | 16,959 / 219 (×77) | 16,975 / 1,051 (×16) |
| `cos` | 388 / 7.9 (×49) | 381 / 16.2 (×24) | 17,060 / 224 (×76) | 17,027 / 1,058 (×16) |
| `tan` | 453 / 20.1 (×23) | 446 / 18.5 (×24) | 18,131 / 201 (×90) | 18,240 / 1,227 (×15) |
| `asin` | 472 / 7.0 (×67) | 465 / 8.4 (×55) | 29,695 / 53.0 (×561) | 29,903 / 1,430 (×21) |
| `acos` | 496 / 9.5 (×52) | 491 / 9.3 (×53) | 30,044 / 68.5 (×439) | 30,069 / 1,496 (×20) |
| `atan` | 403 / 10.7 (×38) | 397 / 14.7 (×27) | 27,225 / 55.0 (×495)† | 27,248 / 876 (×31) |
| `atan2` | 454 / 21.7 (×21)† | 448 / 30.9 (×15) | 28,364 / 60.3 (×470) | 28,260 / 982 (×29) |
| `sinh` | 405 / 28.8 (×14)† | 398 / 28.4 (×14) | 8,487 / 50.5 (×168) | 8,729 / 1,595 (×5.5)† |
| `cosh` | 409 / 10.3 (×40)† | 408 / 10.7 (×38)† | 8,446 / 65.3 (×129) | 8,493 / 1,201 (×7.1) |
| `tanh` | 407 / 23.3 (×18) | 402 / 23.6 (×17) | 9,596 / 44.9 (×214) | 9,462 / 1,579 (×6.0) |
| `asinh` | 442 / 16.1 (×28) | 434 / 17.9 (×24) | 26,108 / 65.1 (×401) | 26,224 / 2,018 (×13) |
| `acosh` | 424 / 9.4 (×45) | 416 / 12.5 (×33) | 25,185 / 65.3 (×386) | 25,278 / 1,808 (×14) |
| `atanh` | 434 / 22.9 (×19) | 428 / 23.3 (×18) | 24,432 / 76.6 (×319) | 24,560 / 1,671 (×15) |
| `erf` | 39,249 / 22.8 (×1724) | 38,451 / 22.4 (×1713) | 36,670 / 104 (×351) | 38,122 / 2,933 (×13) |
| `erfc` | 38,176 / 23.0 (×1663) | 38,568 / 23.5 (×1643)† | 38,293 / 111 (×344) | 36,520 / 2,859 (×13) |
| `tgamma` | 118,558 / 88.3 (×1343) | 122,291 / 130 (×939) | 116,942 / 496 (×236) | 117,588 / 5,051 (×23) |
| `lgamma` | 112,002 / 50.1 (×2237) | 109,901 / 65.6 (×1675) | 112,589 / 269 (×418) | 112,145 / 8,419 (×13) |
| `j0` | 28,640 / 50.9 (×562) | 29,305 / 67.9 (×432)† | 29,609 / 669 (×44)† | 30,286 / 5,517 (×5.5) |
| `j1` | 30,206 / 52.1 (×580) | 29,059 / 71.5 (×407) | 29,108 / 672 (×43) | 28,440 / 5,441 (×5.2) |
| `y0` | 99,041 / 55.6 (×1782) | 98,743 / 72.2 (×1369)† | 100,282 / 665 (×151)† | 98,325 / 5,695 (×17)† |
| `y1` | 99,307 / 55.6 (×1787) | 99,223 / 71.6 (×1385)† | 100,654 / 684 (×147) | 98,518 / 5,700 (×17) |
| `jn` | 29,328 / 142 (×207) | 30,333 / 169 (×180) | 29,671 / 1,382 (×21) | 29,739 / 12,067 (×2.5) |
| `yn` | 98,022 / 129 (×762) | 100,505 / 156 (×643) | 100,220 / 1,365 (×73) | 99,314 / 12,044 (×8.2) |
| `cexp` | 24,209 / 52.4 (×462) | 24,177 / 34.8 (×696) | 24,065 / 351 (×69) | - |
| `clog` | 24,380 / 74.3 (×328) | 24,380 / 72.9 (×335) | 52,303 / 164 (×318) | - |
| `csin` | 25,580 / 96.4 (×265) | 25,072 / 73.1 (×343) | - | - |
| `csqrt` | 6,254 / 51.0 (×123) | 6,239 / 32.1 (×195) | 6,386 / 95.7 (×67) | - |
| `cpow` | 76,366 / 143 (×533) | 76,222 / 120 (×635) | 76,452 / 552 (×138) | - |

  * **What the table says:** Liella is slower than glibc in every cell but two (`hypotf128`, and `sqrtf128` is within
    ×1.7). The elementary `float`/`double` functions are ×14 to ×118, the same 290 to 620 ns for both formats, because
    both run the double-double path. `long double` costs the same as `_Float128`: both run the 384-bit `mp` path, so
    they are ×21 to ×1408 against glibc's x87 code. The special and complex functions of every format, `float` and
    `double` included, run `mp` (24 to 122 µs). `sqrt` runs in software (213 to 941 ns) although SSE `sqrtss`/
    `sqrtsd` and x87 `fsqrt` are correctly rounded in the current rounding direction by definition (IEEE 754, Intel
    SDM).
  * **The cost of the glibc-compatible envelope, measured in Liella (same job, `double` unless noted):**
    | Probe | Liella | glibc static | What it shows |
    |---|---|---|---|
    | `stmxcsr`+`ldmxcsr` pair | 8.7 | - | one environment switch |
    | the wrapper's MXCSR sequence (read mode; save and load default; restore; set inexact) | 24.7 | - | `mathret::elem` without the function |
    | `exp(qNaN)` (no evaluation) | 24.2 | 6.1 | Liella's per-call floor is the MXCSR sequence |
    | `expf(qNaN)` | 25.8 | 3.7 | |
    | `exp(tiny)` (result 1, inexact) | 99.8† | 5.1 | |
    | `exp(1000)` (overflow: flags and `errno` ERANGE) | 50.4 | 10.9 | errno and flags only on exceptional operands |
    | `exp`, `log`, `pow`, `expf` in FE_UPWARD | = nearest | = nearest (6.7, 6.0, 19.8, 3.9) | directed modes cost glibc nothing in these |
    | `sin`, `sinf` in FE_UPWARD | = nearest | 82.0 / 7.4 (dynamic 27.4) | glibc's `sin` pays in directed modes (unexplained static/dynamic gap; not investigated, glibc's side) |
    | `exp`/`log`/`pow` at GLIBC_2.2.5 (dynamic) | not exported | 6.8 / 6.4 / 20.2 | no cost over the current versions |
    | `expf`/`logf`/`powf` at GLIBC_2.2.5 (dynamic) | not exported | 5.9 / 4.6 / 12.5 | the old entries cost +2.0 / +0.6 / +3.5 ns over the 2.27 ones |
    | PLT (glibc dynamic − static) | - | 0.1 to 0.5 | negligible |
    | `sin` on [10⁶, 10³⁰⁰] (Payne–Hanek) | 422 | 66.8 | |

    The previous estimate of "about 10%" in the two MXCSR switches (milestone 2) is out of date. The switches take
    24 ns, which is 8% of today's 320 ns `exp`, but once the fast path lands they would be 3.6 times glibc's whole
    `exp`. They come first. The `errno` and flag costs fall only on exceptional operands, and directed-mode support
    costs glibc nothing in `exp`/`log`/`pow`. So none of these gives an opt-in variant a measured gain worth an
    extension (see the ruling items).
  * **Method, the same for every fast-path story (Ziv's strategy):**
    * The fast path evaluates in the format or a cheap extension: `double` for `float`; double-double with FMA, or an
      FMA-free bound, for `double`; double-double or 128-bit integer fixed point for x87 `long double`; 128/192-bit
      integer fixed point for `_Float128`. It produces y and an error bound ε, both proved in a comment: argument
      reduction, the table error, the polynomial's minimax error from `cargo xtask remez`, and the rounding error of
      each step.
    * The rounding test: both ends of [y − ε, y + ε] round to the same encoding with the same flags in the caller's
      mode (the test already used by `round_bessel`). When it fails, the existing accurate path runs unchanged. So
      the results stay bit-identical to today's.
    * Proof: the existing accuracy suites stay green and unchanged (exhaustive binary32 sweeps, `elem_products`,
      `special_cr`, `special_products`, the complex suites, `docs/libm-accuracy.tsv` unchanged).
    * Plants that must be CAUGHT: ε halved, the rounding test inverted or disabled (always take the fast path), one
      table entry or coefficient perturbed by 1 ulp, the reduction's constant shortened.
    * A fallback-rate counter (M6-0) shows the fast path is taken, with a ceiling per function in the test, for
      example ≤ 1 in 10⁴ random operands.
    * Speed: the story's bench rows (M6-0) at or under glibc's per-call cost, or the remaining gap is stated with its
      cause.
  * **M6-0 `MATH-M6-TOOLS`** (size S, no dependencies): the repository's bench has no libm rows. This plan's numbers
    come from a scratch harness.
    * Move the harness into the repo as `bench/c/cbench_libm.c` rows (`cargo xtask bench`, `bench-rows`): every family
      and format, the envelope probes, and glibc static and dynamic as reference.
    * Add a test-only feature `fastpath-stats`, off in products, so it costs nothing. It counts fast-path and fallback
      calls per function, and a helper reads them in the accuracy tests.
    * Add a plants template for the five plants above.
  * **M6-1 `MATH-M6-ENVELOPE`** (size M, after M6-0): the entry wrapper (`mathret::elem`, `lgamma`, the complex and
    x87 forms).
    * Read MXCSR once (`stmxcsr`). When it is the default control word (round to nearest, all exceptions masked, no
      FTZ/DAZ, any flag bits), compute in place with no switch and no restore.
    * Raise the flags with one `ldmxcsr` only when a flag not already set must be set, or with an arithmetic
      operation that raises exactly that flag.
    * Keep the full switch for non-default control words. Directed modes then pay one switch, as today.
    * x87: the same with `fnstcw`.
    * Proof: every flags and `errno` test, the trap tests (SIGFPE with unmasked exceptions), and the full accuracy
      suites in all four modes, all bit-identical.
    * Plants: skip the restore; skip the flag merge; treat FTZ as default.
    * Expected gain: about 20 ns per call on every libm function (from 24.2 toward about 2 to 4 ns of floor).
  * **M6-2 `MATH-M6-SQRT`** (size S, after M6-1):
    * `sqrt`/`sqrtf`/`sqrtl` (and the `f32`/`f64`/`f32x`/`f64x` aliases) use the hardware square root (`sqrtsd`,
      `sqrtss`, x87 `fsqrt` at 64-bit precision control). The result and flags are correct by IEEE 754 definition in
      every mode. `errno` EDOM is set for x < 0 (not −0), and NaN handling follows D-10.14.
    * `sqrtf128`: a 128-bit integer Newton step with an exact remainder check, instead of the generic software path.
    * The same for the `fsqrt`/`dsqrt` narrowing family if it shares the code.
    * Proof: exhaustive `sqrtf` in four modes against the oracle; `elem_products` sqrt lines.
    * Expected: 453 → about 5 ns (`double`), 213 → about 5 (`float`), 563 → about 12 (`long double`).
  * **M6-3 `MATH-M6-F32-CORE`** (size M, after M6-0/M6-1): `expf`, `exp2f`, `exp10f`, `expm1f`, `logf`, `log2f`,
    `log10f`, `log1pf`, `powf`, `sinf`, `cosf`, `tanf`, `sincosf`, `cbrtf`, `hypotf`.
    * Evaluate in `double` with a table and polynomial. The bound is about 2⁻⁵⁸ relative, so the rounding test fails
      about once in 2²⁹ operands.
    * Proof: the exhaustive 2³² sweeps in all four modes, bit-identical to the current results (the strongest gate in
      the milestone).
    * Can run in parallel with M6-4. Target: glibc's 4 to 20 ns.
  * **M6-4 `MATH-M6-F64-EXPLOG`** (size L, after M6-0/M6-1): `exp`, `exp2`, `exp10`, `expm1`, `log`, `log2`,
    `log10`, `log1p`.
    * Table-driven reduction (2^(i/N) and 1/c tables stored as double-double), then a double polynomial with the
      leading terms in double-double. The bound is about 2⁻⁶⁸ relative, with fallback to the existing double-double
      path.
    * Proof: `elem_products`, the hard-case corpus, and the random corpus with the fallback rate.
    * Target: 6 to 15 ns. Gain about 300 ns per call on the most common `double` functions.
  * **M6-5 `MATH-M6-F64-POW`** (size M, after M6-4):
    * `pow` from M6-4's log with about 2⁻⁷⁰ error carried in double-double, and exp of a double-double argument.
    * The exact cases (integer y, exact powers) before the test. The overflow and underflow limits proved.
    * Target: 20 ns.
  * **M6-6 `MATH-M6-F64-TRIG`** (size L, after M6-1; parallel with M6-4):
    * `sin`, `cos`, `tan`, `sincos`. Cody–Waite reduction with a three-part π/2 for |x| < 2^20 (bound proved), the
      existing Payne–Hanek above that, and double polynomials with the bound.
    * Proof: `elem_products`, the trig hard cases near multiples of π/2, and the huge-argument corpus.
    * Target: 15 to 20 ns (Payne–Hanek range 66 ns).
  * **M6-7 `MATH-M6-F64-REST`** (size M, after M6-4/M6-6): `asin`, `acos`, `atan`, `atan2`, the hyperbolic and
    inverse hyperbolic functions, `cbrt` and `hypot`, in `double` (and any `float` member left by M6-3), built on the
    M6-4/M6-6 kernels.
  * **M6-8 `MATH-M6-SPECIAL-SMALL`** (size L, after M6-4):
    * `erf`, `erfc`, `tgamma`, `lgamma`, `lgamma_r`, Bessel `j0`/`j1`/`y0`/`y1`/`jn`/`yn` for `float`/`double`. They
      are 28 to 122 µs today, ×400 to ×2200 against glibc.
    * A double-double fast path: rational or polynomial fits per interval from `remez`, Stirling with a bound for Γ,
      and Hankel for large x with the existing bound from `special.rs`. Fallback to `mp`.
    * Near the zeros of lgamma and the Bessel functions, the test fails by design and `mp` runs.
    * Proof: `special_cr` and `special_products` unchanged, plus the fallback rate. Can be split per family if it
      runs over.
  * **M6-9 `MATH-M6-BESSEL-RECUR`** (size M, after M6-8; folds #841 and #868's recurrence part):
    * Derive an error bound for the forward and Miller recurrences: n² > x beyond 100, and n > 160. Without it, these
      regimes are rounded without proof (#868). That is a correctness gap, so this story is ordered before any speed
      work on `jn`/`yn` at large orders.
    * With the bound, run the O(n) recurrence in double-double or 192-bit fixed point instead of 384-bit `mp` (#841:
      `jnf128` 30 µs, `ynf128` 99 µs at order 3, growing with n). The rounding test, then fallback to `mp`/`mpw`.
    * Proof: `special_cr` Bessel at orders −7 to 200 and n > 5000 (the open M5-7 operand), and a plant that breaks
      the bound.
  * **M6-10 `MATH-M6-CPLX-DIRECTED`** (size M, no dependencies; the D-10.17 part of #868, correctness, not speed):
    * Directed modes in the remaining complex asymptotic regimes, with a bound and the rounding test, as M5-W left
      it. It must land before M6-11, so that the fallback M6-11 relies on is correct in every mode.
  * **M6-11 `MATH-M6-CPLX`** (size L, after M6-4/M6-6/M6-10):
    * `cexp`, `clog`, `csin`/`ccos`/`ctan`, `csqrt`, `cpow` and the rest in `float`/`double`. They are 6 to 76 µs
      today, so they run `mp`.
    * The fast path composes the real fast kernels with a combined part-wise bound. The Annex G special cases run
      before it.
    * Target: glibc's 30 to 140 ns.
  * **M6-12 `MATH-M6-LD-ELEM`** (size L, after M6-4/M6-6 design; x87 `long double`): exp, log, pow, trig and the rest.
    * Evaluate in double-double (106 bits, ε about 2⁻¹⁰⁰ against a 64-bit target), or in 128-bit fixed point, with
      fallback to `mp`.
    * Target: glibc's x87 30 to 220 ns, against 6 to 48 µs today. The largest ratios in the table (×200 to ×1400).
      Split into exp/log/pow and trig/inverse/hyperbolic if needed.
  * **M6-13 `MATH-M6-F128-ELEM`** (size L, after M6-12):
    * `_Float128` elementary functions in 128/192-bit integer fixed point (u128 products), with fallback to `mp`.
    * Target: glibc's 0.9 to 3 µs, against 6 to 48 µs today.
  * **M6-14 `MATH-M6-WIDE-SPECIAL`** (size L, after M6-8/M6-12/M6-13): the special and complex functions in
    `long double` and `_Float128`, the same scheme on the wider kernels.
  * **Order and parallelism:**
    * M6-0 → M6-1 → M6-2 first. They are small, and every later gain is measured against them.
    * Then M6-3, M6-4 and M6-6 in parallel. They touch separate source files, plus shared rows in `real.rs`.
    * Then M6-5, M6-7, M6-8 → M6-9.
    * M6-10 can run any time. M6-11 after M6-10.
    * M6-12 → M6-13 → M6-14.
    * Expected gain on real programs: M6-1/M6-2/M6-3/M6-4/M6-5/M6-6 cover the functions real programs call
      (`exp`, `log`, `pow`, `sin`/`cos`, `sqrt` and their `float` forms). The rest follow by call frequency.
  * **Not stories** (with reasons):
    * An `errno`-free entry: `errno` is set only on exceptional operands (`exp(1000)` 50 ns), so a variant gains
      nothing on normal ones.
    * A round-to-nearest-only entry: after M6-1, the mode check is one `stmxcsr` and a compare, and directed modes
      cost glibc nothing in `exp`/`log`/`pow`. Re-measure after M6-1, and propose it only if the floor stays above
      about 2 ns.
    * Symbol versions: no call-time cost (PLT 0.1 to 0.5 ns).
