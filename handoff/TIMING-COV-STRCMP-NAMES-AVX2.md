# TIMING-COV-STRCMP-NAMES-AVX2 (handoff, run 1: checkpoint at the call budget)

State: branch task/timing-cov-strcmp-names-avx2, code commit 63c7c1ec (on next eef6b2dd). Role mei:~/w/s2 (Skylake-SP).
Decision in force: D-4.58 (new), with D-4.27, D-4.29, D-4.31, D-4.33, D-4.38. Plants: handoff/TIMING-COV-STRCMP-NAMES-AVX2-plants.txt.

## Profile (step 1; Skylake-SP role host, default level, Liella static vs glibc host-dynamic, whole bench process)
perf stat, CBENCH_ROWS=<row> (Liella / glibc). glibc runs its EVEX variant on this host:
- strcmp_names: dsb2mite_switches.penalty_cycles 45.3M / 8.5M (5.3x); idq_uops_not_delivered 116M / 64M;
  frontend_retired.dsb_miss 9.4M / 4.3M; branch-misses 459k / 513k (Liella has FEWER here). Instructions per call
  (bench --counters) 64.5 / 60.0.
- strncasecmp_c_64: penalty cycles 350M / 163M (2.1x); idq not delivered 692M / 271M; instr/call 70.0 / 60.0.
Per-instruction samples (perf record cycles:upp): strcmp 55% in liella_machine_strcmp, mostly the AVX2 first block
(.Lstrcmp_v_first); ~10% in the SSE2 head and slow path (names in .rodata whose a or b sits in the last 128 bytes of a
page). strncasecmp: kernel 74%, export 19% (`mov %fs:0x138` 33% of the export's samples).

Cycle sink (named): the Skylake jump-crossing rule (a jump, or a fused cmp/test+jcc, crossing or ending on a 32-byte
boundary keeps that 32-byte window out of the uop cache). Checker ~/work/TIMING-COV-STRCMP-NAMES-AVX2/jcc32.py on mei
(objdump scan per function). Hot hits: strcmp+0x3ff `cmp $0xf7f,%edx; ja` (.Lstrcmp_v_first page test of b, every
AVX2 call) and casecmp_ord_avx2+0x11c `test %r8d; jnz` (.Lcord_head_next, every n<=64 call that passes 32 bytes).
Other hits (SSE2 head +0x1b/+0x60, loops, cold exits: 17 in strcmp, 8 in casecmp_ord_avx2) are left as they are.

## Change (63c7c1ec)
`.p2align 5` before .Lstrcmp_v_first (was `.p2align 4`) and before .Lcord_head_next. Both follow an unconditional
jmp, so the padding never runs: instruction counts unchanged by construction. The SSE2 level is untouched (its code
in strcmp comes before the moved block; casecmp_ord_sse2 is its own section).

## NOT VERIFIED (jobs on the final head)
- mei timing-cov-strcmp-names-avx2-gate (direct tier, --base origin/next) and -plants2 (cargo xtask plants,
  /mnt/zram/claude/TIMING-COV-STRCMP-NAMES-AVX2/plants-2): started on 63c7c1ec, running at hand-back.
- Lint (clippy -D warnings + static) and the after counts did NOT run on 63c7c1ec: the labels -lint/-after had been
  used by runs I stopped (stale tree, sync race). Re-run them with new labels, e.g. -lint2 and -after2, using the
  sequencer ~/work/TIMING-COV-STRCMP-NAMES-AVX2/seq.sh with its labels changed.
- Before counts: ~/logs/timing-cov-strcmp-names-avx2-before.log (default level complete; -AVX2 cut short on
  purpose, rows strcmp_names 66.5/70.5 and strncasecmp_c_64 123.1/132.1 present). Before profile: -prof0 log.

## Next step
1. The jobs above with fresh labels; jcc32.py on the after binary must show no hit at those two sites; profile.sh
   `after` must show the penalty cycles falling; instructions equal on every strcmp/strncmp/strcasecmp row.
2. Booked timing (`ops timing-book`) of the two rows on the N100 host and the Ivy Bridge host (nico or kotori,
   whichever is less busy). N100 (Gracemont) has no uop cache, so this fix may not move N100. If N100 stays slow,
   next candidates: the strncasecmp export's layout (3 taken jumps before the tail jmp: the TLS view non-zero arm
   and the AVX2-kernel arm are laid out as branch targets; `core::hint::cold_path` is stable on 1.99.1, but marking
   the SSE2 arm cold would give Ivy Bridge a taken branch, and its margin is 0.947), and the strcmp AVX2 differ
   return as fall-through (one taken branch fewer for names; one more for short equal strings).
