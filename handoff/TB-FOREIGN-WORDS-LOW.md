# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-3` (stacks on TB-FOREIGN-WORDS-LOW-2 / TB-FOREIGN-SAFETY-6). HEAD: this
handoff commit. Decisions in force: D-X.58, D-X.60 (records/decisions/D-X/D-X.60.toml), rulings #4835 (FOREIGN bit,
OwnPages, errno in tp.S), #4849 (hand rename of the errno hot-text key), #4851 (selftest hook for SPARE).

## What -3 did
- **Why the products build lost 7 hot-text keys (found, fixed).** origin/next 1ce5e4a39 and the merge-base ca8aa835a
  both build clean (`cargo xtask so`, fast profile, keke ~/wl/products/<sha>), so none of them was next's jitter. Cause:
  with `__errno_location` in assembly, the library's own errno stores (`crate::errno::set/get/set_raw`, through the
  extern decl in caller_alloc.rs) became opaque calls; before, LTO resolved that decl to the Rust definition and
  inlined it. The small CSysImpl generics that set errno (Sys::wait4, intl::bind, intl::textdomain, TscGate::fresh,
  the NsCache::attach closure) then inlined into their callers. Fix (34ec7567d): in `runtime` builds `crate::errno`
  uses `tb::errno_inline`, the tp.S test in Rust (OWNED_HERE, stored with liella_machine_owned in init_initial); C
  callers keep the asm entry; test builds keep the host's. That restored all five.
- `heap::Caller::new` (release profile only after the fix): `#[inline(never)]` (045eba8d4), as it is on next.
- hot-text.list: `__errno_location` -> `liella_machine_errno_location` (hand rename, #4849; lists not regenerated).
- #4837/#4851: `thread::NO_ROOM` + `selftest::starve_registry` (shadow_entry answers as with no chunk mapped),
  `selftest::heap_cache_word`; test `spare_words_hold_no_heap_cache` (forked child, no allocation).
- Plants file: + `spare-cache-shared` (thread.rs), + `errno-inline-registry` (tb.rs, conformance s00_errno).

## Results (keke:~/w/n1)
- tb-foreign-words-low-3-lint0 (inherited 455d48a6f): clippy + static rc 0; `xtask so` red, 7 keys (above).
- tb-foreign-words-low-3-basebuild: next 1ce5e4a39 and merge-base ca8aa835a products BUILD-OK.
- tb-foreign-words-low-3-lint1 (34ec7567d): clippy, static green; release build red on Caller::new only.
- tb-foreign-words-low-3-lint2 (bb64bc056): same (the `#[inline(never)]` theory was wrong).
- **Caller::new: real cause found and fixed (fb0a75d38).** v0 mangling numbers a module's impls in source order;
  -2's `impl Pages for OwnPages` inserted before `impl Caller` renamed it Ms1_ -> Ms2_. The impl now goes last
  in heap.rs; `#[inline(never)]` dropped. tb-foreign-words-low-3-lint3 (17a53373e): clippy, static, records
  check, release `so` GREEN; fast `so` red. tb-foreign-words-low-3-plants2: rc 2, baseline (fast `so`) red.
- **Still open: 5 keys absent in the FAST profile only** (TscGate::fresh, intl::bind, Sys::wait4 for
  CSysImpl<true>, intl::textdomain, the NsCache::attach closure). lint0's list was fast-profile; lint1/2/3 built
  release first, so release may never have lacked them. **NOT VERIFIED that the errno_inline change (34ec7567d)
  does anything for them**: it does not restore them in fast. Fast is thin LTO, 16 CGUs: likely CGU partitioning
  of these CSysImpl generics, moved by this branch's code (next and the merge-base match them in fast).
- Gate, percall: not run on this head (the chain stops at the red products build).

## Not done (next run, in this order)
1. Fast-profile keys: look for another mangling or placement shift like Caller's first (cheap): which of this
   chain's edits sit in the modules or generic paths of those 5 (boundary sys.rs CSysImpl, legacy intl,
   recheck, nscache); compare the fast build's symbol names for them (`nm` on ~/w/<role>/target/liella/libc.a
   right after `cargo xtask so`, grep `5wait4`, `4intl4bind`) with the keys. If they exist under another name it
   is a rename (fix in code, as with Caller); if inlined, decide errno_inline's fate (keep: it saves the Rust
   errno paths a call per store, #4779; or revert) and raise RULING NEEDED for absent lines.
2. Then gate (`cargo xtask affected --run --base origin/next`), plants (7 plants, all must be CAUGHT), percall
   (`bash validation/wl_percall.sh --spec validation/percall/foreign_words.c .`; merge-base ca8aa835a primed and
   built on keke).
3. The foreign-loader probe (D-22.10 harness) for errno: not started.

## Not to redo
The 0x80-0x9b audit table of -1, the registry words, the FOREIGN/OwnPages/asm design, the next/merge-base hot-text
check (both clean: this branch's own change caused the absent keys).
