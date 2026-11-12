# LIBM-F32-REST: f32 inverse trig one-pass (landed), lgammaf edge path (LIBM-F32-REST-NEXT, -2); handoff, one page

State: branch `task/libm-f32-rest-next-2` (= `task/libm-f32-rest-next` plus handoff commits; base origin/next
`c1ae12c2d`), product code unchanged since `722d7775a`; role kanon:~/w/kn1. Runs 1-5 (LIBM-F32-REST..-5, landed)
moved asinf, acosf, atanf, atan2f to the one-pass `float` form (D-10.82, `crates/math/src/fast32/inv_trig.rs`).

## LIBM-F32-REST-NEXT: lgammaf beside lnΓ's negative zeros (D-10.87), done, held for layout
- `#[cold] lgamma_edge` in `crates/math/src/fast32/gamma.rs` (double-double kernel with a proved margin; tiny |x| by
  -ln|x|); test `lgammaf_beside_the_negative_zeros_takes_the_fast_path`; per-call case `lgammaf_zero`. Details and
  proofs in the doc comments and D-10.87.
- Evidence on the same product code (shiki, run -1's jobs): `libm-f32-rest-next-exh4` rc 0 (every float × 4 modes;
  "near-zero refusals 1 operands"), `-plants4` rc 0 (4 CAUGHT, 1 EQUIVALENT), `-percall4` rc 0 (lgammaf_zero 1049 vs
  ≈500k before, glibc 543), `-bench4` rc 0: cbench_libm lgammaf 315.7 instr/call (glibc 277.3; was mean 2257).

## The red gate (gate4: `hot_text_products init_data_clusters_hold_at_every_text_phase`), attributed (-2)
- kanon, same test, same pads: origin/next `c1ae12c2d` tool 54 at all 10 pads, `true` 11, passes (job
  `libm-f32-rest-next-2-layout-next`); this branch tool 55 at 9 pads and 54 at `data 0x3000`, `true` 12, fails
  (`-layout-branch`, reproduces shiki's gate4).
- Cause: `.data1` (the declared start-up state cluster in the single LTO object of libc.a) holds the SAME 153 members
  with the SAME sizes in both builds, but 60 of them sit at other offsets: LLVM emits the statics of the one fat-LTO
  object in an order that any code change in the crate graph can move, and the section's padding grows 0x2c18 ->
  0x2c50 (linked: +0x30), which moves start-up objects across a page boundary. The story touches no start-up code
  or data; this is the #4347 jitter class (code growth elsewhere moves start-up data), not a regression in the path.
  Tools: ~/work/libm-f32-rest-next-2/d1addr.sh on kanon (objdump of `.data1`, offset/size/name); next.addr, branch.addr.
- Fix belongs in the layout lane (LAYOUT-STARTUP-STATE-LOCALE / data clusters): order the `.data1` members
  deterministically (for example by declared-list order or by alignment then name) when `move_startup_state`
  renames them, so their packing no longer follows LLVM's emission order. Not done here (brief: no data moves).

## Booked timing (-2), staged branch head, cbench_libm rows, ns/call median of 5 interleaved runs (best of 7 each)
| row | Ivy Bridge Liella / glibc (ratio) | N100 Liella / glibc (ratio) | Zen 1 instr Liella / glibc (bench4) |
|---|---|---|---|
| asinf | 38.98 / 12.81 (3.04) | 22.14 / 7.71 (2.87) | 72.4 / 53.5 |
| acosf | 38.43 / 15.14 (2.54) | 23.05 / 10.04 (2.30) | 72.3 / 61.2 |
| atanf | 26.84 / 18.36 (1.46) | 16.17 / 12.17 (1.33) | 68.8 / 66.7 |
| atan2f | 33.22 / 37.15 (0.89) | 18.41 / 22.66 (0.81) | 80.4 / 147.8 |
| lgammaf | 135.98 / 78.21 (1.74) | 65.35 / 49.42 (1.32) | 315.7 / 277.3 |
Jobs nico:`libm-f32-rest-next-2-ivb`, nozomi:`libm-f32-rest-next-2-n100` (booked #178/#179, cancelled after).
IVB spread is wide (one run up to 1.6x the median), N100 tight. Target ≤ 1.3x IVB: met only by atan2f.
asinf/acosf are 1.2-1.35x by instructions but 2.3-3x in time: every operand runs a square root and then a division
into `atan_red_any` (both long-latency on these classes); not profiled further here.

## Open / next step
1. Held for the layout fix (above); re-gate after it lands (`cargo xtask affected --run --base origin/next`).
2. New story (recommend, RUT/instruction-mix): asinf/acosf without the division (e.g. a direct polynomial in a for
   |a| ≤ 1/2 and the half-angle form asin a = π/2 - 2·asin √((1-a)/2) above: one root, no divide; Cody & Waite,
   "Software Manual for the Elementary Functions", ch. 10), and atanf's 1/a; time it, since the cost is latency.
3. erff, erfcf, hypotf onto the one-pass form (D-10.67) with exhaustive proofs (hypotf: bound + 10^9 pairs); not
   started. Profile their time first (timing above shows one-pass alone does not decide the time).
4. Optional: x < -42 (non-pole) and the rounding-test refusals in 2^-8 ≤ |y| < 1 still take the 384-bit path
   (#4918, a separate follow-up).
Not to redo: integer kernels for f32 (DESIGN 3.5 B); a partial-write conversion (D-10.67); the round-to-nearest
index `s·32 + 1.5·2^52` in a one-pass kernel (use `atan_red_any`). Plan rows: D-10.13, D-10.63, D-10.67, D-10.82,
D-10.87, D-10.95.
