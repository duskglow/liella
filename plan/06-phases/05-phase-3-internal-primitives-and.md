<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 3: Internal primitives and memory

* **Scope:**
  * §3.1 and §3.5 (60 symbols, part): the heap, the aligned forms, `malloc_usable_size`,
    obstack, `brk`/`sbrk`;
  * §3.2 introspection: the neutral subset (D-3.4).
* **Clauses:** about 30.
* **Needs:** P1, P2.
* **Builds:** IP-5 (futex locks), IP-6 (page source), IP-7 (the heap algorithm in
  `liella-rt`; the `GlobalAlloc` shim in `boundary::heap`, category *allocator*;
  `TryVec`/`TryBox`; allocation domains), and `boundary::caller_alloc`.
* **Design:** size-class heap over `mmap`. Per-thread caches are enabled only after the
  first thread (P5). Size-aware free is used internally (D-3.3), and C `free(p)` looks the
  size up from the size class. There are no in-band headers on internal allocations.
* **Test surface:**
  * The alignment and zero-fill contracts.
  * `calloc` overflow and requests above `PTRDIFF_MAX`, which give `ENOMEM`.
  * `realloc(NULL, n)` and `realloc(p, 0)` (D-3.2), and a failed `realloc` keeping the
    original block.
  * `posix_memalign` returning an error number and leaving `*p` untouched.
  * `errno` preserved by `free`.
  * Fault injection at every page request.
  * Cross-thread free (stress tests arrive with P6).
  * Double-free and invalid-free detection (a defined abort, S4).
