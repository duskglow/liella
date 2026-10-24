# HEAP-SMALL: the cost of a small malloc/free pair, and how to cut it

Task HEAP-SMALL-DESIGN, 2026-10-09 (branch design/heap-small off 1e3b191). Measurement and plan; one
pure-overhead cut made (section 4). Scratch programs: coordinator scratchpad work/heapsmall/ (hs.c, hs.sh,
trace.gdb).

## 1. Measurement (Zen 1, instructions:u, N-call delta, N = 200000, static Liella vs dynamic glibc 2.39)

Per pair (or per iteration where stated), main 1e3b191:

| pattern                                  | Liella | glibc | ratio |
|------------------------------------------|-------:|------:|------:|
| malloc(16/32/64/128) + free              |    163 |   150 | 1.09  |
| malloc(512) + free                       |    168 |   150 | 1.12  |
| calloc(1,32) + free                      |    214 |   333 | 0.64  |
| malloc 16, realloc 40, realloc 100, free |    931 |  1106 | 0.84  |
| mix3: malloc 32/48/32, free 48/32/32     |    513 |   447 | 1.15  |

Two threads (each its own loop): identical per-pair numbers on both sides (the per-thread cache, D-3.11,
takes no lock and no atomic read-modify-write on the warm path).

FINDING vs the WORDEXP-3 premise: a warm C pair is NOT ~230 instructions; it is 163, 9% above glibc's 150.
The ~230 seen inside wordexp comes from the paths around it, not from the pair itself:
* Rust's internal allocations (D-3.1: library-internal memory uses `GlobalAlloc` directly) have NO
  `GlobalAlloc::realloc` override: every `Vec`/`String` growth is `__rust_realloc` = alloc + memcpy +
  dealloc (disassembled in the product), never an in-place growth inside the block's size class.
* A free that is not of the latest allocation's slab takes the second fast path (segment check +
  `locate_in`): 97 instructions instead of 70 (mix3).

## 2. Attribution of one warm pair (gdb single-step of malloc(32) and free, Liella static)

malloc = 85 instructions, free = 70 (+8 loop = 163).

| layer                                                             | malloc | free |
|-------------------------------------------------------------------|-------:|-----:|
| export prologue/epilogue (push/pop, sret frame)                   |   ~10  |   ~3 |
| DEBUG word test (P6, D-3.5) + NULL test                           |     3  |    4 |
| class_for (size.max(1), MAX_SMALL, <=128 branch, align mask)      |   ~15  |    - |
| thread pointer: out-of-line `liella_machine_tp` call + load 0x90   |     4  |    4 |
| unused cache-id load (`cache_of` reads C_ID)                       |     1  |    1 |
| alloc_hint as a call: Option<Block> returned through memory,      |   ~12  |    - |
|   re-tested discriminant                                          |        |      |
| bounds checks (class words < 256, bitmap index, CLASSES[c])       |   ~10  |   ~4 |
| `% CLASSES.len()` on C_LAST's class (free)                        |     -  |    5 |
| three separate table loads (MAGIC, CLASSES, blocks)               |     -  |    3 |
| the algorithm itself (bitmap word, tzcnt/bts, address, C_LAST,    |   ~30  |  ~45 |
|   live + remote-bit double-free check, hint update)               |        |      |

Allocation-free layers: no lock, no atomic RMW, no zeroing on malloc (calloc zeroes only blocks that are
not fresh), no hardening check on malloc. The free's double-free check (local AND remote bit) is 4
instructions and is required (S6: O(1)-detectable misuse aborts).

LOWER BOUND for this design (bitmap slabs, out-of-band metadata): malloc ~25 (DEBUG test 2, class
lookup 3, TLS cache 1, slab/hint loads 3, bitmap word not/tzcnt/bts/store 5, address 3, C_LAST 1,
call overhead ~5), free ~25-30 (DEBUG/NULL 3, TLS 1, C_LAST range + magic division 6, live+remote bits 4,
store 2, hint 2, overhead ~5). Pair ~55 vs 155 now: 2.8x headroom in our own design, without touching
any hardening.

## 3. Bench rows that move (main_counters 1e3b191)

