# COLDSTART-SECOND-LATENCY: second_setlocale_en_US / second_mktime_new_york time without instructions

State: branch task/coldstart-second-latency (from origin/next c101c4c6, contains 730f416b). No code change. Result:
INCOMPLETE: the N100 re-time confirms both rows are above 1.05x with tight ranges (they are not noise). Aggregate
fresh-process counters do not resolve a per-call latency cost, so no fix was made. Plan rows: none new; the plan is
handoff/coldstart-plan-1106/COLDSTART-SECOND-LATENCY.md.

## Step 0: N100 re-time (staged 730f416b tree, built on kanon)
The planning job nozomi:CSP1106-n100 (DONE) ran `--counters`, which prints Liella ns/op only for these budget rows,
so it does not give a glibc time. I re-ran the same staged products with
`./xtask-bin bench --compare --no-build --counters --runs 5 cbench_opt` (job **nozomi:CSL-n100**, booked through
~/bin/timing; cold rows are a median of 31 processes, best of 7 rounds):

| Row | glibc dyn ns (range) | Liella ns (range) | ratio |
|---|---:|---:|---:|
| second_setlocale_en_US | 184 (178-187) | 261 (257-272) | 1.42 |
| second_mktime_new_york | 485 (476-491) | 630 (604-641) | 1.30 |
| second_localtime_r_new_york (for reference) | 485 | 375 | 0.77 |

The ranges do not overlap, so the gaps are real on the N100 class (about 77 ns, roughly 260 cycles, and about 145 ns, roughly 490 cycles).

## Step 1: aggregate counters per call (N100, both sides, perf stat -r 1500 fresh processes)
Probe: the COLDSTART-PLAN-1106 coldsteps binaries, unchanged. Delta = (base+call) - base, means of 1500 runs. Jobs
nozomi:CSL-lat-n100 (no output: the event list with instructions:k + raw_syscalls was not accepted together; perf
printed no CSV lines) and **nozomi:CSL-lat2-n100** (cycles:u, instructions:u, cycles:k, dTLB-load/store-misses:u,
cache-misses:u, page-faults).

| Call | side | cycles:u | instr:u | cycles:k | dTLB load | dTLB store | LLC miss:u | faults |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 2nd S (SCILFMRNBG -> +S) | Liella | +926 | +177 | -364 | +2 | 0 | +88 | 0 |
| | glibc | -566 | +202 | -306 | 0 | 0 | +25 | 0 |
| 2nd M (SCILFMRNBGSCILF -> +M) | Liella | +1414 | +1071 | +2963 | 0 | 0 | -11 | 0 |
| | glibc | +2075 | +1322 | +969 | 0 | 0 | +33 | 0 |

Reading: the instruction deltas match the planning probe (177/1071 Liella vs 202/1322 glibc), and there are no faults.
The cycle deltas are inside the run-to-run noise of a whole process (glibc's second S comes out negative; for M,
Liella's delta is smaller than glibc's, which is the opposite sign to the bench). Whole-process aggregates therefore
cannot measure a 260 to 490 cycle per-call effect. The only hint is Liella +2 dTLB load misses and +88 LLC misses on
the second S. That is not settled, and the dTLB figure is at the rounding limit of perf's mean.
L1-dcache-load-misses is not offered as a generic event on this CPU (perf list), so it was not counted.

## Not to redo
- The bench re-time (CSL-n100 above is the current number on 730f416b).
- Whole-process perf stat deltas at -r 1500. More runs only shrink the noise slowly; measure inside the process instead.

## Exact next step
Measure the single call inside the process, as the bench does. Use a copy of bench/c/cbench_opt.c's cold child that
opens perf_event_open counters (cycles, instructions, dTLB-load-misses, MEM_LOAD_RETIRED-style L1 miss raw events if
available) and reads them just before and after the second call, over 31+ fresh processes per side, on the N100
(booked) and on a build host. Then profile the Liella side by symbol on the build host (perf record -e
dTLB-load-misses,cycles) to name the pages and lines the second setlocale (find_cached / FindCache,
crates/core/src/locale/mod.rs:575-638) and the second mktime (crates/boundary/src/posix/s23_time.rs:257 -> tz rules)
touch. glibc side: aggregate counters and timing only.

## Report
- Commits: this handoff and the report json only.
- Gate / plants: not run (no code change).
- RULING NEEDED: none.
- INFORMATIONAL: `bench --staged --counters` prints no glibc time for budget (second_*/first_*) rows. A planning re-time
  that has to decide a ratio for those rows needs `--compare --no-build` (as done here).
- Refusals: none. Nothing left running (the nozomi jobs CSL-n100 and CSL-lat2-n100 finished; files are in
  nozomi:~/work/COLDSTART-SECOND-LATENCY/).
