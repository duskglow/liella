# TIMING-COV-REALLOC-GROW-NONAVX2 (realloc_grow_x2): one lookup per moving realloc

State: branch task/timing-cov-realloc-grow-nonavx2. Code head 96b75b97. Later commits change only handoff/ files.
Base: origin/next 41b8737b. TIMING-IVB-MALLOC-CHAIN-2 and PERF-REALLOC-USABLE-2/-3 are already in it.
Role: kanon:~/w/kn1 (Broadwell-EP).

## Profile (step 1, base 41b8737b, wl_trace, job timing-cov-realloc-grow-trace0, AVX2 dispatch)
Calls 0 to 7 of one iteration request 32, 64, ... 4096 bytes:
- 32 B (moves, fused path): 201 instructions. 59 in the realloc entry (the in-place test finds the block, fails),
  130 in realloc_general (Heap::resize finds the same block a second time, then moves it), 12 in memmove.
- 64 B (in place): 63 instructions, all in the entry.
- 512 B (moves): 221 instructions (59 + 130 + 32 memmove).
- 2048 B: 467 instructions. 265 in resize_slow: requests above FORECAST_CAP (1 KiB) took the general path (locate,
  general alloc, general free). The rest is 9 entry, 40 realloc_general and about 150 copy.
- 4096 B: 559 instructions (269 resize_slow, about 230 copy).

Reconcile: the bench row's ratio (IVB 1.11) is lower than the probe's (1.44 non-AVX2 on keke). The probe's per-call
base (malloc + free + loop, divided by 8) is the same shape as the row. The likely cause is that copy cost differs by
host and dispatch. This is NOT VERIFIED: the bench row was not run on IVB in this story.

## Change (REALLOC-ONE-LOOKUP; crates/rt/src/heap.rs, crates/boundary/src/heap.rs, posix/s03_memory.rs)
- `Heap::fit(pages, addr, size) -> Fit`. It returns `Same(usable)`, `Held(Held{class, index})` or `Other`, and replaces
  `fits_in_place`. It finds the block once, through the slab hint. It covers `0 < size <= MAX_SMALL` (was <= 1 KiB).
- Rule for staying in place: up to 1 KiB the HALF_BELOW rule is kept unchanged (PERF-REALLOC-USABLE-3). Above that,
  `keeps_in_place` applies, the general path's own rule.
- `Heap::resize_held`: moves a Held block without a second lookup. It uses `alloc_hint` of the target class, copies,
  clears the old bit and calls `freed_into`. Any miss falls back to resize_slow.
- Target size: `move_target` is the general path's forecast rule, and resize_slow now uses it too (one copy).
- `Heap::resize` = fit, then resize_held or resize_slow.
- Size 0 inside resize (reached only through malloc_debug) now takes resize_slow. The result is the same: class 0 is
  kept in place, and any other class gets a class 0 block.
- Exports `realloc` and `__libc_realloc`: `Same` returns p with no stack frame. `Held` goes to `realloc_held` (inline
  never). `Other` goes to `realloc_general`.

## Results (instructions per realloc, wl_percall tc1106.c --cases realloc_grow_x2, kanon)
| dispatch | glibc | base 41b8737b | new 96b75b97 | new/glibc |
|---|---|---|---|---|
| AVX2 (timing-cov-realloc-grow-percall-avx2) | 199.5 | 252.9 | 209.8 | 1.05 |
| non-AVX2 (timing-cov-realloc-grow-percall-noavx2, hwcaps=-AVX2) | 199.5 | 288.8 | 245.7 | 1.23 |
The gap left on non-AVX2 is mostly the copy of the 1-2 KiB blocks (the non-AVX2 move kernel). This story did not
change the copy.

## Jobs (kanon)
- Gate timing-cov-realloc-grow-gate on 96b75b97: rc 0. 7 suites ok, 8810 passed, 0 failed, conformance 11/11,
  peak 14G.
- Plants: timing-cov-realloc-grow-plants3 (the file in handoff/TIMING-COV-REALLOC-GROW-NONAVX2-plants.txt). Four
  plants must be caught: copy one short, grow past usable, keep-big always true, alloc into the old class. One is
  marked equivalent with its reason: free-before-copy.
- Superseded: -plants and -plants2. In both, the baseline failed because the equivalent line had no cmd field. In
  -plants2 the role was busy, so the fixed file had not synced.
- Not run: the timing jobs nico:timing-cov-realloc-grow-time and nozomi:timing-cov-realloc-grow-n100. The budget ran
  out first.

## Open / next step
1. Book the timing hosts and time realloc_grow_x2 together with the cbench_malloc rows (staging recipe:
   handoff/timing-sweep-1105.md). The change adds no new instruction kinds (no vector, rep or division), so the
   instruction counts are expected to hold.
2. The remaining gap is the non-AVX2 copy of 1-2 KiB, about 36 per realloc compared with the AVX2 dispatch. Look at
   the non-AVX2 move kernel for mid sizes.
3. The in-place call (63 instructions, glibc about 48) is PERF-REALLOC-USABLE's row. Do not redo it here.
Decision: D-NEW (placeholder; REALLOC-ONE-LOOKUP as above). The coordinator reserves the id.
