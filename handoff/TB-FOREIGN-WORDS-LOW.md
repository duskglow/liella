# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-2` (stacks on TB-FOREIGN-SAFETY-6). HEAD: this handoff commit.
Decisions in force: D-X.58 (`thread::owned()`, `thread::words()`, registry), **D-X.60 (record now written,
records/decisions/D-X/D-X.60.toml)**, ruling #4835 (implemented in -2). **INCOMPLETE: do not land yet** (hot-text keys, below).

## What -2 did (commits 967677137, 6a6a8b8e7, b7a98418c, 2e198c752)
- Heap (ruling #4835): `malloc_debug::FOREIGN` (64) is set in `DEBUG`'s initial word in every build and cleared by
  `thread::init_initial` (`Debug::clear_foreign`). Paths reached with the word clear use the new `OwnPages`
  (heap.rs: delegates to `KernelPages`, but `cache_word` is the plain `%fs:0x90` load) and `OWN_CTX`: malloc_entry/
  malloc_general, free_entry/free_general, every plain `c_*` path, `Heap::fit`/`resize_held`. `KernelPages` keeps its
  `owned()` gate for the debug paths, Rust's allocator, process.rs, selftest. `Debug::begin` now does the
  `fetch_and` only when VIRGIN is set (a foreign process's operations all come through it).
- errno: `__errno_location` is assembly, csys/src/machine/tp.S `liella_machine_errno_location` (second label
  `__errno_location` under nested `#ifdef LIELLA_EXPORT` / `#ifdef LIELLA_RUNTIME`): `cmpb $0, liella_machine_owned`,
  `je liella_errno_rest` (Rust local export in s02_diagnostics.rs -> `thread::foreign_errno()`), else `%fs:0 + 0x80`.
  The Rust twin is `export! { asm ... }`; `tb::errno_location` calls the asm (so the tests run it). Declared in
  ffi.rs and liella_csys.h (K5).
- Plants file updated: errno plant moved to tp.S (anchored on the label); new `foreign-bit-unset`.
- validation/percall/foreign_words.c: malloc_free, calloc_free, errno_location cases for the counts.

## Results (mei:~/w/s1)
- tb-foreign-words-low-2-lint0 (inherited head 85c19e6): clippy + static rc 0.
- tb-foreign-words-low-2-lint1: static K5 (asm entry not in liella_csys.h) -> fixed in 6a6a8b8e7.
- tb-foreign-words-low-2-lint2 (6a6a8b8e7): clippy, static, tb_foreign_low_words 1/1, tb_foreign_words 1/1,
  legacy malloc_debug 5/5: rc 0.
- tb-foreign-words-low-2-gate (b7a98418c): rc 1, **0 tests ran**: the products build's hot-text check failed.
- tb-foreign-words-low-2-plants (b7a98418c): rc 2, baseline failed on the same check (no plant ran).
  7 hot-text keys absent from the build: `__errno_location` (now the asm section
  `.text.liella_machine_errno_location`), `heap::Caller::new` (startup), and 5 not obviously mine (TscGate::fresh,
  intl::bind, CSysImpl::wait4, intl::textdomain, NsCache::attach closure), probably inlining shifts from this change.

## Not done (next run, in this order)
1. Hot-text keys: the brief forbids regenerating the lists. Check the same products build on origin/next
   (are the 5 others absent there too? then it is layout jitter, `INCOMPLETE: LAYOUT-JITTER` rule). For
   `__errno_location` the list must name `.text.liella_machine_errno_location` instead (a rename, not a regen):
   **RULING NEEDED** whether a hand rename of that one key in the hot-text lists is allowed, or a hot-text.absent line.
   `Caller::new`: see whether it is now inlined (heap.rs `Caller` near debug_alloc).
2. Then gate (`cargo xtask records check && cargo xtask affected --run --base origin/next`) and plants
   (handoff/TB-FOREIGN-WORDS-LOW-plants.txt, all 5 CAUGHT) on the final head.
3. Counts: `bash validation/wl_percall.sh --spec validation/percall/foreign_words.c .` from the role (base primed:
   merge-base with origin/next). Target: malloc_free/calloc_free unchanged, errno_location +2 at most.
4. #4837 SPARE plant: not written. No test can reach SPARE (registry allocation failure) today; needs a
   selftest-only hook that makes `shadow_entry` fail, a test that a SPARE thread gets `cache_slot() == None` and a
   working errno, and a plant `(!is_spare(w)).then_some(&w.heap_cache)` -> `Some(&w.heap_cache)`.
5. The foreign-loader probe (D-22.10 harness) for errno.

## Not to redo
The 0x80-0x9b audit table of -1 (handoff history), the registry words, the FOREIGN/OwnPages/asm design.
