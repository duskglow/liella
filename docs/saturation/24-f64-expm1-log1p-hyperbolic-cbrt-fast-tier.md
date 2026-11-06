## 24. f64 expm1/log1p/sinh/cosh/tanh/cbrt fast tier: counts and the residual over glibc (LIBM-F64-EXPM1-HYPER-CBRT-TIER-2/-3, 2026-11-05, counted)

Code: the round-to-nearest fast tier in `crates/math/src/fast64/exp_log.rs` (expm1, log1p, cbrt) and
`crates/math/src/fast64/hyperbolic.rs` (sinh, cosh, tanh), on the shared `fast64` exp and log kernels (Ziv's first
step, double-double, correctly rounded or refused; D-10.13), wired in `crates/boundary/src/hot.rs`. Zen 1 host
(EPYC 7551), `cargo xtask bench --compare --counters --runs 3 cbench_libm`, instructions per call.
Before = the accurate path (origin/next c6e5db9d); after = the tier (LIBM-F64-EXPM1-HYPER-CBRT-TIER-2 final head).

| Row | before | after | glibc | ratio |
|---|---:|---:|---:|---:|
| expm1 | 928.5 | 156.4 | 86.4 | 1.81 |
| log1p | 894.9 | 204.5 | 83.5 | 2.45 |
| cbrt | 635.9 | 141.5 | 148.4 | 0.95 |
| sinh | 1105.2 | 274.6 | 144.8 | 1.90 |
| cosh | 1084.7 | 260.5 | 84.5 | 3.08 |
| tanh | 1105.3 | 294.7 | 131.6 | 2.24 |

The f32 rows did not change (expm1f 161, log1pf 174, cbrtf 162, sinhf 97, coshf 76, tanhf 100).

* **POSITIVE, kept:** every row is 3.7-6.5x fewer instructions than the accurate path; cbrt is at parity by
  instructions (0.95).
* **Residual, not closed (no per-phase profile yet):** expm1 1.8x, log1p 2.5x, sinh 1.9x, cosh 3.1x, tanh 2.2x glibc.
  Candidates from the code, unmeasured: sinh/cosh/tanh call the exp kernel twice (`e^a` and `e^-a`, `exp_pm`) where
  one table lookup and one polynomial could give both; every entry rounds through `fast64::settle` with an explicit
  error bound where the cheaper `near` test of the in-place exp/log entries would do; log1p forms `w.lo/w.hi` with a
  division. The next story profiles one call per row (validation/wl_trace.sh) before choosing.
* **The real error is far below the proved bounds (about 2^-70 relative):** with every kernel path's bound set to
  zero, a search of 1.6·10^9 uniform operands per range misrounded 3 to 34 times per range (expm1 in [0.0157, 1]
  34, log1p 3 and 7, cosh 11, e^a/2 above 40 7, tanh 4), 10^8 operands gave 1 for sinh in [0.0157, 40] and none for
  cbrt. Random test sweeps therefore never test the bounds; the hard cases found are kept in `fast64/tier_tests.rs`
  for the bound plants.
