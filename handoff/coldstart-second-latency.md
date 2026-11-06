# COLDSTART-SECOND-LATENCY(-2): second_setlocale_en_US / second_mktime_new_york time without instructions

State: branch task/coldstart-second-latency-2 (from origin/next c101c4c6 + the -1 run's WIP commit). No code change.
Result of -2: INCOMPLETE. The in-process probe is built and run on the Broadwell-EP build host. The single-step
footprint of both second calls is named (Liella only). The N100 run of the same probe (nozomi:csl2-pmc-n100) was still
queued behind another story's bench at hand-back, so the N100 attribution is still open. Plan rows: none new; the plan is
handoff/coldstart-plan-1106/COLDSTART-SECOND-LATENCY.md.

## Earlier (-1 run, still valid)
N100 `--compare` re-time (nozomi:CSL-n100): second_setlocale 261 vs glibc 184 ns (1.42x), second_mktime 630 vs 485 ns
(1.30x), ranges disjoint. Liella runs fewer instructions on both (177 vs 202, 1071 vs 1322). Whole-process `perf stat`
deltas cannot resolve a call of this size. Do not redo either.

## Probe (-2): in-process counters around the one call
`mkprobe.py` patches a copy of bench/c/cbench_opt.c. It adds `pmc.h` (perf_event_open, user-only, a group of up to 8
events read with rdpmc through the mmap page, so no syscall in the window). It reads the counters just before and after
setlocale (S), localtime_r (L) and mktime (M) at k=0/1, plus an empty window (N, the probe's own cost). It also adds a
`pmc <R>` mode that runs the bench's `fresh()` (31 fresh processes, same env) R times. Events come from `./pmc_events`
("<type> <hex config>" per line). Sources: kanon:~/work/COLDSTART-SECOND-LATENCY-2/{pmc.h,mkprobe.py,build.sh,run.sh,
runk.sh,agg.py,trace.py,ana.py}. Binaries: probe.liella (static, libc.a of this tree) and probe.glibc (dynamic). The same
files are in nozomi:~/work/COLDSTART-SECOND-LATENCY-2/.

### Broadwell-EP build host (loaded, job kanon:csl2-pmc-bdw): 155 fresh processes per side, means
| window | side | cycles | instr | L1D miss | L1I miss | dTLB miss | iTLB miss |
|---|---|---:|---:|---:|---:|---:|---:|
| N2 (empty) | Liella / glibc | 1696 / 1500 | 197 / 197 | 17.7 / 16.9 | 8.6 / 2.9 | 0.8 / 0.2 | 0 / 0 |
| S2 | Liella / glibc | 2142 / 1885 | 378 / 382 | 32.4 / 26.2 | 40.2 / 25.6 | 0.7 / 0.0 | 0 / 0 |
| M2 | Liella / glibc | 3593 / 3216 | 1228 / 1522 | 54.8 / 34.8 | 70.6 / 74.9 | 0.7 / 0.3 | 0 / 0 |
| L2 (reference) | Liella / glibc | 3210 / 3260 | 789 / 961 | 49.6 / 42.8 | 67.4 / 65.9 | 2.1 / 0.2 | 0 / 0 |

Net of the empty window (medians): S2 384 vs 398 cycles, so parity on this class, with +9 L1I and +5 L1D misses.
M2 1848 vs 1718 cycles (+130), with **+19 L1D misses** and about the same L1I misses. There are no TLB misses to speak of
on Broadwell (its STLB is large). The empty window already differs by 8.6 vs 2.9 L1I misses and about 200 cycles: the
probe's own code lies differently in the two binaries. Subtract N, or compare the sides only after subtracting it.

### Footprint of the second calls (Liella only; gdb single-step of probe.liella, kanon:csl2-trace3)
Run as the bare cold child (`probe.liella cold /nonexistent`, the bench env), so the gettext rows ran without their
catalog; this does not change the footprint of the two calls themselves.
- **2nd setlocale**: 173 instructions over 4 code pages (19 lines): setlocale, boundary::locale::setlocale,
  legacy::global_locale::setlocale, machine strlen/equal. 42 memory operands over 9 data pages (16 lines). Three of them
  are .rodata pages read for ONE line each (an llvm constant at 0x75d000, another at 0x7c4000, `liella_machine_short_lim`
  at 0x82f000). Then cpu::STATE / cmp tables (.data 0x86c000), global_locale (0x86d000 and 0x8ab000), one heap line and
  the stack.
- **2nd mktime**: 925 instructions plus one vDSO call over 10 code pages (68 lines). The vDSO call is
  `recheck::now` -> clock_gettime(CLOCK_MONOTONIC_COARSE), from `time_compat::select`'s "check due?" test on every
  implicit tzset, through vcall2 and VDSO_ST. It adds the vDSO text page and the vvar page (a line the kernel rewrites on
  every tick). Code: env_find (TZ compare), time_compat::select, timelocal/zone::mktime/Zone::resolve (0x451000-0x454000),
  Zone::kind_ at 0x4d1000 (far from the rest). Data: 12 pages / 41 lines. The kept zone is in **five separate anonymous
  mappings** (0x7ffff4040000, ...4240000 (6 lines), ...4300000, ...4380000 (7 lines), 0x7ffff7fe8000), apparently one per
  allocation size class. On top of that come 17 stack lines over 3 pages, .data (environ, time_compat CURRENT, VDSO_ST,
  cpu::STATE) and two .rodata pages.

Reading (Broadwell): M2's +19 L1D misses fit a zone spread over 5 mappings plus a deep stack and the vvar line, which the
intervening rows (regcomp, wordexp, gettext, ...) have evicted. glibc's absolute miss count is lower (aggregate only).

## Open / next step (in order)
1. Read nozomi:csl2-pmc-n100 (`~/logs/csl2-pmc-n100.log` on nozomi; raw data in ~/work/COLDSTART-SECOND-LATENCY-2/
   out.{A,B}.{liella,glibc}). It has two event sets of 10x31 processes per side: A = cycles, instr, mem_load_uops_retired
   .l2_hit/.l3_hit/.dram_hit, dtlb_load_misses.walk_completed, mem_bound_stalls.load/.ifetch; B = icache.misses, itlb
   walks, ld_head.l1_miss_at_ret/any_at_ret, mem_bound_stalls.ifetch_l2_hit/load_l2_hit. Subtract N2 per side. If the N100
   repeats Broadwell's picture (M: L1D/L2 misses and load stalls; S: ifetch / L1I or page walks), the cause is named.
2. Proposals, by expected gain, each needing a measured before/after on the N100:
   a. mktime: skip the vDSO coarse-clock read in `time_compat::select`'s hot path with the TSC gate gettext already uses
      (recheck::tsc_allow / tsc_holds, D-7.12 GETTEXT-6): rdtsc instead of vDSO text + vvar + VDSO_ST. This is local and
      testable; the TSC-gate tests in intl are the model.
   b. mktime: allocate a kept zone's parts (transitions, types, abbreviations, Zone record) from one block, so a
      conversion reads 1-2 pages, not 5 mappings. This touches the zone loader / allocator use, so it needs a ruling
      (architectural).
   c. setlocale: on the N100 the gap is 77 ns with 9 data pages for 16 lines. Co-locate the three single-line .rodata
      reads and the global_locale state (0x86d000 / 0x8ab000) only if set B shows page walks or L1D misses there.
      Broadwell shows parity, so wait for the N100 first.
3. Tests/plants only once code changes: the setlocale/mktime conformance tests and the recheck/tsc tests.

## Not to redo
The bench re-time; whole-process perf stat; the probe build (rerun `bash ~/work/COLDSTART-SECOND-LATENCY-2/build.sh`
on kanon after a code change); the gdb traces (trace3 in kanon:/mnt/zram/claude/csl2/, removed by ram-sweep). The gdb
trace must break at `*setlocale` / `*mktime` (exact entry) and step over vDSO calls: a single-stepped vDSO
coarse-clock read spins in its seqlock retry loop.

## Report
- Commits: this handoff and the report json only (no code).
- Jobs: kanon:csl2-build2 (probe build, rc 0), kanon:csl2-pmc-bdw (rc 0), kanon:csl2-trace3 (rc 0), nozomi:csl2-pmc-n100
  (queued in the timing queue at hand-back). Superseded: kanon:csl2-build (rc 1, missing -I bench/c), kanon:csl2-trace
  (killed: it broke after the prologue), kanon:csl2-trace2 (it single-stepped into the vDSO).
- Gate / plants: not run (no code change).
- RULING NEEDED: none yet. Proposal 2b, if the N100 confirms it, needs one.
- Refusals: none. Nothing of mine is running on kanon. nozomi:csl2-pmc-n100 is left queued (data only).
