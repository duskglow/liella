# TIMING-COV-STRCMP-NAMES-AVX2 (handoff, run 2 = TIMING-COV-STRCMP-NAMES-AVX2-2)

State: branch task/timing-cov-strcmp-names-avx2-2, head = the report commit after 33e05f83 (revert). Role shiki:~/w/sh2
(Haswell-EP). Code: csys/src/machine/scan.S is back to next's layout (the 63c7c1ec alignment is reverted).
Decisions: D-4.58 rewritten as "tried, reverted"; in force D-4.27, D-4.29, D-4.31, D-4.33, D-4.38.
Plants: handoff/TIMING-COV-STRCMP-NAMES-AVX2-plants.txt (now anchored on liella_machine_strcmp: /
liella_machine_casecmp_ord_avx2:; they mutate the blocks, not the alignment, so they hold on the reverted code).

## Run 2 results
- plants2 of run 1 was REFUSED by the tool (no function-label anchors). Fixed (6022ad95); plants on 6022ad95:
  4 CAUGHT, 1 EQUIVALENT (page-early: the slow path serves every offset), 0 WARN (shiki label -plants).
- Lint (clippy -D warnings + static) rc 0 on 6022ad95 (-lint).
- jcc32 scan (staged base vs head): both named crossings gone (strcmp+0x3ff cmp $0xf7f/ja; casecmp_ord_avx2+0x11c
  test/jne); the shift moved one crossing into .Lcord_step32 (cmp $4064/ja, generic loop) and cleared 4 others.
- Haswell-EP counters (whole process, base vs head, 3 rounds): dsb2mite penalty and idq-not-delivered flat (noise).
  Haswell has no jump-crossing rule, so this is expected. The Skylake-SP check (mei -profsk) sat queued behind a
  whole-host booking for 25 min and was cancelled; the 5.3x / 2.1x counter fall is NOT VERIFIED.
- Timing, staged trees (`bench --stage`, unpacked whole), A/B base(A) vs head(B), 9 rounds:
  N100 (nozomi -n100): strcmp_names 0.999, strncasecmp_c_64 1.000, instr equal 64.5 / 70.1; Liella/glibc 1.106-1.111
  and 1.050 both trees. cbench_opt/second_strcmp 62 -> 71 ns SLOWER (spread 1.6%); rerun 21 rounds (-n100ab3)
  62 -> 70 ns, NOISY (spread 21%). Ivy Bridge (kotori -ivb): both rows flat (NOISY spreads, cycles ok; strncasecmp_c_64
  0.948 vs 0.950 Liella/glibc), no row flagged.
- Verdict: no gain on any timing host, one cold row slower on N100 -> reverted (33e05f83), NEGATIVE row in
  docs/saturation/15-the-seven-benches-section-14-skipped.md.

## Final head jobs (shiki): -lint2, -gate2 (direct tier, --base origin/next), -plants2 (plants-2 RAM dir).

## Open / next step
- Targets still open on N100: strcmp_names 1.11, strncasecmp_c_64 1.05 (Ivy Bridge faster). N100 (Gracemont) has no uop
  cache: candidates are (1) the strncasecmp export's layout (3 taken jumps before the tail jmp; cold_path on the SSE2 arm
  costs Ivy Bridge a taken branch, its margin 0.947), (2) the strcmp AVX2 differ return as fall-through (one taken branch
  fewer for names, one more for short equal strings). Profile on N100 first (it has perf, no compiler; stage both trees).
- If Skylake-family timing is ever available, the casecmp half alone (`.Lcord_head_next`) is the part worth re-trying;
  it does not move strcmp, so second_strcmp would not see it. Needs a Skylake timing host, which the lab does not have.
- Scripts: shiki ~/work/TIMING-COV-STRCMP-NAMES-AVX2-2/ (base.sh stages a base from a RAM copy with a reverse patch;
  prof.sh; jcc32.py prints the fused pair); timing.sh / ab2.sh on nozomi and kotori.
