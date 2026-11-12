# LIBM-TINY-FASTPATHS (-1, -2): handoff + report

State: branch task/libm-tiny-fastpaths-2 (stacked on -1's task/libm-tiny-fastpaths), code head 81e094df1. Two
families of six done. Decisions in force: D-10.108 (exp family tiny, amends D-10.63) and the pending record
records/pending/LIBM-TINY-FASTPATHS-2-1.toml (D-NEW, series D-10: f32 trig huge operands; the coordinator assigns
the id). -2 stopped at its context budget (180k).

## Done 1 (-1): expf / exp2f / exp10f, normal |x| < 2^-27 (D-10.108)

The one-pass entry answers 1 + x rounded in the caller's direction (proof in the code and the record). Probe
(validation/libm_tail, x32 = every float, mei, instructions per call Liella/glibc), final code bbf6d8488 (probe2):
tiny band 9 (exp10f 13) vs 26-32 (was 233-928); kernel range back at 46/46/50; mean expf 119.8 vs 27.2 (was
240.5). Exhaustive sweeps3 (all floats x 4 directions x 3 functions) rc 0; gate3, plants3 (2 CAUGHT) rc 0.

## Done 2 (-2): sinf / cosf / tanf / sincosf, 2^19 <= |x| < inf (pending D-NEW)

Traced from code: `trig_core_with` refused |x| >= 2^19 at its range test, so every huge operand went through the
envelope to `crate::trig` (double-double Payne-Hanek and series): 1327-1474 vs glibc 60-124 (LIBM-TAIL-PROBE).
Now `reduce_large` (fast32/trig.rs) does Payne-Hanek in fixed point: TRIG_LARGE[E-146] = floor(2^(E-23)/pi) mod
2^128 (109 x u128, const-built from 256 bits of 2/pi pinned to TWO_OVER_PI), P = m*T mod 2^128 = x*64/pi mod 128
(121 fraction bits), n = rounded top 7 bits, the fraction normalised by its high word's leading zeros, times
pi/4 in Q63 in integers, one cvtsi2sd: |dr| <= 1.01*2^-53|r| (proof at reduce_large), inside the fast range's
2*2^-53|r|, so TRIG_BOUND/TAN_BOUND/TRIG_DELTA/TAN_DELTA are unchanged. |f| < 2^-31 refused. The kernel
evaluation is shared (`sin_cos_at`); `trig_core_wide(_with)` serve sin/cos/tan/sincos (hot entries and `eval`,
`eval_nearest`, `sincos`); `trig_core` (fast range only) stays for cexpf and Bessel, whose proofs state it.
Tests: trig_large_table_is_two_over_pi, trig_core_wide_is_within_its_bound_above_2_19 (both pass on kanon).

Jobs on kanon (role kn1), head 81e094df1: see "Results" below.

## Results (-2)

All on kanon, role kn1, code head 81e094df1:
* libm-tiny-fastpaths-2-lint0 (inherited head 8c79dfacd: clippy + static) rc 0.
* libm-tiny-fastpaths-2-t1 (fast32::tests::trig, 5 passed; cargo xtask clippy) rc 0.
* libm-tiny-fastpaths-2-gate (affected --run --base origin/next) rc 0, "affected: ok", peak 4.5G.
* libm-tiny-fastpaths-2-plants (handoff/LIBM-TINY-FASTPATHS-2-plants.txt): 5 CAUGHT, 0 SURVIVED, 0 WARN.
* libm-tiny-fastpaths-2-sweeps ({sinf,cosf,tanf}_fast_path_agrees_exhaustively, every float x 4 directions) and
  libm-tiny-fastpaths-2-probe (x32 sinf/cosf/tanf, output ~/work/LIBM-TINY-FASTPATHS-2/probe-after on kanon) were
  still running at hand-back. NOT VERIFIED by this run: the probe's after-figures. The continuation reads them
  against LIBM-TAIL-PROBE's binades-f32.tsv: the band >= 2^19 (expected near 80-100, was 1327-1474) and the kernel
  range (must not be worse).

## Not done (next, in order of cost)

1. f32 trig tiny: sin/cos/tan/sincos below the rounding test's reach (sinf 2^-26..2^-22.6, cosf/sincosf
   2^-26..2^-23.3 = sincosf [1.49e-8, 9.48e-8], 2377 vs ~55; and |x| < 2^-26, 1088-2304): in the RN entries answer
   x (sin, tan) or 1 (cos) directly with inexact (sin x = x(1 - x^2/6...) lies strictly inside (x - ulp/2, x) for
   |x| < 2^-12; cos x inside (1 - 2^-25, 1)); normal x only (subnormals keep the envelope: underflow).
2. Exp family huge |x| (>= 1024: 220 vs 25; 128..1023: 655-1409): overflow/underflow flags and errno, a dedicated
   cold path cheaper than `elem`.
3. asinf |x| < 1.57e-7 (x + x^3/6), log1pf tiny (x - x^2/2), expm1f (x + x^2/2; x <= -18 -> -1 + tiny); log1pf and
   expm1f have no one-pass entry (130-141 typical vs 20-81).
4. expf/exp2f 2^-27 <= |x| < 2^-26 (kernel could start at 2^-27).

Probe usage (tool only on origin/task/libm-tail-probe): copy validation/libm_tail/ to ~/work/<story>/, then
`cargo xtask so && bash run.sh <tree> <outdir> x32:sinf:1024 ...`. Gate and probe both build the products: run them
in sequence in one role (seq.sh pattern), never concurrently.
