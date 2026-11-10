# HEAP-FIRST-USE-2: ask the allowed-node set once, read the node only on several nodes

State: branch `task/heap-first-use-2` (stacks on `task/heap-first-use`, not landed when this ran), role mei:~/w/s1.
Code commit 07b7a2e4 (+ this handoff/report commit). Status: PENDING JOBS (gate escalated to FULL, counts running).

## Decisions in force
D-3.20 (new, `records/decisions/D-3/D-3.20.toml`, `amends = ["D-3.12"]`, coordinator ruling on #4168; fixes #4169):
the heap asks `Pages::allowed_nodes` once, lazily, on its first cache/slab slow path (`Heap::shard_of`; product: one
`get_mempolicy(MPOL_F_MEMS_ALLOWED)`, 1024-bit mask, `EINVAL` = several, other errors = one). Several nodes:
`numa` on, node read on slow paths as D-3.12 says. One node: shard 0, default placement, the node is never read
(the vDSO `getcpu` is never resolved). An explicit `Heap::set_numa` before the first slow path decides instead. A
process whose allowed set changes later is out of scope (in the record). D-3.19 unchanged.

## Code
- `crates/rt/src/heap.rs`: `Pages::allowed_nodes` (default 1), `Heap.topology`, `Heap::shard_of`; `first_cache` and
  `refill` call `shard_of` instead of `pages.shard()`; `set_numa` also marks the decision taken.
- `crates/boundary/src/heap.rs` (`KernelPages::allowed_nodes`), `crates/boundary/src/raw.rs` (`allowed_node_count`),
  `crates/kernel/src/uapi.rs` (`MPOL_F_MEMS_ALLOWED` = 4).
- Tests (rt `heap_and_sync`, simulated topology; `FakePages` now simulates 2 allowed nodes by default so the
  existing shard tests keep choosing shards): `one_allowed_node_never_reads_the_node` (asked once, 0 node reads
  across ~20 refills and a second thread, one segment, node None), `several_allowed_nodes_place_each_threads_slabs_on_its_node`
  (no `set_numa` call; asked once; no node read on 100 fast-path allocations; node 0 / node 1 segments).

## Results (mei)
- Foreground on the synced tree: `cargo xtask clippy` rc 0, `cargo xtask static` rc 0, `records check` ok,
  `cargo test -p liella-rt --test heap_and_sync` 27 passed.
- Plants `hfu2-plants` (`handoff/HEAP-FIRST-USE-2-plants.txt`): 3 CAUGHT (one-node-reads-node, two-nodes-no-numa,
  node-read-on-fast-path), 0 SURVIVED, 0 WARN.
- Gate `hfu2-gate`: `affected --run --base origin/next` ESCALATED TO FULL because `records/decisions/D-3/*.toml` is
  "not a path the selection knows"; running at hand-back (`~/bin/jobwait hfu2-gate` on mei).
- Counts `hfu2-counts` (`mei:~/work/HEAP-FIRST-USE-2/counts.sh`, run from a RAM copy of the tree so it does not build
  in the role during the gate): meas.sh (HEAP-FIRST-USE's probe `hf.c` = TIMING-COV-COLD-SETLOCALE's cs.c + `M`)
  -> `~/work/HEAP-FIRST-USE-2/after/counts.txt`; strace of the first malloc; `strace -k` of the mmap_min_addr
  openat (#4170); `bench --compare --counters cbench_opt` -> `~/work/HEAP-FIRST-USE-2/bench.log` (first_* rows).
  Base to compare with: `mei:~/work/HEAP-FIRST-USE/after2/counts.txt` (liella M 9,225 iu / 320,428 ik / 25 faults;
  PS 67,805 / 513,413 / 49). Expected direction: -1 vDSO parse (~6k iu, 1 fault), +1 get_mempolicy. NOT VERIFIED.

## Open
- #4170 (mmap_min_addr read on the first malloc): from the code, `resolve_fold*` is reached from the copy/fill/memchr
  entries, so some small memset/memcpy/memchr in the first-malloc path triggers it; the guard only enables NULL
  folding, and until it is resolved every call takes the explicit checks, so moving the trigger off the heap path
  does not weaken it. The trigger site was not identified yet: `hfu2-counts` prints its stack (`strace -k`). Next:
  remove that call from the heap path if it is a heap-side array init/copy; if the only route is to skip or weaken
  the guard, RULING NEEDED instead. Note most programs then pay the 3 calls at their own first memcpy.
- INFORMATIONAL: D-3.12's rseq `node_id` branch is not wired (`uapi::Rseq` exposes no node_id; the read on several
  nodes is the vDSO getcpu). Recommend a separate story (probe whether the kernel fills node_id for Liella's 32-byte
  registration).
- Timing (Ivy Bridge, N100) of the first_* rows: NOT BOOKED (budget). Next step after counts are green.
- IMPROVEMENT: `affected` escalates to FULL on any `records/` path; a decision record should select nothing (or the
  tests named in its `tests` field).
