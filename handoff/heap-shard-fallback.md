# HEAP-SHARD-FALLBACK: handoff

Branch task/heap-shard-fallback. Plan rows: D-3.10, D-3.12 (shards), D-3.11 (owner/home shard).

## State
`Heap::refill` (crates/rt/src/heap.rs): when the calling shard's `take_slab` fails (no empty slab, current segment
spent, new segment cannot be mapped, e.g. RLIMIT_AS), `take_elsewhere` visits each other shard once, taking that
shard's lock alone (own lock released first, never nested: lock order unchanged) and calling `take_held`, which
takes an empty or never-used slab and maps nothing. `#[cold]`, failure path only; the success path has no new lock.

## Why a block from another shard frees correctly
`adopt` already records the segment's home shard in the slab head ("the home shard is the segment's, whichever shard
handed the slab out"), and `release_slab` returns an emptied slab to that home shard. The borrowing thread's cache
owns the slab while in use; remote frees use the owner cache as for any slab. So nothing new was needed on free.

## Tests
`a_shard_that_cannot_map_takes_free_slabs_of_another_shard` (crates/rt/tests/heap_and_sync.rs): shard 1 has a cache
but no segment, limit leaves no room; small, odd-class, aligned (64, 256) and MAX_SMALL requests are served from
shard 0's segment without mapping; frees go home; a 16 MiB request still fails. Plants: nofallback, ownshard, skipfirst.

## Run 2 (HEAP-SHARD-FALLBACK-2, branch task/heap-shard-fallback-2)
- gate3's failure (unused binding in the test) was already fixed by a4bba7ef; gate on the final head is green.
- New: `first_cache` falls back to `unpark_elsewhere` (a parked cache of another shard, each shard once under its own lock,
  `#[cold]`) when `new_cache` fails (cache chunk or table cannot be mapped). The cache keeps its recorded shard and parks
  back there; `refill` uses the calling thread's shard, so blocks and frees are unaffected. If no shard has a parked cache
  the allocation still fails (bounded). Test `a_thread_whose_cache_cannot_be_made_takes_a_parked_cache_of_another_shard`.
- Plants (RAM copy, baseline green first): nocachefallback, cacheownshard, cachefirstonly, nofallback, skipfirst: all CAUGHT.
- Counts (kanon, RAM copies of origin/next heap vs new, `cargo xtask bench --compare --counters cbench_malloc`): the
  instruction columns are identical (malloc_free_16 122.1 both; other rows differ by <=0.7 instr in 11339, i.e. noise).
- REGEX-ENOMEM reproduction (mei, 2 nodes, tree without the test's CPU pin, load 14-16): base heap 0/400 failures, fixed
  heap 0/400. The reproduction did NOT trigger on the base heap in this run, so it neither proves nor disproves the fix;
  the deterministic unit tests are the proof. Nothing closed on the flake story from this.

## Not covered / open
- Large blocks need their own mapping; no fallback exists for them (nothing to borrow).
- Parked caches only: a live cache of another shard is never borrowed.

## Next step
Coordinator: read the JOBS line (gate, plants, counts, repro), then land.
