# PERF-LIBM-HYPERBOLIC: handoff (WIP checkpoint, NOT READY)

State: branch task/perf-libm-hyperbolic, code commits on top of origin/next 0005889c (a6fc34c2 merge base). Role keke:~/w/n1.

Done
- `crates/math/src/fast_hyper.rs`: round-to-nearest in-place entries `{sinh,cosh,tanh,asinh,acosh,atanh}_nearest` (f64) and
  `*f_nearest` (f32), built on `fast64::exp_kernel` / `log_kernel` (made `pub(crate)`), with an absolute error bound (module doc
  has the bounds), `fast64::settle` (f64) or a float midpoint test `settle32` (f32). `None` -> accurate path unchanged.
- Wired through `s10_math.tsv` rows (f, d, lq) and the regenerated `s10_math_gen.rs`.
- `crates/math/tests/fast_hyper.rs`: bitwise equality with `hyper::*` (Nearest) on 40k samples per range (f64) and a stride sweep of
  float patterns (default stride 8191; `LIELLA_HYPER_STRIDE`, `LIELLA_HYPER_N`). Passes (release, keke).
- clippy `--workspace --all-targets --all-features -D warnings`: clean (job lint2 rc 4 was the trace tag, fixed; static then rc 0 inside the counts job).

Counts (keke, cbench_libm --counters --runs 3, instructions:u per op, Liella / glibc, from ~/work/hyper-counts.log on keke)
before (brief, origin/next): 1445-1549 for all twelve rows. After:
sinhf 295.7/145.4 (2.03x), sinh 273.7/144.8 (1.89), coshf 281.6/78.9 (3.57), cosh 261.6/84.5 (3.10), tanhf 398.8/131.3 (3.04),
tanh 257.5/131.7 (1.96), asinhf 399.6/89.0 (4.49), asinh 378.3/99.4 (3.81), acoshf 391.4/73.0 (5.36), acosh 371.3/83.6 (4.44);
atanh/atanhf rows are in the log (not read for lack of budget). Time ratio vs glibc fell from 15-48x to 2.3-12x.
TARGET 1.05x NOT MET on any row. No-row-slower statement for the rest of cbench_libm: NOT VERIFIED (the other rows' code is untouched).

NOT DONE (budget: context hit ~130k of 140k): direct-tier gate, plants (range filter, rounding test, eps), the large accuracy
sweep (stride 1..97 of float, 1M f64) and the saturation row for the floor. Small-|x| ranges fall back to the accurate path by design:
sinh/tanh a < 2^-8, asinh/atanh a < 2^-25, cosh a < 2^-30 (f32: 2^-12); a >= 708 etc. No FMA dispatch variant written.

Next steps (about 25 calls)
1. Read the atanh rows; profile why the instruction counts are 260-400 (est. 150-200): the `fast64` kernels are used non-fused
   (`two_prod` Dekker, no `#[target_feature(fma)]`); an FMA-dispatched copy of fast_hyper (bit-identical results, same `settle`) and
   cheaper `recip`/`div`/`dd::sqrt` (hardware sqrt, 4 Newton steps are ~30 instructions) are the first levers.
2. Run gate (`cargo xtask affected --run`), plants (eps*0.5, inverted `settle` test, domain lower bound, `mid()` test in settle32)
   in /dev/shm copies, the accuracy sweep with LIELLA_HYPER_STRIDE=61, then add docs/saturation row with per-phase floors.
3. Dependency: PERF-LIBM-EXPM1-LOG1P would let sinh/tanh/asinh/atanh cover the small ranges.

Rules in force: D-10.13 (accurate path is the reference), clean room (nothing consulted beyond the repo's own kernels and the brief).
Plan rows to read: D-10.13, D-10.25 (FMA build of fast64).
