# LIBM-DIRECTED-INPLACE(-2, -3): the double exp quick phase in the caller's directed rounding

State: branch task/libm-directed-inplace-3 (-2's branch merged with origin/next at 25b11ee02, after
LAYOUT-DATA-CLUSTERS-4C landed and lifted the hold), role keke:~/w/r5. Decision D-10.94
(records/decisions/D-10/D-10.94.toml). Full report of run -3: handoff/LIBM-DIRECTED-INPLACE-3-report.md.

## Done
- Run 1: `fast64::quick::exp_directed` (both builds), its proof (module doc of
  crates/math/src/fast64/quick.rs, "Directed rounding"; `directed_bounds` test), `hot::exp_directed`,
  `mathret::elem_fast_dir` / cold `elem_directed`, the `exp d` row on elem_fast_dir.
- Run 2: tests in crates/boundary/tests/exp_quick.rs (breakpoints, 30000 x 3 modes x 2 builds, ignored
  10^7 accuracy job), D-10.94; plants 4/4; 10^7 accuracy 0 mismatches in every direction and build.
- Run 3: **the exp_upward stall named and fixed.** `h` was selected from the MXCSR value as data
  (`sete; movsd (table,idx,8)`), so the whole evaluation waited for the MXCSR read, and on the Zen 1
  host that read completes only after the caller's earlier FP work: successive calls could not
  overlap (perf: IPC 0.90, 92% backend-stalled, vs 2.21 in nearest). Now `exp_directed::<UP>` is a
  const generic chosen by a predicted branch (control dependency only): IPC 2.32, and the time
  dropped from 4.8x glibc to 1.2x (table below). Probes ruled out the other suspects: add, multiply and
  FMA cost the same in all four rounding directions on Zen 1 (no directed-mode penalty), and the
  disassembly of the directed path shows no other MXCSR access and no float-to-int conversion.

## Results (keke, Zen 1, UNBOOKED, load ~20: ratios only, not a verdict)
| row | before (meas1) Liella / glibc ns | after (t4, 3 runs) Liella / glibc ns |
| exp_upward | 68.30 / 14.28 | 22.1-22.6 / 18.1-18.5 |
| exp (nearest) | 28.29 / 14.29 | 19.3-19.4 / 18.1-18.3 |
- Probe (hardware, rcprobe.c in the story's work dir): stmxcsr plus a use of its value costs about
  20.5 TSC ticks per iteration on Zen 1 in every mode, whatever FP work sits beside it. Every
  Liella libm entry makes that read once per call, nearest included (see open questions).

## Open questions
- The per-call MXCSR read itself (~20 TSC ticks reciprocal throughput on Zen 1) is a fixed cost of
  every elem_* entry. Whether the env check can be cheaper (or skipped where the result is the same
  in every mode) is a design question for a separate story.
- Booked timing on Ivy Bridge (where the read is microcoded, per LIBM-LD-ARC-RETHINK-2) and N100 not done.

## Next step
1. Booked timing of exp/exp_upward/expf_upward on Ivy Bridge (nico or kotori) and N100 to confirm the
   gain off Zen 1 (target <= 2x glibc).
2. LIBM-DIRECTED-INPLACE-4: log, sin, pow with the same recipe (logf is already one-pass in the
   caller's direction like expf). Keep any direction-dependent constant a const generic picked by a
   branch, never data selected from MXCSR. For log: the reduction (r exact in any direction), th
   exact; Fast2Sum error terms per Boldo-Graillat-Muller; replace `zh + zl·C == zh` by the
   two-sided monotone test; decline x = 1 (ln 1 = +0 in every direction; `x − x` gives −0 downward).
