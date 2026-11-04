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

## Not covered / open
- A new thread's cache chunk (`new_cache`) that cannot be mapped still fails; parked caches of other shards are not tried.
- Large blocks need their own mapping; no fallback exists for them (nothing to borrow).
- REGEX-ENOMEM-INTERMITTENT: not re-run here; the mechanism it found (other-node thread cannot map a 16 MiB segment
  with 6 MiB headroom) is what the fallback addresses only if another shard holds an unspent/empty slab. NOT VERIFIED
  against that test's reproduction.

## Next step
Coordinator: read the gate/plants/counts jobs (JOBS line), then land.