No cbench row measures a bare pair (bench_heap is a Rust bench without counters). Allocation-heavy rows
above parity: wordexp x8 (cbench_opt/patterns, 1.5-2.0x), regcomp_regexec_regfree_email (2.30x),
regcomp_regfree_ipv4 (1.07x), glob_1000_* (1.64x). Rows below parity that also allocate: iconv_open
(0.30), regcomp_sed_kv (0.47), newlocale_free (0.44). Estimate: cutting the pair from 163 to ~100 moves
a wordexp row by 5-10% (it makes ~10-20 allocations of 5-9k instructions); the realloc override moves
rows that grow Vecs (glob, regcomp, wordexp) more than the pair cut does. Neither alone brings wordexp
or glob to parity: their gap is mostly elsewhere (see WORDEXP-3 / glob rows).

## 4. Cuts

Made (pure overhead, no hardening change):
* C1 `Heap::alloc_hint` `#[inline(always)]` (was `#[inline]`, which LLVM declined): removes the call, the
  Option<Block> through memory and its re-test. Result (Zen 1, same method): pair 163 -> 155 (glibc 150),
  calloc+free 214 -> 184, realloc chain 931 -> 867, mix3 513 -> 489; malloc 85 -> 77 single-stepped.

Recommended next, each small and hardening-neutral (one task, HEAP-SMALL-CUTS, ~60 calls):
* C2 `GlobalAlloc::realloc` through `Heap::resize` (in-place inside the class; reserve blocks and
  alignments > ALIGN keep the default path) and `alloc_zeroed` using `Block::zeroed`. Largest win for
  internal Vec growth. Test: planted bug (resize copying len-1) must fail.
* C3 class_for by a 65-entry table for size <= 1024 (index (size+15)>>4): ~15 -> ~4 instructions.
* C4 thread pointer read inline (`mov %fs:0x90`) instead of the out-of-line csys call: needs one `asm!`
  in the boundary (S-category: existing machine/tp), ~3 instructions per call, malloc AND free.
* C5 free: class tables padded to 64 entries and `& 63` instead of `% 44`; MAGIC/size/blocks in one
  16-byte entry (one cache line, one index); ~6 instructions.
* C6 `Cache.w` as `&[AtomicU64; CACHE_WORDS]` and the record as a fixed array, so index checks fold away
  (the bitmap index `j` masked to BITMAP_WORDS-1, which every valid hint satisfies); ~10 instructions.
* C7 drop the C_ID load from the fast paths (`own_cache` returns only the words).

Expected after C1-C7: pair ~100-110 instructions (glibc 150), i.e. better than glibc, no rule changed.

## 5. Structural options and their security cost (RULING NEEDED where marked)

* S-A (recommended against): a glibc-style per-thread singly linked free list (tcache) in front of the
  slabs. Buys ~15-20 instructions over the post-C1-C7 path at best. Costs: in-band next pointers in freed
  blocks = cache-poisoning surface (needs pointer mangling, and still forgeable by a use-after-free
  write); double-free detection stops being O(1)-exact (a listed block must keep its bitmap bit set, or a
  slab walk would hand it out twice, so a second free of it finds the bit set and passes unless the
  list is searched);
  mallinfo (D-3.4) must count listed blocks as free; M_PERTURB/mcheck (D-3.5) must flush the lists;
  fork/thread exit must drain them. Would change D-3.3 (no in-band metadata) and S6 exactness:
  RULING NEEDED only if anyone proposes it; recommendation: do not, the bitmap cache is already a
  per-thread small-block cache without in-band metadata.
* S-B: a free fast path for "any slab this cache owns" (not only the latest/current one) via the owner
  word in the slab record: removes the 97-instruction second path for mixed sizes; no hardening change
  (the owner check is what free_slow does now). Part of HEAP-SMALL-CUTS.
* S-C: per-class "last slab" instead of one C_LAST word: the mix3 frees all hit the first path. One word
  per class in the cache (already 5 words per class; 44 x 8 bytes per thread more). No hardening change.

Considered and rejected: dropping the remote-bit test on the owner's free (it is the exact double-free
check against a remote free not yet collected: S6); skipping the DEBUG word (P6 already makes it one
test; mcheck/perturb depend on it).

## 6. HEAP-CUTS2 (2026-10-09, branch opt/heap-cuts2 off opt/heap-cuts eae2b24)

Done, each keeping every check of malloc/free (live + remote bit, S6) and every ruling as it was:
* S-C (as built): the one `C_LAST` word became a 16-entry table of slab hints in the cache's spare words
  (`C_SLABS` = 236..251, no size change), indexed by the slab address bits above `SLAB_SHIFT`; entry = base |
  class of a slab that is its class's current slab. Every allocation from a current slab writes its entry
  (as it wrote `C_LAST`); `refill` clears every entry of the class before the slab stops being current (the
  same single place `C_LAST` was cleared). A per-class word alone could not be used by `free` (it does not
  know the class); the address-indexed table can. Two current slabs in one entry: the later wins, the other's
  frees take the second path (slower, never wrong).
