# LIBM-F32-HYPERBOLIC: handoff (float hyperbolic family on the fast32 pattern)

State: branch `task/libm-f32-hyperbolic` (stacks on `task/libm-f32-double-kernels-2`), code head `872c0c88`;
this note and `handoff/reports/LIBM-F32-HYPERBOLIC.json` are committed on top (handoff/ only). Role shiki:~/w/sh1.

## Done
* `crates/math/src/fast32.rs`: `sinh_core`, `cosh_core`, `tanh_core`, `asinh_core`, `acosh_core`, `atanh_core`,
  `Fast::{Sinh..Atanh}`, `fast_fn!` entries falling back to `crate::hyper`; `real.rs` routes `f32` to fast32.
  - sinh = (E + E/(E+1))/2, tanh = E/(E+2) with E from `expm1_core` (bounds 12.2u, 11.2u); cosh = (E + 1/E)/2
    from `exp_core` (2^-45.14, margin 512 as for expf).
  - asinh, acosh, atanh: new shared `log1p64(w)` (ln(1+w) for a double w in [2^-26, 2^130], 8.2u; z in [1, 1.5)
    uses an exact r = L·w + (L-1) with w split at 28 bits) plus `sqrt64` (doc domain widened to [2^-60, 2^260],
    tested). Bounds 11.5u, 12.3u, 9.3u. Proofs in the doc comments.
  - tanh for finite |x| >= 9.5 returns y = 1 - 2^-30 (tanh and y share one rounding interval (1 - 2^-25, 1)).
  - Small-|x| cut 2^-26 (HYP_MIN) for the whole family; overflow ranges go to the accurate path.
* No new polynomial or table, so no Remez fit (D-10.3 rule vacuous here): log1p64 reuses log1p's D3..D8 and
  LOG_INV/LOG_C, the exps reuse their existing coefficients.
* Tests (fast32::tests): `hyperbolic_margins`, `hyperbolic_cores_are_within_their_bounds` (double-double
  references from exp_dd/expm1_small/log1p_dd/dd::sqrt; log1p64 and sqrt64 on their own), the six rows in
  `fast_path_agrees_with_the_accurate_path` (bench ranges, fallback <= 1e-4), and ignored
  `<f>_fast_path_agrees_exhaustively` per function, each its own job. Local run on the role: 15 passed.

## Counts (instructions per call, Liella / glibc, cbench_libm, shiki)
| row | before (`-before`, base 617417a6) | after (`-after`) |
|---|---|---|
| sinhf | 1520.1 / 145.4 | see report |
| coshf | 1505.0 / 78.8 | see report |
| tanhf | 1520.3 / 131.2 | see report |
| asinhf | 1482.4 / 88.9 | see report |
| acoshf | 1444.4 / 72.9 | see report |
| atanhf | 1494.3 / 115.7 | see report |

## Jobs (shiki) on 872c0c88
`libm-f32-hyperbolic-gate` (direct tier, --base origin/next), `-plants` (5 plants via `cargo xtask plants`,
file ~/work/LIBM-F32-HYPERBOLIC/plants.txt), `-after` (counts), `-sweep-{sinhf,coshf,tanhf,asinhf,acoshf,atanhf}`
(run in sequence by ~/work/LIBM-F32-HYPERBOLIC/chain.sh after gate and counts; one build in the role at a time).

## Open / next
* Status of the jobs at hand-back is in the report json; the dispatcher reads them from jobs.tsv.
* The 1.05x target: expected not met for the sqrt rows (sqrt64 ~22 instructions); if the after-counts show
  sqrt dominating asinhf/acoshf, ruling #2783's `_mm_sqrt_sd` is the next lever.
* Plan rows: ruling #2760 (A), #2783, #2784; handoff/libm-f32-double-kernels.md PATTERN.
