# LIBM-F64-INVHYP-QUICK: double asinh/acosh/atanh quick tier (part 1 done; part 2 not started)

State: branch `task/libm-f64-invhyp-quick` (stacks on LIBM-F64-LOG-QUICK(-2), cut from origin/next), role mei:~/w/s4.
Code commit 2250341f6, kernel index 16a511270, test fix 0cb8a104b; handoff commits on top. Brief
`/home/claude/state/briefs/LIBM-F64-INVHYP-QUICK.md`. Decisions in force: D-10.51 (two-phase double), D-10.60 (the
quick log kernel), D-10.65 (this story: method, ranges, bound), D-10.25/D-10.50 (two builds), D-10.3 (correct rounding).

## What is in

* `crates/math/src/fast64/quick_log.rs`: pair entries `log_pair_parts(xh, xl)` and `log1p_pair_parts(yh, yl)` on the
  D-10.60 kernel (both builds through its macro). The low part `d = RN(c·xl·2^-e)` is folded in as `d·(1 − r + r²)`;
  `log1p` with `yh < 2^-7` passes `(yh, yl)` straight as the reduced argument (`c = 1`, `e = 0`), else 2Sum of `1 + yh`.
  Stated `RHO_PAIR = 2^-63`, `C_PAIR = 0x3ff0_0804_0201_0081` (derivation in the module doc, "Pair arguments").
  `log_quick_parts`/`log_quick`/`log_tiered` are unchanged.
* `crates/math/src/fast64/quick_invhyp.rs`: `asinh_quick`, `acosh_quick`, `atanh_quick` (SSE2 build and `fused`): the
  logarithm's argument as a pair within `2^-98` (sqrtsd + one Newton correction with an exact square; atanh one
  division and the remainder by an exact product), then the pair entries and Ziv's test with `C_PAIR`. Ranges
  `2^-200 <= |x| < 2^1000` (acosh `1 < x < 2^1000`, `acosh(1) = +0` exact), else `None`. Bound and flag argument in
  the module doc.
* Wiring: `crates/boundary/src/hot/hyperbolic.rs` `asinh_nearest`/`acosh_nearest`/`atanh_nearest` (FMA build by
  `dispatch::fma`, `sqrtsd` inline); `s10_math.tsv` rows `asinh`/`acosh`/`atanh` `d` now `mret::elem_fast` (tier 2 =
  the accurate path `hyper`, unchanged; `lq` rows unchanged); `s10_math_gen.rs` regenerated (`cargo xtask math-gen`;
  C adapters unchanged). Selftest hooks `asinh_quick`, `acosh_quick`, `atanh_quick`.
* Tests (clause s10_0_f_0): `crates/boundary/tests/invhyp_quick.rs`: both builds, quick phase and entry correctly
  rounded against the accurate path over regions next to the singular points and regime boundaries
  (`LIELLA_INVHYP_N` per region, default 20000), fallback rate between 0.05% and 0.6%; ranges; `C_PAIR` exact check.
  Measured fallback (mei, both builds alike): asinh 250/180000 (0.14%), acosh 254/120000 (0.21%), atanh 145/120000
  (0.12%); model 0.196%.

## Jobs (mei)

* `libm-f64-invhyp-quick-lint3` (rc 0, head 2250341f6 + kernel index): kernel-index, static, the new test.
  Full-workspace clippy passed in `-lint2` (its static step failed on the stale KERNELS.md, fixed by 16a511270).
* `libm-f64-invhyp-quick-plants` (head 16a511270, rc 1): 5 of 6 CAUGHT (sqrt-low-dropped, atanh-division-error-dropped,
  large-regime-boundary-moved, log1p-small-boundary-moved, pair-low-part-dropped); near-always-true SURVIVED: the
  actual error is far below RHO_PAIR, so bypassing the test rarely misrounds a random operand. Fixed by 0cb8a104b (the
  fallback rate must stay above 0.05%: Ziv's window is `C_PAIR − 1` wide whatever the error). Rerun on the final
  head: `libm-f64-invhyp-quick-plants2` rc 0, 6 of 6 CAUGHT, no WARN.
* `libm-f64-invhyp-quick-gate` (head 16a511270): stopped in liella-math (load 45) after liella, boundary (both
  modes) and legacy passed, to rerun on the final head. Final-head jobs:
  `-gate2`, `-plants2`, `-counts2` (cbench_libm `--compare --counters`, default build then
  `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`; whether that tunable reaches cbench is NOT VERIFIED, as LIBM-F64-LOG-QUICK
  noted).
* Superseded: `-build1` (first compile, clippy range lints), `-lint` (script aborted on an empty grep), `-lint2`
  (static: stale KERNELS.md), `-counts` (stopped while queued, to free the role for the final head).

## Not done (next steps)

1. Counts: read `-counts2` (acosh/asinh/atanh rows, Liella vs glibc instructions; before, keke FMA: acosh 984 vs 86,
   asinh 1039 vs 102, atanh 1054 vs 115). Booked timing on Ivy Bridge and N100 not run (budget): stage with
   `cargo xtask bench --stage`, book kotori/nico and nozomi, run cbench_libm `--compare`. Target <= 2x time.
2. If the counts show the division in `sqrt_pair` (`e/(2·sh)`) on the critical path: `sl` needs only `2^-13`
   relative accuracy (its weight is `2^-53·sh`), so an `f32` division or a reciprocal estimate would do; not tried.
3. Part 2 (separate story): asin/acos/atan quick phase (Tang-style atan table + odd minimax, asin via
   `x + x³·P(x²)` below 0.5 and `2·asin(sqrt((1 − x)/2))` above), cosh/sinh/tanh on the exp quick kernel, then
   cbrt/hypot. The pair entries here (`log1p_pair_parts`) are reusable for a quick `log1p` (`yl = 0`, plus negative `y`
   support: today `y >= 0` only).

## Not to redo

The pair-entry bound (case A needs each caller's `E1 + E2`, stated per function in quick_invhyp's doc), the
`2^-200`/`2^1000` ranges (no spurious underflow/overflow), the large-`x` cut at `2^32` (`ln 2x`, `2^-70.5`).
