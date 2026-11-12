# LIBM-F32-REST: f32 inverse trig one-pass (landed), lgammaf edge path (LIBM-F32-REST-NEXT, -2, -3); handoff, one page

State: branch `task/libm-f32-rest-next-3` (= `task/libm-f32-rest-next-2` + merge of origin/next `d1b3a596c` +
handoff commits); role keke:~/w/n1. Product code: lgammaf's edge path (D-10.87, `crates/math/src/fast32/gamma.rs`),
unchanged since `722d7775a` apart from the merge. Runs 1-5 (LIBM-F32-REST..-5, landed) moved asinf, acosf, atanf,
atan2f to the one-pass `float` form (D-10.82).

## -3: unheld after LAYOUT-STARTUP-STATE-LOCALE-4 landed; merged next, re-gated
- Inherited head `b93f19510`: clippy + static rc 0 (keke `libm-f32-rest-next-3-lint0`).
- Merge of origin/next: one conflict, `crates/math/src/fast32.rs` `eval`: kept the branch's
  `Fast::Lgamma => return eval_lgamma(x, mode).map(|r| r.0)` and took next's `bessel_eval` arms (J0/J1/Y0/Y1).
  Merged head clippy + static rc 0 (`-lint1`).
- Gate `libm-f32-rest-next-3-gate` (`affected --run --base origin/next`, keke) and plants
  `libm-f32-rest-next-3-plants` (handoff/LIBM-F32-REST-NEXT-plants.txt, /dev/shm/claude/LIBM-F32-REST-NEXT-3/plants-1):
  GATE_RESULT.
- lgammaf evidence from -1 still holds (same product code): exhaustive 2^32 x 4 modes rc 0, lgammaf_zero 1049 instr
  vs about 500k before (glibc 543), cbench lgammaf 315.7 (glibc 277.3; was mean 2257). D-10.87.

## Profile of erff, erfcf, hypotf (-3; Zen 1, instructions per call, keke `libm-f32-rest-next-3-prof2`)
Per-call spec `validation/percall/erff.c` (bench rows [-5, 5] plus one case per kernel branch); hypotf from
`validation/percall/hypotf.c`. `wl_percall --base none`; traces by `wl_trace --list` (call 11).
| case | glibc | Liella | ratio | Liella split of one call (trace) |
|---|---:|---:|---:|---|
| erff ([-5,5]) | 139.2 | 155.9 | 1.12 | |
| erff_small (\|x\| < 1) | 59.5 | 125.0 | 2.10 | 115: envelope `mret::elem` 45 + MXCSR read 3 + entry 2 = 50; kernel+round+stats 65 |
| erff_big (1..4) | 160.9 | 180.0 | 1.12 | 170: envelope 50; kernel 120 |
| erfcf ([-5,5]) | 143.0 | 163.8 | 1.15 | (self-check MISMATCH on erfcf(0x1.b12e88p+0): glibc 0x1.11c964p-6, Liella 0x1.11c966p-6; exact 0x1.11c9655...p-6 (pinned for the double row in crates/boundary/tests/erf_quick.rs), so Liella's is the correctly rounded one) |
| erfcf_small | 62.5 | 125.0 | 2.00 | 115: envelope 50; kernel 65 |
| erfcf_pos (1..9) | 167.6 | 187.0 | 1.12 | 177: envelope 50; kernel 127 |
| erfcf_neg (-4..-1) | 164.4 | 182.0 | 1.11 | 172: envelope 50; kernel 122 |
| hypotf | 37.0 | 50.0 | 1.35 | 39: out-of-line `liella_machine_mxcsr_get` 3 + `liella_machine_sqrtsd` 2 + body 34 |
Timing (results/timing-sweep-1110-table.md, Ivy Bridge): erff 1.66x, erfcf 1.80x, hypotf 1.72x glibc in time against
1.28 / 1.33 / 1.37 by instructions: the time gap is larger than the instruction gap, as expected from the MXCSR
store/load (and, for erff/erfcf, the envelope's exit) on every call.
**Verdict: the profile names the entry, not the kernels.** erff/erfcf still go through the full `mret::elem` envelope
(50 of 115 instructions on the |x| < 1 path, more than glibc's whole call); hypotf reads MXCSR through an out-of-line
call before its (already one-pass-shaped) `sqrtsd` body. Changing the polynomials would not reach the gap.

## Design for the follow-up (not started here: budget)
1. `erf_onepass_any` / `erfc_onepass_any` in `crates/math/src/fast32/erf.rs`, exported through
   `mret::elem32_onepass` (s10_math.tsv row, `cargo xtask math-gen`): operand from bits for normal finite x (zero,
   subnormal, inf, NaN refused before any FP op), the existing `erf_small` / `erfc_big` (its `exp_z` is already proved
   in every direction in exp_onepass's proof, |z| < 2^13.95 here), ±ERF_ONE / ERFC_TWO saturation, erfc refused from
   9.1875 (0x4113_0000; subnormal results), margin test + `cvtsd2ss`. Bounds redone with 2^-52 per operation
   (estimate: erf ≤ ~101u -> delta 256; erfc ≤ ~636u (erfc_big's exp part 2^-43.69) -> delta 1024). Proof: add both
   to the ONEPASS table in crates/boundary/tests/s10_math_symbols.rs (entries-match test + two 2^32 x 4-direction
   sweeps). Expected: erff_small about 125 -> ~45 (glibc 59.5), bench rows about -60 each.
2. hypotf: the same contract holds for its body (squares of normal floats are exact normal doubles; a refused sum
   or root inexact implies an inexact result), so it can drop the MXCSR read: refuse NaN/inf and subnormal operands
   on the bits (umax/umin of |bits|), keep the range and midpoint tests, `sqrtsd` inline from `hot` (as asinf), entry
   `elem32_onepass2`. Prove with the 10^9-pair job plus the hypotf sweep already in s10_math_symbols.
3. Book IVB + N100 timing for erff/erfcf/hypotf after 1-2 (the timing gap is the target, ≤ 1.3x IVB).

## Open / other next steps
- asinf/acosf division-free: LIBM-ASINF-NODIV (#4924), not this story.
- Optional: lgammaf x < -42 (non-pole) and the rounding-test refusals in 2^-8 ≤ |y| < 1 still take the 384-bit path (#4918).
Not to redo: integer kernels for f32 (DESIGN 3.5 B); a partial-write conversion (D-10.67); the round-to-nearest
index `s·32 + 1.5·2^52` in a one-pass kernel (use `atan_red_any`); the .data1 attribution (-2; fixed by the layout
lane). Plan rows: D-10.13, D-10.63, D-10.67, D-10.82, D-10.87, D-10.95.
