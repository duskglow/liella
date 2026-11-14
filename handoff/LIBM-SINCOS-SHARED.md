# LIBM-SINCOS-SHARED: sincos/sincosf/sincosl share one reduction

State: branch `task/libm-sincos-shared-2` (LIBM-SINCOS-SHARED-2; continues `task/libm-sincos-shared`, whose code
commits are 3d9f4f9ba + 7c371d328), origin/next merged at 8a2aa414d, D-10.89 corrected at 8272f2828; head = the
handoff commit on top. Role keke `~/w/n2`. Decisions in force: D-10.89 (this story), D-10.91 (binary128 pair), D-10.52,
D-10.67, D-10.79.

## What changed (whole story)

* `sincosf`: in-place fast entry `mret::sincos_fast::<HW_ANY, f32>` -> `hot::sincosf_nearest` ->
  `fast32::sincos_nearest_widen` (one `trig_core_with`, both results rounded by the sinf/cosf test; a refusal goes to
  the cold `fast32::sincos`). Row `sincos f` split out of `sincos flq` in `s10_math.tsv` (math-gen regenerated).
* `double`: the envelope path `trig::sincos` uses `fast::sincos` (one reduce/poly, two assembles) and one
  double-double `sin_cos` for what the fast tier leaves. The in-place tier already shared (`sincos_tiered`).
* x87 `long double`: `wide_int::kernel` = `reduced` + `place`; `sin_cos_pair` reduces once and evaluates both
  series once; `wide::sincos` takes it plus one 384-bit `sin_cos_of` for refusals.
* `_Float128` (this run, from the merge): next brought D-10.91's shared `quad::sin_and_cos` in `wide::sincos`, which
  conflicted with this branch's `wide::sincos`. Resolution: `wide::sincos` keeps this branch's shape and
  `sin_cos_pair` sends the 113-bit format through `quad::sin_and_cos`, falling back to two single `quad::sin_cos`
  where it declines, so every result is still the one sinf128/cosf128 give. D-10.89's text corrected to say so.
* Bound (D-10.89): each result is computed by exactly its single function's operations: same bits and flags as
  sin*/cos*, correctly rounded in every mode.

## Why CI went red, and what this run found

CI red at f2db67c28: `liella::hot_text_products` start-up fault bounds (next sat at the bounds; a libm change moves
liella_boundary's codegen units). Held until the layout work landed (LAYOUT-DATA-CLUSTERS-4C). On the merged head
(8a2aa414d) on keke: `libm-sincos-shared-2-hottext` rc 0, 21/21 tests in both passes (plain and `--with-exports`).
No bound touched.

## Counts (instr per call, cbench_libm, Skylake-SP; kanon jobs `libm-sincos-shared-count0` / `-counts`, pre-merge)

| row | Liella before | Liella after | glibc | single fn alone |
|---|---:|---:|---:|---:|
| sincosf | 251.4 | 115.2 | 57.6 | sinf 85.2 |
| sincos | 188.6 | 188.3 | 151.3 | sin 127.9 |
| sincosl | 1146.0 | 697.0 | 1179.3 | sinl 624.3 |

sincosf and sincosl are now below single + half the other. sincosf is still 2.0x glibc by instructions (sinf
itself is 2.03x: the tier, not the sharing). sincos (double) is 1.24x, the tier itself. Accuracy job
`libm-sincos-shared-accuracy` (exhaustive sincosf vs sinf/cosf, every mode) rc 0 on kanon.

## Tests and plants

`s10_3_c_r4` (sincos vs sin/cos bits and flags, every mode, f32/f64/f128), the boundary test
`math_float_sincos_fast_entry_matches_the_envelope_in_every_mode`, `elem_ld_products`. Plants:
`handoff/LIBM-SINCOS-SHARED-2-plants.txt` (the four earlier ones plus `f128-pair-swapped`), job
`libm-sincos-shared-2-plants`. Gate `libm-sincos-shared-2-gate` (affected --base origin/next), lint
`libm-sincos-shared-2-clippy2` / `-static2`. Results: report `handoff/reports/LIBM-SINCOS-SHARED-2.json`.

## Next step (optional follow-ups, not blocking landing)

1. Booked timing on Ivy Bridge (nico or kotori) and N100 for rows sinf,cosf,sincosf,sin,cos,sincos,sinl,cosl,sincosl
   (`cargo xtask bench --stage`); target <= 1.3x glibc. Not done (the counts show the sharing works).
2. sincosf/sinf at 2x glibc by instructions is the float tier itself: a libm f32 trig tier story, not this one.

Do not redo: the merge, the conflict resolution, the hot_text check.
