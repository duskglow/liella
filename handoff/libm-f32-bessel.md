# LIBM-F32-BESSEL: handoff (j0f, j1f, y0f, y1f fast path) — INCOMPLETE, checkpoint at the context budget

State: branch `task/libm-f32-bessel` (stacks on `task/libm-f32-double-kernels-2`), role keke:~/w/n2.
Commits: `071d09ba` (Remez tool: Bessel targets + `centers` key + 8 fit files), `b28fdba5` (WIP kernels,
NOT BUILT: the 8 `crates/math/src/coeffs/fast32_{j0,j1,y0,y1}{,_small,_reg}.rs` files do not exist yet).
Nothing was compiled or tested in this run (no local cargo; the one remote job was the Remez run).

## Design (D-NEW placeholder: record as a D-10.x row; no id was reserved)
* Pieces: |x| in [2^-20, 2): J0 = even fit of J0 (relative), J1 = x·(even fit of J1/x); Y0 = (2/π)ln(x)J0 +
  R0(x) and Y1 = -2/(πx) + (2/π)ln(x)J1 + R1(x), R0/R1 the regular parts (fits `y0_reg`/`y1_reg`, absolute),
  ln from `log_core`. [2, 24): eleven pieces [c-1, c+1), c = 3..23 odd, one degree-16 polynomial in t = x - c
  each (t exact). [24, 2^19): Hankel's form, P and Q truncated after a_20/a_21 (first neglected term
  < 2^-59 at x = 24, DLMF 10.17(iii)), coefficients from the recurrence in a `const fn`, sin/cos from
  `trig_core`, amplitude via `sqrt64`. Outside: accurate path.
* Deviation from the brief, for a ruling or a D-row: no table of zeros. Every core returns an ABSOLUTE bound e
  and `bessel_finish` uses margin e·2^53/|y| per call; near a zero the margin grows and the rounding test
  refuses (accurate path). The brief's "zero-table entry" plant becomes a "piece-table entry" plant (e.g. a
  center in `fast32_j0.fit`/a row of `FAST32_J0`), plus the switch point `BES_HANKEL`.
* Wiring: `special::{j0,j1,y0,y1}` try `fast32::eval(Fast::J0..)` when `F::PREC == 24`; `*_accurate` (cfg(test))
  are the references for the tests.

## Remez results so far (keke job `libm-f32-bessel-remez`, stopped by me at hand-back before the centered fits ended)
j0_small 2^-57.31 rel, j1_small 2^-56.92 rel, y0_reg 2^-53.85 abs, y1_reg 2^-56.77 abs; levelled errors
2^-80..-84: the degrees (even 0..22) are FAR too high. Each small fit took about 1 minute; the 44 centered fits
did not finish in about 8 minutes.

## Next step (in order)
1. Lower the degrees (try even 0..16 for the small fits, powers 0..13 for the centered ones; the target is
   rounded error ≤ ~2^-55) in `crates/math/fits/fast32_*.fit`, then run `cargo xtask remez <the 8 fits>` as a
   detached jobrun (it blocks: start it with setsid/nohup or from the role, not a foreground ssh), and copy the 8
   generated files back into the worktree. Then update the `[u64; 12]`/`[[u64; 17]; 11]` types in fast32.rs.
2. Build + clippy on n2. Known items to check: `hankel_coeffs` const float arithmetic; clippy on `!(d < ...)`
   (use `d.is_nan() || d >= ...`, both refuse).
3. Tests (in `fast32::tests`): `bessel_bounds_hold` (from the coefficients: γ_2n·Σ|a_i||t|^i + fit ≤ the
   BES_*_E constants; mp `bessel_j`/`bessel_y` at sample points: |y - f| ≤ e); `hankel_coefficients_are_the_series`;
   the rows in `fast_path_agrees_with_the_accurate_path`; four `#[ignore]` exhaustive tests, ONE job each (size
   first with LIELLA_SWEEP_STRIDE=64: the accurate Bessel path may be slow).
4. Plants via `cargo xtask plants`: a bound constant, a coefficient, a piece-table entry, BES_HANKEL. Gate
   (direct tier, --base the stack base), counts `cbench_libm` j0f/j1f/y0f/y1f before/after.

## Not to redo
The Remez tool extension (targets j0, j1, j1_over_x, y0, y1, y0_reg, y1_reg; `centers = a, b, ...` writes a
`[[_; n]; k]` table, one fit per center) is done; `y_reg` uses 96 guard bits for the cancellation near 2^-20.
