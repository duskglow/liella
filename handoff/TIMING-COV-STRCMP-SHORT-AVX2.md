# TIMING-COV-STRCMP-SHORT-AVX2 (handoff; -2 continuation: timing + SSE2 entry layout)

State: branch task/timing-cov-strcmp-short-avx2-2 (= task/timing-cov-strcmp-short-avx2 + -2 commits). Code: f05d3094,
af02dbd1 (run 1), 6e563d2c (-2: SSE2 head is the entry's fall-through). Product change: csys/src/machine/scan.S only.
Test machine_scanners (strcmp page-end test), validation/percall/casecmp_short.c, plants
handoff/TIMING-COV-STRCMP-SHORT-AVX2-plants.txt. -2 role keke:~/w/n1. Stacked on 883a05df (TIMING-IVB-STRSCAN-DISPATCH-2).
Merge note: on top of current next, scan.S has ONE conflict (next's liella_machine_schr_mask block, PERF-STRCHR-NONAVX2-3,
and this branch's liella_machine_strcmp_vlim block are adjacent data blocks): keep both. Resolved that way for the timing trees.

## Change
- strcmp entry: one `cmpl liella_machine_strcmp_vlim(%rip)` on (a - 1) & 4095 is the level test AND the AVX2 head's page
  test of a (vlim 3968 on AVX2, 0 on SSE2, set in liella_machine_set_scan_level). -2: the branch is `jb .Lstrcmp_v_first`
  (AVX2 first block out of line, after `.Lstrcmp_top`'s jmp); the SSE2 head is the fall-through. Run 1 had the AVX2 block as
  fall-through and `jae` to the SSE2 head: taken on every SSE2 call, measured slower on Ivy Bridge (below). Same
  instruction counts either way.
- casecmp_ord_avx2 head: exact per-string page test of the 65 bytes from x - 1 (limit 4031) replaces NULL tests + or-tests.
- Test: strcmp_head_finds_every_difference_in_strings_ending_at_a_page_end (both levels).

## Results
Instructions:u per call (wl_percall, AVX2 class; glibc / before 883a05df / after): strcmp_names 46.4 -> 42.4,
strcmp_16_mix 59.8 -> 57.8, strcmp_64_mix 75.7 -> 73.7, strncasecmp_c_64 79 -> 73, strcasecmp_c_16 57 -> 56 (run 1 on mei;
-2 head re-counted on keke: identical "new" column, check ok on all 5).

Timing (method as handoff/TIMING-IVB-STRSCAN-DISPATCH.md): `cargo xtask bench --stage` on keke:~/w/n1 of three trees,
all = next 41b8737b + scan.S only: before = next's scan.S; after = run-1 head's scan.S merged; after2 = -2 head's scan.S
merged (patch of 6e563d2c onto the resolved merge). run.sh / run2.sh `5 4` (4 alternations x 5 rounds, interleaved) under
~/bin/timing, analysed with handoff/TIMING-IVB-STRSCAN-DISPATCH-2-analyze.py. Ratio = Liella after/before (median of
paired rounds); glibc drift is glibc's own after/before.

Run-1 head (jae to SSE2 head):
| row | N100 (AVX2) L a/b | glibc drift | Ivy Bridge (SSE2) L a/b | IVB L cycles/call before -> after |
|---|---|---|---|---|
| strcmp_names | 0.933 | 1.000 | 1.028 | 26.4 -> 27.3 |
| strcmp_16_mix | 0.914 | 0.998 | 1.119 | 20.1 -> 22.7 |
| strcmp_64_mix | 0.986 | 0.996 | 1.102 | 33.4 -> 36.9 |
| strncasecmp_c_64 | 0.954 | 1.000 | 1.000 | 36.5 -> 36.5 |
| strcasecmp_c_16 | 0.939 | 1.000 | 0.999 | 31.1 -> 31.2 |
Ivy Bridge ns are bimodal per round (frequency states; glibc shows the same two modes), so the time spread there is ~1.4;
the per-run cycle counters (counters.tsv) agree in all 4 runs and within each ns mode the shift is the same (16_mix 8.07 ->
9.09 ns). Instructions equal on IVB (56.9, 94.1, 66.5): the taken entry branch, not the count. -> fixed in 6e563d2c.

-2 head (jb to AVX2 block, SSE2 falls through):
| row | N100 (AVX2) L a/b | glibc drift | Ivy Bridge (SSE2) L a/b | glibc drift | IVB L cycles/call before -> after |
|---|---|---|---|---|---|
| strcmp_names | 0.970 | 1.000 | 1.019 | 1.001 | 26.05 -> 26.85 |
| strcmp_16_mix | 0.945 | 0.992 | 1.035 | 1.002 | 19.9 -> 20.5 |
| strcmp_64_mix | 0.990 | 0.986 | 1.009 | 1.001 | 33.3 -> 33.4 |
| strncasecmp_c_64 | 0.955 | 1.000 | 1.000 | 1.000 | 36.2 -> 36.2 |
| strcasecmp_c_16 | 0.939 | 1.000 | 1.000 | 1.000 | equal |
Verdict: AVX2 class faster on all 5; SSE2 class within the 1.05 parity band on all 5, but the cycle counters show a
consistent +0.6 cycles/call (+3%) on strcmp_16_mix and strcmp_names in all 4 runs (16_mix fast-mode ns 8.1 -> 8.35) at
equal instructions: RULING NEEDED in the hand-back (recommend land; follow-up below). nico2 was started 9 s before the
booking's settle window ended (the timing tool's own 12 s settle covered it; host load 0.00).

## Next step (optional follow-up)
Recover the SSE2 class's +3% on strcmp_16_mix / strcmp_names: candidates (none tried) - `.p2align 4` padding is NOT free
on the fall-through, so try placing the SSE2 head at an aligned address by moving the entry's three instructions into
the padding of the previous function's section, or a level-specific entry symbol chosen at dispatch time. Time on the
Ivy Bridge host with the staged trees above (before.tar is next 41b8737b).

## Jobs (-2)
keke: timing-cov-strcmp-short-avx2-2-gate (static, machine_scanners 50/50, casecmp_avx2 6/6, both percall tables) rc 0;
timing-cov-strcmp-short-avx2-2-plants (cargo xtask plants, 5/5 CAUGHT, 0 WARN-RC-ONLY/NO-MSG; WARN-DEV-SHM only) rc 0;
timing-cov-strcmp-short-avx2-2-lint (clippy -D warnings + static on 0e09f9e5) rc 0; stage-after/before/after2 rc 0.
nico: timing-cov-strcmp-short-avx2-2-nico (run-1 head), timing-cov-strcmp-short-avx2-2-nico2 (-2 head).
nozomi: timing-cov-strcmp-short-avx2-2-nozomi (run-1 head), timing-cov-strcmp-short-avx2-2-nozomi2 (-2 head).
Staged tars and run scripts: ~/work/TIMING-COV-STRCMP-SHORT-AVX2-2/ on keke, nico, nozomi.

## Not to redo / open
- Do not put the AVX2 first block back as the entry fall-through (SSE2 class slower at equal instructions).
- Instruction gap left on strcmp_names (AVX2): 7.4 (page test of b 4, a 4, block 7, settle 3, return 3).
- No D-row (local layout detail). Bench --compare --counters rows not run; percall only.