* C6: `Cache.w` is `&[AtomicU64; CACHE_WORDS]` and the fast paths' slab record `&[AtomicU64; RECORD_WORDS]`
  (`first_chunk`, folds when `Pages::words` is inlined); bitmap word indexes masked to `BITMAP_WORDS - 1`
  (every valid hint / `q < blocks` index satisfies it). One check remains: the free path's class index from a
  hint (`& 63`) can exceed the 44 classes' words.
* C4: `Pages::cache_word` (default: `cache_slot().load(Relaxed)`); the boundary reads it with
  `tb::read::<tb::HeapCache>()`, the existing S2 *tb-field* site (D-X.16: a marker per profiled hot reader).
  No new unsafe site; tb-field stays 1.
Skipped: S-B (a fast path for frees into an owned non-current slab): such a free must update the use count
and may list or release the slab (`after_free`), which is `free_slow`'s owner branch already; with S-C the
mixed-size pattern no longer reaches it, and the target was met without it.

Measured (Zen 1 instructions:u per op, hs.c N = 200000 delta, threads = 1, LIELLA_FULL_PROFILE=1 products):
| pattern           | eae2b24 | HEAP-CUTS2 | glibc |
| malloc(n) + free  |     148 |        136 |   150 |
| calloc(1,32)+free |     178 |        170 |   333 |
| realloc chain     |     870 |        728 |  1106 |
| mix3              |     471 |        405 |   447 |
(The HEAP-CUTS table's 158 / 502 were not release-profile products; the same tree gives 148 / 471 here.)


## 7. REALLOC-FUSE (2026-10-09, branch opt/realloc-fuse; decision D-3.11)

Done (crates/rt/src/heap.rs `Heap::resize` / `resize_slow` / `forecast`; boundary `c_realloc`):
* Fused small path: a block of a current slab of the calling thread's cache, resized to at most
  `FORECAST_CAP` (1 KiB), is found once through the slab hint (as `free`'s fast path finds it), checked live
  and not remotely freed (S6 checks kept), kept in place while its class fits and is not more than twice the
  need, else moved into a block from the target class's hint word with its old bit cleared and the class hint
  lowered - no second lookup. Any miss falls through to `resize_slow` (the former general path, unchanged).
* Growth forecast: a small block that moves upward gets the class for about twice the request (at most
  `FORECAST_CAP`), so a doubling caller finds the next step in place. Only realloc-grown blocks; malloc keeps
  its own class. Memory bound (class table arithmetic, not a workload measurement): slot <= 2.0x the plain
  slot, <= 2.82x the request, mean 1.62x the plain slot over requests 17..1024.
* `realloc(NULL, n)` is taken at the boundary as `malloc(n)`, before the resize path's frame.

Measured (Zen 1 instructions:u per op, hs2.c = hs.c + rdouble/rinplace/rnull, N = 200000 delta, threads = 1,
LIELLA_FULL_PROFILE=1 products; tree = opt/realloc-fuse merged with main 66571cf; base = the same tree with the
REALLOC-FUSE source changes reversed):
| pattern                          | base | REALLOC-FUSE | glibc |
| malloc(32)+free                  |  136 |          136 |   150 |
| calloc(1,32)+free                |  170 |          170 |   333 |
| realloc chain 16->40->100        |  728 |          524 |  1106 |
| mix3                             |  405 |          405 |   447 |
| rdouble 16->32->...->256 + free  | 1376 |          772 |  1149 |
| rinplace 20->24->30 + free       |  454 |          370 |   604 |
| rnull realloc(NULL,32)+free      |  153 |          144 |   177 |
Planted bugs (heap_and_sync, 7 plants: short copy, in place past the class, forecast on malloc, fused path
without the live check / remote bit, class hint not lowered, old bit kept): all 7 caught.
Side effect found by the gate: the forecast can open a slab of a class nothing else used, so a probe that
"exhausts the heap" with a few sizes may leave free blocks in that class; s23_4_f_0/enomem now exhausts every
size in 16-byte steps (expectation unchanged).
Next cut (not done): a moving realloc is ~190 instructions against a budget of ~45-50 (entry wrapper ~20,
compat realloc's frame with malloc/free inlined ~30, lookup ~40, alloc_hint ~35, copy call ~25): split the
fused path into its own leaf without the inlined malloc/free frame, returning the pointer in rax.
