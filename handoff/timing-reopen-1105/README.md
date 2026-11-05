# TIMING-REOPEN-1105: profiles of the rows where time disagrees with instructions, and the follow-up briefs

Source: TIMING-SWEEP-1105 (sweep build of next f2157504, its counters.tsv/rounds.tsv from both timing hosts, re-analysed
with handoff/timing-sweep-1105-analyze.py). Profiles: booked Ivy Bridge timing host (E3-1220L v2, booking 34), perf 6.8
named events, user mode; jobs timing-reopen-1105-prof (perf stat, 7 event sets x 3 runs, cycle samples per row),
timing-reopen-1105-prof2/-prof3 (memref region counters; the sampling half produced no samples). glibc side: event
counts and per-symbol shares only, no annotation (clean-room rule: no disassembly of glibc).

| Group | Brief | Rows (IVB time) | Named stall | Confidence |
|---|---|---|---|---|
| strchr/strchrnul/strncmp | TIMING-IVB-STRSCAN-DISPATCH | 1.16-1.56 | front-end: undelivered uop slots 22-27x glibc = 90-97% of the strchr gap; two taken dispatch branches + two mask loads per call; strncmp: load-latency (no-execute cycles 8.7x) | high (strchr), medium (strncmp) |
| malloc/free, realloc in place | TIMING-IVB-MALLOC-CHAIN | 1.06-1.19 | scheduler full (rs-full 20.4 vs 7.7 per pair) on a chain carried through the bitmap word (PERF-MALLOC-IVB-CYCLES) | high (pair), not verified (realloc) |
| memref small/mid | TIMING-IVB-MEMREF-ENTRY | 1.08-1.22 | constant +2.5-5 cycles per call with fewer uops; entry/dispatch suspected | medium; stall attribution not verified |
| iconv translit 4k | TIMING-ICONV-TRANSLIT-CPI | 1.30 IVB / 1.45 N100 | mixed: MITE delivery (4.4x), flags-merge uops (1.08G vs 0), store-forward blocks (35.6M vs 0), rs-full 1.38x | high (events), sites not located |
| nscache first call | TIMING-NSCACHE-FIRSTCALL | 286-290 vs 273 us | cold start: IPC 0.39, ~35% of cycles front-end, 22-28% kernel; per-row split not possible with CBENCH_ROWS | low |

Not covered this round (later round, N100 host not profiled for lack of budget): mutex_lock_unlock_normal_mt
(N100 instr 0.852, time 1.199), memcpy_144 / memcpy_256 (N100 1.077), memcmp_256 / memcmp_4k (N100 1.061 / 1.086),
memchr_64k (N100 1.169), rawmemchr_64k (N100 1.060), strstr_miss_64 (N100 1.102); Ivy Bridge REOPEN rows outside
the groups: strstr_hit_4k (1.277, cycles 1.100), regexec_class_nomatch_1k (1.145), posix_spawn_true_heap256m (time 1.493,
cycles 1.054: kernel/fork time, not user cycles).
