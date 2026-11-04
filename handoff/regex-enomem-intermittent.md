# REGEX-ENOMEM-INTERMITTENT: handoff

Branch task/regex-enomem-intermittent. Explained and fixed in the test (test-only change).

## Cause
`regex_enomem` lowers RLIMIT_AS to size+6 MiB in a re-executed child. The Liella heap serves a thread from the shard of
its NUMA node (D-3.10/3.12) and a shard without a segment needs a new 16 MiB segment (`SEG_SHIFT` 24), more than the
6 MiB left. If the child's thread moved to a CPU on the other node between the warm-up and the limit, a small
allocation failed: Rust aborts the process ("memory allocation of N bytes failed", SIGABRT, parent sees `None`), or
the last small `regcomp` returned an error (child exit 14, the failure seen at the gate). mei has 2 NUMA nodes, so
the outcome depended on where the scheduler put the thread (host load), not on the code under test or on other tests.

## Evidence (mei, interposed binary, 1 test per binary, so no other test allocates meanwhile)
- unfixed, run alone: 10/300 failures (all exit 14), later 0/3000; whole binary: 0/300, 5/400, 147/3000 (all aborts).
- affinity: pinned to node0 CPUs 0/4000 failures, node1 CPUs 0/2000, unpinned 57/2000. Pinned to the full mask 0-47: 0/2000 (not explained; taskset's explicit affinity may change placement).
- forced migration (warm on CPU 0, move to CPU 12 before the limit): 42/50 fail; staying: 0/50.
- fixed test (pin to current CPU at the start of the child): 0/4000. Plant (pin removed): 33/4000 caught.
- ruled out: allocation counting/hook (none exists; the limit is a real RLIMIT_AS), other tests in the binary (only one test), memory pressure (room measured 6291456 in every run), environment.
- not proven: that exit 14 on the 05:05 gate was the same mechanism; it is the same symptom class (a small allocation at the limit failing), appeared only before the fix, never after.

## Decision / ruling
The test fix is local. RULING NEEDED: product question: under RLIMIT_AS the heap does not fall back to another shard's
segment with free space when its own shard cannot map a 16 MiB segment, so a multi-node process near its limit gets
ENOMEM for small blocks that fit elsewhere. Recommend: on a failed segment map, retry the allocation from other
shards' partial slabs before failing (a heap change with its own story and counts).

## Next step
Coordinator: decide the heap fallback story. Nothing left here. Plan rows: D-3.10, D-3.12, D-14.4.
