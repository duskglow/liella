# LIBM-LGAMMAF-EDGE-3 report

Branch `task/libm-lgammaf-edge-3` (continues -2), role keke:~/w/n2, base (merge base with origin/next) af1c82203.

## Why the -2 run went red
- `libm-lgammaf-edge-2-percall` (shiki) rc 2: the base 24a434a1 was not primed on shiki (setup; the coordinator fixed
  sync_role). `libm-lgammaf-edge-2-gate` and `-plants` on shiki had no jobs.tsv row: their logs stop part-way (cut
  off). The sweep (`libm-lgammaf-edge-2-sweep`, exhaustive lgammaf + table enumeration) ended rc 0 on code 1d1a7e435.

## Run 3 commits (each with a percall against base af1c82203; instructions:u per call, keke)
| head | change | pos | zero | neg | r_neg | big | tab | retry |
|---|---|---|---|---|---|---|---|---|
| base af1c82203 | - | 325.8 | 1056.0 | 372.6 | 379.6 | 302859.8 | 272874.4 | 484529.8 |
| 29bc9a5b3 (-2 head) | - | 330.1 | 1069.0 | 378.7 | 385.7 | 358.0 | 445.9 | 1072.0 |
| 7307d2897 | lgamma_rest(x, mode) recomputes | 332.8 | 1071.0 | 380.7 | 387.7 | 360.0 | 661.4 | 1257.0 |
| fa82e6a4b | one cold call on special::lgamma's refusal path | 323.8 | 1062.0 | 371.7 | 378.7 | 351.0 | 671.4 | 1267.0 |
| ac391318c | reflection test out of lgamma_edge (lgamma_wide) | 323.8 | 1061.0 | 371.6 | 378.6 | 342.0 | 654.4 | 1265.0 |
| 4c914338c | second try without lgamma_edge's kernels | 323.8 | 1060.0 | 371.6 | 378.6 | 343.0 | 631.4 | 1247.0 |

OPEN: lgammaf_zero is still 4 instructions per call above base (1060 vs 1056, 0.4%); every other typical row is at or
below base. Not profiled (budget). Hypothesis to check first: the row's operands that lgamma_edge's own piece refuses
still pay gamma_core + ln64 in lgamma_rest (lgamma_core_pieces::<false>) before None; count the row's refusals
(64 operands from bits 0xc0493471 - 32) and disassemble lgammaf_after_first / lgamma_rest vs base.

glibc (dynamic): pos 138.4, zero 543.0, neg 394.7, r_neg 398.7, big 280.5, tab 288.5, retry 266.4.
Profile (disassembly of special::lgamma's float body, base vs fa82e6a's parent): with two calls on the refusal path
the allocator kept the abs mask in rbx for the whole function and moved the result pointer to r12 (push/pop r12, a
movabs on every call). The zero row's last +5: refusals of lgamma_edge's own pieces were recomputed through the
double-double kernel by lgamma_rest before it returned None.
The cold rows pay one more evaluation of the pieces than the -2 head (tab 446 -> ~655, retry 1072 -> ~1265): still
400-700x fewer than base's 384-bit path.

## Jobs
Superseded (older heads): libm-lgammaf-edge-3-gate, -gate2, -gate3, -gate4 (killed when the code changed), -plants
(9/9 CAUGHT on 29bc9a5b3), -plants2/-plants3/-plants4 (killed), -percall/-percall2/-percall3/-percall4 (rows above),
-lint0..-lint3 (rc 0 on their heads).
Final head: see the JSON record's `jobs`.

## Informational
- percall `check` MISMATCH on lgammaf_retry and lgammaf_big: the same in base and new; Liella's sums match base's
  accurate path. Not a regression.
- The -2 sweep (exhaustive) was not rerun: the run-3 changes alter only which code reaches the same pieces; the
  unit tests that cover the second try and the table (lgammaf_small_refusals_take_the_second_try,
  lgammaf_table_entries, lgammaf_below_minus_42_takes_the_fast_path) run in the gate.
- Follow-up #5641 (margin 5 for lgamma_edge's tiny piece) is out of scope, noted in handoff/LIBM-LGAMMAF-EDGE.md.
