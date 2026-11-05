# PERF-MALLOC-LATEST-FREE-EXP: a "latest malloc" fast path in free (experiment). DO NOT LAND before the D-12 ruling

State: branch task/perf-malloc-latest-free-exp, code commit dd873a8e (on IVB-CYCLES 97347080, next e8a36c58);
role shiki:~/w/sh1. Register row added under the IVB-CYCLES row in docs/saturation/04-allocator.md.

## The change (crates/rt/src/heap.rs)

- Two cache words: `C_LATEST` (384) is the block the latest fast malloc (`alloc_hint`) returned, and `C_LATEST_AT`
  (385) is `(class + 1) | index << 8`, 0 for none. `CACHE_WORDS` goes from 384 to 392 (one more line).
- `alloc_hint` stores both (two stores; the index is already computed).
- `free_hint` first checks `addr == C_LATEST`. If it matches, `free_latest` takes the class, word and bit from
  `C_LATEST_AT`, loads the local word, tests the bit and the remote bit (same test as the other fast paths), clears
  the bit and calls `freed_into`. On any failure it returns false and the regular slab-hint, segment and slow paths
  run as before (they report the double free).
- `refill` clears `C_LATEST_AT` next to `K_CUR = 0`, before the class's slab stops being current.
- Test: `a_free_of_the_latest_fast_block_releases_that_block_only` (rt heap_and_sync): latest blocks in words 0 to 3
  freed on the fast path, every neighbour stays live, a second free reports DoubleFree, the block comes back. It also
  covers the latest block of a slab that is no longer current, with block 0 of the new slab live. The remote case is
  covered by the existing `the_free_fast_path_finds_each_blocks_own_slab` ("remotely, then by the owner").

## Measured (probe = the IVB-CYCLES pair probe; base binary = the IVB-CYCLES base build of e8a36c58, same md5)

| host | size | base cyc | new cyc | glibc cyc | instr base / new / glibc | rs-full base / new / glibc |
|---|---|---|---|---|---|---|
| Ivy Bridge (nico, booking #28, idle), mean of 6 | 16 | 52.9 (51.3-52.2 + one 57.9) | 45.6 | 47.0 | 123.2 / 110.2 / 148.2 | 21.3 / 17.1 / 8.4 |
| | 64 | 51.5 | 45.3 | 44.8 | same | 19.7 / 16.5 / 6.0 |
| | 1024 | 51.5 | 45.4 | 46.4 | same | 19.7 / 16.8 / 7.7 |
| Skylake-SP (mei, unbooked, load 20-37), min of 6 | 16 / 64 / 1024 | 45.6 / 45.6 / 45.5 | 41.4 / 41.3 / 41.6 | 45.5 / 45.4 / 45.7 | 123.2 / 110.1 / 148.2 | n/a |

Jobs: nico:perf-malloc-latest-free-exp-ivb, mei:perf-malloc-latest-free-exp-skx2 (mei means are noisy: 45.7 to 57
for every binary; the min is the clean figure). Instructions fall by 13 per pair, not just cycles: the latest path
skips the slab-hint load, the class check, the FREE_VIEW load, the magic multiply and the `q*size == off`/`q < n`
checks, against about 4 added in malloc.

## Ruling for D-12 (RULING NEEDED; the coordinator takes it to the owner)

- Gain per CPU class: Ivy Bridge -6.2 cycles per pair (-12%; from 1.11-1.16x glibc to 0.97-1.01x); Skylake-SP -4.1
  cycles (-9%, now 0.91x glibc); both -13 instructions per pair (110 vs glibc 148).
- Cost on other calls: every fast malloc +2 stores (and an or/shift); every free that is not of the latest block
  +1 load, compare and branch (predictable in the pair shape, a likely mispredict source when frees alternate);
  cache +64 bytes (20 caches per 64 KiB chunk instead of 21). Workload counts (instructions:u, heap workloads) were
  started on the Zen 1 host (keke:perf-malloc-latest-free-exp-wlc-zen, queued at load 24) and not read in this run:
  the ruling should wait for them.
- Safety: the record is written and read only by the owning thread (cache words); a non-zero record always names
  the current slab of its class (`refill` clears it before `K_CUR` drops, so it cannot name a slab that may leave
  the cache); the free re-loads the word and tests the local and remote bits exactly as the other fast paths, and
  falls back to them on failure, so a double free (local or remote) is still reported. Plants (all caught):
  wrong word, wrong bit, no clear in `refill`, no liveness test, no remote test.
- Recommendation: adopt if the workload counts show no row slower than noise; otherwise keep it as a measured
  negative.

## Gate and plants (final code head dd873a8e)

- shiki:perf-malloc-latest-free-exp-check (cargo xtask clippy, static, heap_and_sync 25 passed) rc 0.
- shiki:perf-malloc-latest-free-exp-gate (direct tier, --base origin/next) rc 0, peak 11G.
- shiki:perf-malloc-latest-free-exp-plants (`cargo xtask plants`, /dev/shm/claude/PERF-MALLOC-LATEST-FREE-EXP/plants-1):
  5 CAUGHT, 0 WARN. Plants file: ~/work/PERF-MALLOC-LATEST-FREE-EXP/plants.txt on shiki.

## INFORMATIONAL

- shiki has no workload recipes (~/wl/recipes empty): `wl_count` there stops at `wl_prep: no link command for sort`
  (then `du`). Only keke and mei have them. The count therefore runs on keke from a copy of the synced tree in
  ~/work/PERF-MALLOC-LATEST-FREE-EXP/tree (rsync of the worktree, sync marker copied from sh1; the build happens
  there, outside a role). Superseded: shiki:perf-malloc-latest-free-exp-wlc, -wlc2, -wlc3 (base not primed, then
  no recipes), mei:perf-malloc-latest-free-exp-skx (an event Skylake-SP lacks; no counts).
- shiki is a Haswell (E5-2697 v3), so the AVX2 build host is Haswell, not Zen.

## Next step

Read keke:~/logs/perf-malloc-latest-free-exp-wlc-zen.log (wl_count, base e8a36c58); bring the ruling with those
numbers. If adopted: decision row D-12.x (reserved id), then land without the experiment wording.

Scratch: /home/claude/w/.scratch/PERF-MALLOC-LATEST-FREE-EXP/ (ab.sh, ab2.sh, plants.txt, binaries).
