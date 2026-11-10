# LIBM-F64-INVHYP-QUICK: double asinh/acosh/atanh quick tier (part 1 done and merged with next; part 2 not started)

State: branch `task/libm-f64-invhyp-quick-2` (LIBM-F64-INVHYP-QUICK-2; continues `task/libm-f64-invhyp-quick`,
origin/next 2f72b2638 merged in by 81c44bc5b), role mei:~/w/s2. Briefs `~/state/briefs/LIBM-F64-INVHYP-QUICK.md`
(the spec) and `-2.md`. Decisions in force: D-10.51 (two-phase double), D-10.60 (quick log kernel), D-10.65 (this
story; now `records/decisions/D-10/D-10.65.toml`), D-10.66 (log2/log10/log1p on the same kernel), D-10.25/D-10.50
(two builds), D-10.3 (correct rounding).

## What is in (unchanged from the first run unless noted)

* `crates/math/src/fast64/quick_log.rs`: pair entries `log_pair_parts(xh, xl)`, `log1p_pair_parts(yh, yl)`, stated
  `RHO_PAIR = 2^-63`, `C_PAIR = 0x3ff0_0804_0201_0081`. **Merge (this run):** adapted to next's kernel macro
  (`$d`/`$tf`/`$i` attribute groups) instead of pasted: `pair_tail` is now `join::<true>` with `dt = d·(1 − r + r²)` as
  the extra term (`$ma(d, $ma(r, r, -r), d)`, as D-10.66's `log1p` correction), and `log_pair_parts` takes `r, th, tl`
  and `bits(c·2^-e)` from `split` (`d = RN(xl·(c·2^-e))`: the same single rounding as before; `c·2^-e` normal for `e` in
  `[-1000, 1001]`, which the callers' range gives). Bound re-checked against the merged kernel: `RHO`, `C`, the
  coefficients, the tables and the documented case bounds A/B/C are byte-identical before and after the merge; the FMA
  build's `r² − r` is now one rounding instead of two (smaller error, inside the `2^-110` term). Bound unchanged.
* `crates/math/src/fast64/quick_invhyp.rs`, `crates/boundary/src/hot/hyperbolic.rs` wiring, `s10_math.tsv` rows,
  selftest hooks (`asinh_quick`, `acosh_quick`, `atanh_quick`, kept beside next's `log_family_*`), tests
  `crates/boundary/tests/invhyp_quick.rs` (clause s10_0_f_0): untouched by the merge.
* Generated views: KERNELS.md regenerated (`cargo xtask kernel-index`; static's tables-fresh needs it);
  `plan/07-decisions/s10-mathematics.md` taken from next (the train regenerates it from the D-10.65 record).

## Jobs (mei)

* Step 0, inherited head 62f7a925f: `libm-f64-invhyp-quick-2-lint0` (clippy + static) rc 0.
* `-lint3` (head 3777b1afc): records check, clippy, static, invhyp_quick: rc 0.
* `-plants` (head 73e39d064; code identical to the final head, which adds only KERNELS.md and handoff/):
  7 of 7 CAUGHT, no WARN (`handoff/LIBM-F64-INVHYP-QUICK-2-plants.txt`; `pair-low-part-dropped` follows the merged
  code, `pair-scale-dropped` is new for the `c·2^-e` scale).
* `-gate` (direct tier, `cargo xtask affected --run`, final code) and `-counts2` (`bench --compare --counters
  cbench_libm --rows asinh,acosh,atanh`, from a RAM copy): see the report json / hand-back for their state.
* Superseded: `-lint1` (rustfmt check of next's code, not enforced; records: bare `|` in D-10.65, fixed),
  `-lint2` (static tables-fresh: KERNELS.md, fixed), `-counts` (bad argument order, rc 1), `-kidx` (generator only).
* First run, for reference (Zen 1, FMA build, instructions Liella vs glibc): asinh 139.3 vs 99.5, acosh 131.5 vs 83.7,
  atanh 141.2 vs 112.8 (before the story ~1000). `-FMA` tunable did not reach cbench (log says "unset"): SSE2-build
  counts NOT VERIFIED.

## Next steps

1. If `-counts2` shows a row moved by more than noise from the numbers above, look at `log_pair_parts` (the `split`
   reuse) first. Booked timing (Ivy Bridge, N100) not run: target <= 2x time.
2. `sqrt_pair`'s division `e/(2·sh)` needs only `2^-13` relative accuracy; an `f32` division or reciprocal estimate
   would do (not tried).
3. Part 2 (separate story): asin/acos/atan quick phase, cosh/sinh/tanh on the exp quick kernel, then cbrt/hypot.

## Not to redo

The pair-entry bound (case A needs each caller's `E1 + E2`), the `2^-200`/`2^1000` ranges, the large-`x` cut at `2^32`,
the merge resolution above.
