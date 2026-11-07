# LIBM-F64-POW-QUICK: binary64 pow quick phase (tier 1), wired; INCOMPLETE (jobs pending)

State: branch `task/libm-f64-pow-quick` (stacks on LIBM-F64-LOG-QUICK-2 at 615d6a4f9), code head 665d40bfb.
Role mei:~/w/s3. Brief `/home/claude/state/briefs/LIBM-F64-POW-QUICK.md`. Decisions in force: D-10.51, D-10.54,
D-10.60, D-10.62 (this kernel: composition, run-time bound), D-10.3, D-10.25/D-10.50.

## What is in
* `crates/math/src/fast64/quick_pow.rs`: the quick log with a degree-10 term -> `(lh, ll, r3)`; `y·lh` exact
  (2MultFMA / Dekker+Veltkamp), `tl = y·ll + e`; the quick exp kernel on `th` with `tl` folded into the Cody-Waite
  step; Ziv one-sided test with a per-call multiplier `c = RN(|y·r3|·89/16 + C0)` (bound composed at run time,
  proof in the module doc). Domain: `pow_domain` (exact cases refused first), `2^-60 <= |th| < 708`,
  `|y·r3| <= 2^-12`; otherwise tier 2 = `fast64::pow_nearest` unchanged. Both builds.
* Wired: `crates/boundary/src/hot/exp_log.rs::pow_nearest` -> `quick_pow::{fused::,}pow_tiered`; selftest hook
  `pow_quick`. Visibility only: `quick.rs`/`quick_log.rs` consts `pub(super)`, `reduce`/`half_square`
  `pub(in crate::fast64)`. KERNELS.md regenerated.
* Tests: `crates/math/src/fast64/quick_pow/tests.rs` (5: constants and recomputed bounds checked exactly in Mp,
  log-stage bound, composed error bound, CR match with non-vacuity, exact/special refusals);
  `crates/boundary/tests/pow_quick.rs` (2, clause s10_0_f_0: FMA build vs CR; entry sweep both builds,
  `LIELLA_POW_SWEEP=<n>` per set, 4 sets).

## Results (job `libm-f64-pow-quick-t2`, mei, head 5e87b32 tree = code head minus KERNELS.md)
math 5/5, boundary 2/2, `cargo xtask clippy` rc 0, `cargo xtask static` rc 0 (after kernel-index).
SSE2: fallback 599 of 10^5 on the cbench box (0.60%; model 0.57%), unchecked zh wrong on 160 (all declined),
largest relative error 2^-61.575, log-stage error / bound at most 0.583. FMA: fallback 5 of 5·10^4.

## NOT done (budget/context checkpoint)
* Direct gate `libm-f64-pow-quick-gate` started detached on mei (`cargo xtask affected --base origin/next --run`,
  log ~/work/LIBM-F64-POW-QUICK-gate.log); re-attach `~/bin/jobwait libm-f64-pow-quick-gate`. Result unknown.
* Plants not run. Proposed (file quick_pow.rs, direct tests `-p liella-math --lib quick_pow` + boundary
  `--test pow_quick`): ll dropped (`(lh, (sh - lh) + lo, r3)` -> `(lh, 0.0, r3)`, msg:error .* above|correctly rounded);
  inexact product (`$ma(y, ll, p.lo)` -> `$ma(y, ll, 0.0)`, msg:above rho|correctly rounded); near always true
  (`if zh + zl * c == zh` -> `if true`, msg:correctly rounded); exact detector bypassed (`!pow_domain(x, y)` ->
  `!(x > 0.0 && x != 1.0)`, msg:must refuse the exact case).
* Counts before/after (both builds), wl_trace profile, booked IVB/N100 timing, the 10^8-operand sweep
  (`LIELLA_POW_SWEEP=25000000`), four rounding modes (directed modes do not reach the fast path: elem_fast needs
  the default MXCSR; the sweep checks round-to-nearest only).
