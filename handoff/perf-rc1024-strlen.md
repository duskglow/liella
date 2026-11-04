# PERF-RC1024-STRLEN handoff (run 3, PERF-RC1024-STRLEN-3: RESOLVED by PERF-RC1024-HEAP-FREE; no code)

State: branch task/perf-rc1024-strlen-3 (on top of runs 1 and 2: task/perf-rc1024-strlen, task/perf-rc1024-strlen-2).
No product code was changed in any of the three runs. Status: resolved by PERF-RC1024-HEAP-FREE (branch
task/perf-rc1024-heap-free, head 29a5b91f, queued for CI). Nothing is left to do here once HEAP-FREE lands.

## Result (run 3): HEAP-FREE removes the regression
wl_count on keke, job perf-rc1024-strlen-3-count1 (rc 0), same method as run 2 (WL_BASE_EXACT=1, --base fa678f74,
instructions:u, ratio new/base):

| tree | bash-loop | make-n |
|---|---|---|
| 29a5b91f (HEAP-FREE head, heap.rs back to fa678f74) | **0.9997** | **0.9989** |
| ce0a9ce6 (HEAP-FREE's base, still has perf-heap-pair-check's heap.rs) | 1.0087 | 1.0053 |

Raw instructions:u: bash-loop 19066619518 (29a5b91f) / 19243837907 (ce0a9ce6) / 19072619131 (fa678f74);
make-n 758931584 / 763809105 / 759762382. Both workloads are below fa678f74 with HEAP-FREE: the story target is met.
`git diff --stat fa678f74 29a5b91f -- crates/rt/src/heap.rs crates/boundary/src/heap.rs` is empty (byte-identical).

## Earlier findings (runs 1 and 2)
- Run 2 bisected all the RC-1024 growth in bash-loop (+0.87%) and make-n (+0.54%) to one merge, 4b31a189
  perf-heap-pair-check. The scan.S merges are not the cause (run 1, 0.9987 .. 1.0007).
- The fresh-block alignment probe showed identical user-pointer offsets mod 64, so alignment of fresh blocks is not the
  mechanism. The exact mechanism (reuse path, strlen input shapes) was NOT found and is not needed now that the heap change is
  reverted.
- du's growth (fts_build/fts_alloc, candidates 5dd88211 / e45c4758) is a separate item, not counted.

## Open / next step
- Nothing for this story. If perf-heap-pair-check's malloc+free pair gain is attempted again, count bash-loop and
  make-n against fa678f74 before landing (these two workloads caught it); the reuse-path strlen input shapes are the
  first thing to record (validation/wl_shapes.sh).
- du's fts growth may deserve its own story (coordinator's call).

Do not redo: the bisect, the alignment probe, the HEAP-FREE count above.

## Jobs (keke)
- perf-rc1024-strlen-3-count1: rc 0. Log ~/logs/perf-rc1024-strlen-3-count1.log; per-sha logs
  ~/work/perf-rc1024-strlen-3/count-<sha>.log. Script ~/work/perf-rc1024-strlen-3/count.sh <list>.
- Runs 1-2: perf-rc1024-strlen-bisect2, perf-rc1024-strlen-2-bisect3, perf-rc1024-strlen-2-bisect4 (all rc 0).
