# LIBM-F32-REST: one-pass asinf, acosf, atanf, atan2f (handoff, one page)

State: branch `task/libm-f32-rest` (from origin/next), role mei:~/w/s1. Done in this run: asinf, acosf, atanf and atan2f moved
to the one-pass `float` form (D-10.82). Not started: lgammaf (latency first), erff/erfcf/hypotf. Those, and the booked timing,
go to the continuation **LIBM-F32-REST-2**.

## What changed
- `crates/math/src/fast32/inv_trig.rs`: `atan_red_any` (the table reduction with an index that does not depend on the
  rounding direction: s·32 rounded to the 2^-7 grid by +1.5·2^45, nearest integer taken on the bits; c = i/32 rebuilt via
  the 1.5·2^52 shift, no `cvtsi2sd`), `onepass_abs` (positive normal `float` bits -> `double`, no `cvtss2sd`),
  `onepass_finish` (margin 64 on the low 28 bits, then one `cvtsd2ss`), and `asin_/acos_/atan_/atan2_onepass_any`. The
  bound in every direction (≤ 21u) is in the doc comments. The old `*_core` kernels stay (complex/other callers, `eval`).
- `crates/boundary/src/mathret.rs`: `elem32_onepass2` (two operands; every refusal takes the full envelope).
- `crates/boundary/src/hot/f32_invtrig.rs`: `asinf_onepass`/`acosf_onepass` pass `sqrtsd` (correctly rounded in the
  caller's direction, full-register write).
- `s10_math.tsv` rows f for asin/acos/atan/atan2 (+ regenerated `s10_math_gen.rs`; `cargo xtask math-gen`).
- Tests (`crates/boundary/tests/s10_math_symbols.rs`): asinf/acosf/atanf join `ONEPASS` (the sampled 16-MXCSR test in the
  gate gets new edge operands), `math_{asinf,acosf,atanf}_onepass_sweep` (ignored; 2^32 × 4 directions),
  `math_atan2f_onepass_matches_the_envelope` (gate: 2^15 pairs + 144 special pairs × 16 MXCSR settings) and
  `math_atan2f_onepass_pairs` (ignored; 2.5·10^8 pairs per direction = 10^9).
- `validation/percall/math_f32_invtrig.c`: per-call spec for the four rows.

## Jobs (mei; re-attach `~/bin/jobwait <label>`)
- `f32rest-exh` (`~/work/LIBM-F32-REST/exh.sh`): the three exhaustive sweeps and the 10^9-pair atan2f job; `EXH` lines.
- `f32rest-gate`: `cargo xtask affected --run --base origin/next`.
- `f32rest-plants`: `cargo xtask plants handoff/LIBM-F32-REST-plants.txt` (7 plants: reduction low part dropped, margin
  test skipped (atanf sweep, stride 3), sign applied after the conversion in asinf/atanf/atan2f, acosf's x < 0 branch,
  `cvtss2sd` reintroduced in asinf -> `cargo xtask partial-write`).
- `f32rest-percall`: `wl_percall.sh --spec validation/percall/math_f32_invtrig.c` (glibc / base / new instructions).
- Earlier, green: `f32rest-check1` (`cargo xtask static` rc 0, `cargo xtask partial-write` rc 0, the sampled one-pass
  tests 2 passed), `f32rest-gen1` (math-gen + clippy clean), `f32rest-rec2` (records check ok).
Results of the four jobs above were not in when this note was written: see the hand-back / report json.

## Not to redo
Integer kernels for f32 (DESIGN 3.5 B, no gain); a partial-write conversion (D-10.67). The round-to-nearest index trick
`s·32 + 1.5·2^52` is wrong under directed rounding for a one-pass kernel (floor/ceil, |s - c| up to 1/32): use
`atan_red_any`.

## Next step (LIBM-F32-REST-2)
1. Booked timing (Ivy Bridge: nico or kotori, the less busy; N100) of asinf, acosf, atanf, atan2f, Liella vs glibc (the
   change alters the instruction mix: `sqrtsd` replaces the software root, so timing is required in-story). Target ≤ 1.3x IVB.
2. lgammaf: measure the latency form first (9.18x time at 1.04x instructions). 3. erff/erfcf/hypotf one-pass.
Plan rows: D-10.63, D-10.67, D-10.72, D-10.82.
