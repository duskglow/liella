# PERF-LIBM-INVTRIG: checkpoint (WIP, context limit reached at ~25 calls)

Branch task/perf-libm-invtrig, cut from origin/next 0005889c. NOT VERIFIED: nothing below was run beyond `cargo check -p liella-math` (clean).

## Done
* `crates/math/src/invtrig.rs` (new, `pub mod invtrig` in lib.rs): round-to-nearest fast kernels, each `Option<_>` (None = domain refusal or failed rounding test, caller runs the accurate path):
  `atan32_nearest`, `asin32_nearest`, `acos32_nearest`, `atan2_32_nearest(y, x)` (one f64 evaluation, `fast32::round` with DELTA 24),
  `atan64_nearest`, `asin64_nearest`, `acos64_nearest`, `atan2_64_nearest(y, x)` (double-double reduction, series tail in double, Ziv test with EPS = 2^-62|y|),
  `hypot64_nearest(x, y)` (double-double sum of squares, hardware sqrt, double-double correction; refuses when the root may be exact).
  The error analysis is in the module comment (a proof sketch, not yet checked by a test).
* `fast32::round` made `pub(crate)`.

## Design decisions (all mine, not in a plan row)
* Entry: the existing in-place nearest entry (`mret::elem32_fast` / `mret::elem_fast`, as exp2/log/pow use), not `Real` dispatch. Directed modes and non-default MXCSR keep the current accurate path (unchanged speed, unchanged results).
* Kernels refuse (None) before any floating-point operation that could raise a flag other than inexact: zero, subnormal, tiny (< 2^-60), |x| >= 1 for asin/acos, huge operands. Hardware flag contract = "inexact only".
* Hardware sqrt via `_mm_sqrt_sd` inside the math crate (forbid(unsafe) holds if the compiler accepts it: `cargo check` passed).
* No FMA dispatch yet. Expected floor without it: asin/acos about 2-3x glibc's instruction count (Dekker products); f32 rows and atan expected near 1x. UNMEASURED.

## Next steps (in order)
1. Wire rows in `crates/boundary/src/posix/s10_math.tsv` like `exp2` (split `fdlq` into `f`, `d`, `lq`):
   `asin f` -> `mret::elem32_fast::<{ math::Status::HW_ANY.0 }>({x}, math::invtrig::asin32_nearest, math::real::Real::asin)`;
   `asin d` -> `mret::elem_fast::<{ math::Status::HW_ANY.0 }, _, _>({x}, |x| math::invtrig::asin64_nearest(x), math::real::Real::asin)`; same for acos, atan;
   `atan2 f/d` -> `mret::elem_fast::<HW_ANY, _, _>(({x}, {y}), |(y, x)| math::invtrig::atan2_32_nearest(y, x), |(x, y), m| math::real::Real::atan2(x, y, m))` (first C parameter is the y coordinate; check the f32 tuple has `Denorm`);
   then `cargo xtask math-gen` (never hand-edit s10_math_gen.rs).
2. hypot f64: in `crates/boundary/src/mathret.rs`, `impl Hypot for f64`: read mxcsr once; if `fpu::is_default_but_rounding(mxcsr)` and `fpu::rounding_of(mxcsr) == Rounding::Nearest`, try `invtrig::hypot64_nearest`; on refusal RESTORE mxcsr (`liella_machine_mxcsr_set(mxcsr)`: the attempt's Dekker splits raise inexact even for an exact root such as hypot(3,4)) and run `pow::hypot` as the f32 variant does.
3. Tests (`crates/math/tests/invtrig.rs`): each `*_nearest` equals `trig::{asin,acos,atan,atan2}::<f32|f64>(x, Nearest)` / `pow::hypot` bit for bit when Some; random + boundaries (2^-60, 1-ulp, 1, 2^60, midpoints, hard cases); fallback rate ceiling (expect f32 ~1e-6, f64 ~2^-9 of operands); f32 exhaustive sweep of asin/acos/atan (|x|<=1 ~1.1e9 patterns) as an #[ignore] job; a hypot test for exact roots (3,4) flags.
   Also: flags test at the boundary (only inexact raised), errno unchanged.
4. Jobs on the final head (labels in the brief): perf-libm-invtrig-gate, -plants (break rounding test: EPS/DELTA; range filter: the 2^-60 / <1 bounds; dispatch: the nearest-mode condition in hypot), -accuracy, -counts (`cargo xtask bench --compare --counters --runs 3 cbench_libm`, before/after).
5. If a row misses 1.05x: counted floor + row in docs/saturation/ (method of docs/saturation/16).

## Open questions / risks to check first
* `atan_unit64` assumes `|tl| <= ulp(th)/2` and `th <= 1 + 2^-52` (index 64 of ATAN_J64 exists).
* asin64: `flip = a > s.hi`; confirm t <= 1 + 2^-52 always.
* `settle` copies trig's nearest rounding test; the `need_inexact` extra condition is only for hypot.
* acos at |x| near 1 and asin near 1: check against the oracle for the cancellation-free claim (acos x>0 near 1 uses 2*atan(w), w tiny: fine in theory).
* RULING NEEDED: none yet.
