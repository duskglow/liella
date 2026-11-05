# LIBM-F32-ERF: handoff (erff, erfcf on the fast32 PATTERN)

State: branch `task/libm-f32-erf` (stacks on `task/libm-f32-double-kernels-2`), code head `0649d1af`, role
keke:~/w/n1. The run reached the context checkpoint. Jobs on keke: `libm-f32-erf-verify` (fast32 unit tests on
680f8534, 13 passed, rc 0), `libm-f32-erf-lint` (math-gen + workspace clippy -D warnings + xtask static on the
final code, rc 0), `libm-f32-erf-gate` (direct tier, `--base origin/next`, started detached on 0649d1af; result
not read). NOT run: plants, exhaustive sweeps, counts. Verdict: INCOMPLETE.
Note: one sync (before 680f8534) delivered fast32.rs but left the old s10_math.tsv on the role (cause not
investigated); the sync after the commit delivered it. Check the role's copy after a sync of uncommitted edits.

## Done (code, this branch)
* `crates/mathtools/src/remez.rs`: the Remez tool gets an optional `shift = c` key (fit p(t) ≈ f(c + t), parity
  fold off) and an `erfcx` target (erfc(x)·e^(x²), both factors at p + 8 bits). Needed for centred pieces.
* Fits (D-10.3, the derivation is the fit files plus the generated headers): `crates/math/fits/fast32_erf.fit`
  (erf on [-1, 1], odd 1..21, 3.67u after rounding) and `fast32_erfcx0..5.fit` (erfcx on [1, 3/2, 9/4, 27/8,
  81/16, 243/32, 37/4], each centred, degree 15; 0.09/0.89/0.19u for pieces 0-2, 3-5 see the generated files).
  Degrees from a trial job (`libm-f32-erf-trialfit`, killed after 13 of 36 fits, evidence only): degree 12 lost
  about 2^3.5 per piece (2^-55.8, -51.2, -47.7, -45.2 on pieces 0-3); degree 15 holds 2^-53 or better.
* `crates/math/src/fast32.rs`: `Fast::Erf`, `Fast::Erfc`; `erf_small` (x·P(x²), 7u), `erfc_big`
  (e^(-a²)·Q_i(a - c_i), 153u, reusing the exp reduction through the new `exp_z`, which `exp_core` now calls;
  same operations), `erf_core` (bound 26u, DELTA 40), `erfc_core` (bound 160u, DELTA 248). Saturation: erf for
  |x| ≥ 4 returns ±(1 - 2^-30) and erfc for x ≤ -4 returns 2 - 2^-29: the true value and these lie in one
  interval without a `float` rounding boundary (proof in the ERF_SAT comment), so the rounding test is exact there.
  erfc x ≥ 9.25 (subnormal results), ±0, infinities, NaN go to `crate::special`.
* Wiring: `s10_math.tsv` rows `erf f` / `erfc f` now call `math::fast32::erf` / `erfc` (d, l, q unchanged);
  `s10_math_gen.rs` must be REGENERATED with `cargo xtask math-gen` (done on the role by the verify job; the
  regenerated file still has to be copied into the worktree and committed if it is not on the branch).
* Tests: `erf_fast_path_agrees_with_the_accurate_path` (stride, grid, piece edges ±8 floats, special values
  refused), `erf_cores_are_within_their_bounds` (against the correctly rounded `double` accurate path, gap
  bound - 1 ulp), the fallback-rate rows for erff/erfcf in `fast_path_agrees_with_the_accurate_path`, ignored
  `erff_/erfcf_fast_path_agrees_exhaustively`. `agree()` now covers only the first 15 functions (CHEAP): the
  accurate erf is 384-bit mp, too slow for its wide strides.

## Open / next step (exact)
1. Read `~/logs/libm-f32-erf-verify.log` on keke (math-gen + `cargo test --release -p liella-math --lib fast32`);
   fix what it finds; copy back `crates/boundary/src/posix/s10_math_gen.rs` and `crates/math/src/coeffs/
   fast32_erf*.rs` if not committed.
2. `cargo xtask clippy`, `cargo xtask static`, then the gate (generated files: escalates to FULL), plants
   (ERF_DELTA/ERFC_DELTA, one coefficient, a piece boundary in ERFCX_LO, ERF_SAT), counts (cbench_libm erff,
   erfcf rows before/after).
3. Sweep cost (sizing first, stride 64): every taken call is checked against the mp accurate path (~2.3·10^5
   instructions), 2^34 calls per function: on the order of 10^14-10^15 instructions per sweep, far above the inverse
   trig sweeps. RECOMMEND: a test-only oracle that computes the mp value once per x and rounds it in the four modes
   (4x fewer mp calls), or a ruling on a stride for the sweep.

## Not to redo
The trial degree search (above). The fallback design: subnormal erfc results stay on the accurate path.
