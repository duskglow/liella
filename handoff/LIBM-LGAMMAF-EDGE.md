# LIBM-LGAMMAF-EDGE handoff (run 3, LIBM-LGAMMAF-EDGE-3: INCOMPLETE, one typical row +4 instr)

State: branch `task/libm-lgammaf-edge-3` (continues `task/libm-lgammaf-edge-2`), code head 4c914338c, stacked on
af1c82203 (the merge base with origin/next). Role keke:~/w/n2. Decision: pending record
records/pending/LIBM-LGAMMAF-EDGE-2-1.toml (series D-10; id to be assigned). Spec: ~/state/briefs/LIBM-LGAMMAF-EDGE.md
plus the -2 and -3 briefs. Full report with the percall table per commit: handoff/LIBM-LGAMMAF-EDGE-report.md.

## Done (runs 2 and 3)
- `lgamma_reflect` (crates/math/src/fast32/gamma.rs): lgammaf for -2^23 < x < -42 by reflection in double, proved
  8.2 ulp bound, margin 16, measured worst 3.0 ulp; reached through the cold `lgamma_wide` (gamma_core refused x).
- `lgamma_rest` (cold): the second try (`lgamma_edge`'s kernel for margin 11 264) then the 18-entry generated table
  (lgamma_tab.rs). It recomputes the pieces with `lgamma_core_pieces::<false>` (no `lgamma_edge` kernels).
- Run 3 hot-path fixes (the -2 head made the typical rows 4-13 instr slower): `special::lgamma` calls
  `fast32::lgamma_first` and, on a refusal, ONE cold `lgammaf_after_first` (second try, then lgamma_accurate): two
  calls there made the allocator pin a constant in rbx for the whole function. The x < -42 test moved out of
  `lgamma_edge` into `lgamma_wide`. `eval_lgamma` (tests, eval(Fast::Lgamma)) still makes both tries.
- Percall 4c914338c vs base: pos 323.8 (325.8), neg 371.6 (372.6), r_neg 378.6 (379.6), zero 1060 (1056: +4),
  big 343 (302860), tab 631 (272874), retry 1247 (484530).

## Open / next (exact next step)
1. lgammaf_zero +4 instr/call over base. Check first: how many of the row's 64 operands (bits 0xc0493471 - 32 ..)
   the first try refuses; each one now pays gamma_core + ln64 in `lgamma_rest` before None. If that is it, skip the
   recompute for edge refusals (e.g. a cheap test in `lgammaf_after_first`, or let the refusal carry the piece
   without adding live registers to the answered path: re-measure the closure). Else disassemble
   `special::lgamma<f32>` and `lgamma_edge` against base (scratch script: ~/work/LIBM-LGAMMAF-EDGE-3/dis.sh on keke,
   sections by name; function symbols are stripped). Then percall again; land only at no row slower.
2. Then collect gate/plants on the new head (plants file handoff/LIBM-LGAMMAF-EDGE-2-plants.txt, 9 plants, all
   CAUGHT on 29bc9a5b3).
- Follow-up #5641 (NOT in scope): `lgamma_edge`'s own refusals (608 operands, |x| < 2^-20) still take the accurate
  path; a margin of 5 for the tiny piece would cut 594 to about 40.
Not to redo: the prerequisite check, the error analysis, the enumeration (current for margin 16), the 9 plants.
