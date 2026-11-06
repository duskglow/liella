# NULLFOLD-COPY-MID: handoff (one page; last run NULLFOLD-COPY-MID-4)

State: branch `task/nullfold-copy-mid-4` (stacks on `task/nullfold-copy-mid-3`, -2, the first; includes the coordinator's
merge of origin/next cc39389c, c3120f6b). Code commit of this run: ef1b3535; handoff/report after.
Verdict: COMPLETE. D-4.55 129-256 copy folding with the one-word gate; start-up back under the limit and below next.

## This run (CI red at c3120f6b: startup_instr_c 2158 > limit 2150)
Cause, measured on the Zen 1 host (keke, `CBENCH_COUNTERS=1` ctr lines of the bench binary, min of 5 children):
next (HEAD with this story's crates/boundary/src + csys/src diff reversed) 2142; head c3120f6b 2158 (+16). `select` stored the
129-256 word twice-callable with a separate fold-limit load each, though at startup folding is still undecided (lazy,
STARTUP-1) and the word is already 0. Fix 1 (store in `store_cmp_gates` unconditionally) gave 2148: the fold-off cmove +
store still ran at startup. Fix 2 (ef1b3535): `store_cmp_gates` raises the word only `if fold` (constants on the fold-on
branches) and `drop_move_word` drops it before every fold-off store of the fold limit (`select_null_fold(false)`,
`unresolve_null_fold`); with folding off the word is 0 by that invariant, so the startup `select` does not touch it.
Invariant unchanged (never names folding while off, never the AVX2 body while the flag is off), covered by
`the_129_to_256_copy_word_follows_the_folding_and_build_settings_in_either_order` (every transition, both orders).

| row (Zen 1, instr/process) | next | c3120f6b | fix 1 | ef1b3535 |
|---|---|---|---|---|
| startup_instr_c | 2142 | 2158 | 2148 | 2136 |
| startup_instr_setloc | 67824 | - | 67815 | 67820 |
| startup_instr_printf | 71602 | - | 71607 | 71580 |
| startup_instr_wctype | 68871 | - | 68879 | 68870 |
(ASLR moves a few instructions; fix 2 is at or below next on every row.) fold.S changed in comments only, so the 129-256
copy rows' code is the -3 run's (timed there: no row slower on Ivy Bridge or Alder Lake-N).

## Jobs (keke)
nullfold-copy-mid-4-cnt-base (next, rc 0), -cnt-head2 (c3120f6b, rc 1: the INSTR-REGRESS reproduced), -cnt-fix1 (rc 0),
-cnt-fix2 (ef1b3535 code before commit + workspace clippy -D warnings + xtask static: rc 0), -gate (direct tier, ef1b3535),
-plants (11 plants, handoff/NULLFOLD-COPY-MID-plants.txt), -cnt-final (startup + memref on the final synced head).
Results: see handoff/reports/NULLFOLD-COPY-MID-4.json. Superseded: -cnt-head (rc 1, wrong bench argument, mine).

## Next step
None required: land (with its parent branches). Optional: Zen 1 instruction row for memcpy_144/256.
Plan rows: D-4.22, D-4.52, D-4.55 (plan/07-decisions/s04-byte-arrays-and-strings.md); saturation docs/saturation/03.

## Not to redo
Two-word gates (either order); code alignment of the SSE2 body; explicit NULL tests on one build only; a one-compare
signed/unsigned trick; any move-word store or fold-limit load in the startup `select` (startup_instr_c limit).

## Slips (this run)
First count job used a positional bench name (`--only` is required). One `cat > file < /dev/null` truncation (no effect).
