# STRSTR-SHORT-DENSE-PARITY handoff (CHECKPOINT, not finished)

State: branch task/strstr-short-dense-parity, stacked on STRSTR-FILTER-LAND (a65a823f, now in origin/next). Commits:
37e2ef81 (WIP code: DENSE hand-back + short-needle prefix step), its revert, one docs commit (negative row in
docs/saturation/03) and this handoff. Net code change vs a65a823f: none. Role keke:~/w/n4.

Decisions in force: owner ruling #2686 (adaptive filter landed; strstr_hit_64 and strstr_dense_4k not final).

## What was tried (counts: job strstr-short-dense-parity-counts2, keke, rc 0, instructions:u, base = a65a823f)
| row | glibc | base | new (37e2ef81) |
|---|---|---|---|
| strstr_dense_4k | 818 | 997 | 1068 |
| strstr_scan_4k | 818 | 775 | 790 |
| strstr_hit_short | 153 | 173 | 188 |
| strstr_hit_4k / miss_4k / verify_4k | 10155 / 10146 / 18524 | 1271 / 1003 / 1021 | 1287 / 1016 / 1036 |
| ss_hit_64 | 163.2 | 188 | 207 |
| ss_hit_1k | 355.2 | 436 | 728 |
| ss_hit_4k / ss_miss_64 / ss_pairmiss_4k / ss_miss_4k | 833.6 / 339.8 / 951 / 7507 | 769 / 217 / 695 / 961 | 788 / 222 / 712 / 966 |
Every row worse, so both changes are reverted. Note: this run's base numbers differ from the brief's (hit_64 188 here
vs 171.7; dense 997 vs 1042), so the brief's numbers are from another build or harness: re-baseline first.

1. DENSE flag (bit 8 of c0, strpair AVX2): a one-stream stop within 1024 bytes of the previous one returned the unit
   before the group as a false candidate (a 0 in the group went to the block loop), so Rust re-ranked the needle and
   called again on the rarest pair (for "needle": d,l, which the dense row never holds). Faults: the FIRST stop counts
   as dense when it lies within 1024 bytes of the start, so a single match group (ss_hit_1k) paid the hand-back
   (+292); on dense_4k the hand-back plus second call cost more than the pair filter (+71). Not understood why it is
   that large: profile before retrying.
2. liella_machine_strprefix (SSE2, one 16-byte step for needle length and haystack prefix, 17 = fall back to the
   loop): about +15 on every row instead of a saving. Not understood (spills around the new call? the fallback
   taken?): check with validation/wl_trace.sh.
Plants on 37e2ef81 (job strstr-short-dense-parity-plants, rc 0): 5 CAUGHT, 2 EQUIVALENT, 0 WARN. The new test
strstr_dense_last_byte_run_hands_over_to_the_rare_pair is in 37e2ef81 (reverted with the code; reusable).

Not to redo: the two designs above as built. No per-phase profile exists yet (step 1 of the brief NOT DONE).

Exact next step: re-baseline, then trace one call of ss_hit_64 and strstr_dense_4k (validation/wl_trace.sh, spec
strstr_getline.c / strstr_cycles.c, function strstr) and write the per-phase split before any new design. Tooling:
prime the base first (bash validation/wl_prime_base.sh keke <base sha>) or wl_percall exits 2.

Plan rows to read: docs/saturation/03 strstr rows (adaptive filter row and the new NEGATIVE row).