* **Exit:** §3.1/§3.5 green. The allocator passes a randomised allocation-trace fuzzer.
* **Delivered** (exit met, with the deviations listed):
  * **Exports:** 59 of the 60 §3 symbols, each a single-expression translation (`cargo xtask
    purity` clean), over `liella_legacy::malloc_compat` and `liella_legacy::obstack`. The
    hooks, `__curbrk`, `obstack_exit_failure` and `obstack_alloc_failed_handler` are data
    objects in `boundary::data`. Not exported: `malloc_info`, which writes to a `FILE`
    (phase 8).
  * **Heap (IP-7):** `rt::heap` is safe Rust over addresses, with all state in atomics,
    sharded per NUMA node after the review below (D-3.10; IP-5's `RawMutex`, `RawOnce`,
    `RawRwLock`, `RawCondvar` are in `rt::sync`).
    Blocks up to 64 KiB come from 44 size classes in 256 KiB slabs, 63 slabs per 16 MiB
    segment, with out-of-band records and free bitmaps at the segment's start (no in-band
    headers, D-3.3). Larger blocks get their own mapping, recorded in an open-addressing
    table, and grow with `mremap`. Invalid and double frees are detected exactly and abort
    with glibc's messages (S4). The memory source is the `Pages` trait;
    `boundary::heap::KernelPages` implements it with `mmap` (never `brk`, D-3.7), and
    `boundary::heap::Liella` is the `GlobalAlloc` shim (category *allocator*: 4 sites).
  * **Fallible allocation (S8):** `liella::mem::{TryVec, TryBox}` and the obstack-semantics
    `Arena` (D-3.6). `clippy.toml` bans the infallible `alloc` methods, denied in every
    library crate root (D-3.9).
  * **Tests:** 59 translation tests and 43 clause tests for §3. `liella-rt`'s own suite runs
    the heap against a fake page source: fault injection at every page request, a
    60,000-step randomised trace fuzzer (no overlap, alignment, usable size, statistics), and
    misuse detection. It also runs the four sync primitives under real threads. Aborts,
    exits, resource limits and heap statistics run in a re-executed copy of the test binary.
    Full workspace: 1,398 passed, 0 failed, both plain and with `--all-features`. The latter
    interposes the exports on the test processes themselves, so the test harness runs on
    Liella's `malloc` and `memcpy` (D-4.13). `cargo clippy --workspace --all-targets
    --all-features` is clean.
  * **Pending clauses:** 3: `malloc_info` (phase 8) and the two `alloca` clauses (a compiler
    builtin, D-3.8).
  * **Found and fixed:** with the `export` feature, every phase-2 test binary built with the
    exports crashed at startup. Compiler-generated `memcpy` calls inside the `memcpy` export
    path bound to that export and recursed. Phase 2 had only run the tests without the
    feature. D-4.13 records the fix, and `--all-features` is now part of the test gate.
  * **Deviations from the plan as written:**
    1. **Pages are owned by address.** `KernelPages` keeps no `Region`: the `Region` from
       `mmap` is leaked on purpose, and unmapping, discarding and remapping go through
       `boundary::raw` (allocator category). A safe heap in a `static` cannot hold
       `Region`s, and the heap's records already identify every mapping exactly.
    2. **No per-thread caches** until threads exist (phase 6; the design enables them only after
       the first thread), and every thread uses shard 0 until it knows its node. `fork` resets
       the heap locks with the real `fork` (phase 6, F54).
    3. **Memory goes back to the kernel** when a large block is freed and on `malloc_trim`
       (`MADV_DONTNEED` on empty slabs). There is no automatic trim threshold yet.
    4. **Capacity limits** (after the NUMA review): 16 TiB of small blocks and 786,432 live
       large blocks. Beyond either, allocation fails with `ENOMEM`.
    5. **Size-aware internal free** (D-3.3) is not a separate path yet: `GlobalAlloc::dealloc`
       looks the size up from the size-class records, like `free`. This affects speed only.
    6. **The obstack failure handler** exits with `exit_group`, without `atexit` handlers,
       until `exit` exists (phase 4). The C obstack protocol works on caller memory directly
       (`legacy::obstack`), not over `Arena`: the `<obstack.h>` macros own that memory.
    7. **`#[global_allocator]`** is set by the product crate (phase 4). Until then Rust code in
       the test processes allocates through the host allocator, or through Liella's exports
       when they interpose.

  * **NUMA review** (owner request, 2026-10-01; changes made before phase 4). The review asked
    whether the phase-3 structures would be expensive to make multi-core and NUMA-aware once
    threads exist. Four of them were, and they were changed now:
    1. *Global free lists behind one lock*, with no owner in the slab records. Per-thread or
       per-node caches would have needed a new record layout and a new free path. **Now:** 64
       cache-line-separated shards, and the owner shard is in every slab record.
    2. *Address lookup* through a sorted array, shifted on insert and valid only under the
       global lock. Frees from other nodes would have needed that lock. **Now:** segments are
       aligned to 16 MiB and found through an insert-only hash, read without locks.
    3. *Plain loads and stores on the bitmaps and use counts.* Lock-free remote frees would
       have raced. **Now:** atomic read-modify-write (claims with `fetch_or`, frees with
       `fetch_and`, which also detects double frees), and the use count has its own word.
    4. *`mmap` assumed flat memory.* **Now:** `Pages::map` takes a node, and `KernelPages`
       applies `mbind(MPOL_PREFERRED)`; mappings are aligned by over-mapping and trimming.

    Neutral, and kept: the segment and slab geometry, the size classes, the out-of-band
    records (each segment's metadata lies in its own first pages, so it lands on the same
    node), and the large-block table, now behind its own lock and sized 1M slots.

    Validation: the placement, ownership and registry paths are tested against the fake page
    source (12 heap tests). On the real kernel, a test checks alignment, and checks the node
    whenever the kernel supports NUMA. This WSL2 kernel is built without `CONFIG_NUMA` (`mbind`
    returns `ENOSYS`, which the heap ignores), so real placement is untested on this machine.
    Testing it needs a NUMA-enabled kernel, for example a QEMU guest with `-numa` nodes.

    Cost: allocator speed against glibc is in `benchmarks/progress.md`, entry 1: 8× slower
    for a single thread, 700× under 16-thread contention. The layout is ready for the phase-6
    caches; the speed is not there yet.
