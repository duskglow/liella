# PERF-RC1024-STRLEN handoff (WIP after run 2, PERF-RC1024-STRLEN-2; no code change yet)

State: branch task/perf-rc1024-strlen-2 (on top of run 1's task/perf-rc1024-strlen WIP), base origin/next 125c0ccc.
No product code changed. Status: NOT READY. The cause is found to one merge, but there is no fix yet.

## Result: the regression is one merge, 4b31a189 "Merge task/perf-heap-pair-check into next"
wl_count on keke (WL_BASE_EXACT=1, --base fa678f74, instructions:u ratio new/base):

| commit | bash-loop | make-n |
|---|---|---|
| the 5 scan.S merges and their first parents (run 1, bisect2) | 0.9994 .. 1.0002 | 0.9987 .. 1.0007 |
| 094b61a5 perf-execvp-missing | 0.9996 | 0.9987 |
| b3108350 perf-wctype-l-cold | 0.9996 | 0.9987 |
| **4b31a189 perf-heap-pair-check** | **1.0088** | **1.0052** |
| 5abebe52, 10734b43, b6f8d1e6, 9131d50e, c874901f, 78e911bc, 9d9bc8fb | 1.0085 .. 1.0089 | 1.0052 .. 1.0053 |
| 76041c21 (the RC) | 1.0084 | 1.0053 |

- The scan.S merges are NOT the cause. The strlen growth in the profiles comes from the heap change, through strlen's inputs.
- There is no other step between fa678f74 and 76041c21. 4b31a189 accounts for all of the RC-1024 growth in bash-loop
  (+0.87%) and make-n (+0.54%).
- du is not this cause (run 1: fts_build/fts_alloc growth, candidates 5dd88211 / e45c4758, not counted).

## What does not explain it
- Probe (keke ~/work/perf-rc1024-strlen-2/align.c, static against ~/wl/products/<sha>): for 64 fresh live blocks
  of each size 1..200, the user-pointer offsets mod 64 are IDENTICAL in b3108350 and 4b31a189. Alignment of fresh
  blocks therefore does not explain it. Untested: offsets after free/reuse (bash and make free and reallocate
  constantly), and a change in how many strlen calls are made or how long the strings are.

## Next step
1. Find what changed in strlen's inputs between b3108350 and 4b31a189. Record strlen call shapes (count, length,
   address mod 64 and page offset) for bash-loop at both shas with validation/wl_shapes.sh or wl_trace.sh (both
   products are already built in keke ~/wl/products/<sha>). Also profile make-n's memmove and calloc, which grew too.
2. Read the current heap code (crates/rt/src/heap.rs, crates/boundary/src/heap.rs) for what pair-check changed in the
   reuse path. Its own record is row N2-2 in docs/saturation/14-next-ranked-perf-stories-after-perf.md.
   Do not read other people's commit diffs: work from the code as it is and from counts.
3. The fix must keep perf-heap-pair-check's malloc+free pair gain (`cargo xtask bench --compare --counters` for the
   malloc/free pair rows and the strlen rows). Target: bash-loop and make-n at or below fa678f74.
   If the pair gain and the workloads conflict, report the numbers and write RULING NEEDED.
4. Gate (direct tier) and plants on the fix.

Do not redo: the bisect (all points above), priming (sources for all counted shas are in keke ~/wl/src, products
in ~/wl/products), the du breakdown, the fresh-block alignment probe.

## Jobs (keke)
- perf-rc1024-strlen-bisect2 (run 1): rc 0, the scan.S chain.
- perf-rc1024-strlen-2-bisect3: rc 0, 10 points 094b61a5..76041c21. Log: ~/logs/perf-rc1024-strlen-2-bisect3.log
- perf-rc1024-strlen-2-bisect4: rc 0, b3108350. Log: ~/logs/perf-rc1024-strlen-2-bisect4.log
- Script: ~/work/perf-rc1024-strlen-2/bisect3.sh <list> (lines "<name> <sha>"; it makes the `.synced` stubs under
  t/<sha>). Prime each sha first with validation/wl_prime_base.sh keke <sha>. A second count for a different sha can
  run in parallel: wl builds each sha under its own flock in ~/wl, outside the role.
