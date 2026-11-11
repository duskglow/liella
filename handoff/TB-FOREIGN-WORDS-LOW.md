# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low` (stacks on TB-FOREIGN-SAFETY-6, 70eeb1619). HEAD: the handoff commit on
top of the lint-fix commit. Decision: **D-X.60 reserved, record NOT yet created** (`cargo xtask records new decision D-X.60`).
Builds on D-X.58 (`thread::owned()`, `thread::words()`, registry). **INCOMPLETE: do not land yet** (see Not done).

## Audit of 0x80-0x9b (every reader/writer; no %fs:0x8x/0x9x in csys asm)
| Word | Site | Before | Now |
|---|---|---|---|
| 0x80 errno | tb.rs `errno_location` (`__errno_location`) | tp+0x80 always | `owned()` ? tp+0x80 : `thread::foreign_errno()` (registry word, stable per thread; SPARE = shared word) |
| 0x84 tid | thread.rs own_tid/fast_tid/own_tid_fast | gated by D-X.58 | unchanged |
| 0x84 tid | setxid.rs, thread.rs tid_of/recycle via `tb::listed` | | unchanged: listed blocks are always runtime-made (adopted threads are never `list_insert`ed) |
| 0x84 tid | runtime.rs `set_tid_address(&t.tid)` | | unchanged: only on the runtime-start path, which makes the process owned |
| 0x88 dtors | process.rs thread_atexit / run_thread_dtors | `tb::current()` | `thread::words()` |
| 0x90 heap cache | heap.rs KernelPages cache_slot / cache_word | block word | slot: `thread::heap_cache_slot()`; word: `owned()` ? %fs read : `foreign_heap_cache()` (cold) |
| 0x98 node | heap.rs node_hint | block word | `thread::words()` |
- `Pages::cache_slot` is now `Option<&AtomicUsize>` (rt/heap.rs): a thread on the shared SPARE words gets `None` and
  allocates with no cache (`cache()`/`first_cache` return None), so two threads never share a cache.
- `clear_words` (new thread on a reused registry entry) now also clears errno, tls_dtors (never run on another thread)
  and node; the heap cache word is KEPT (the dead thread's cache passes to the new one: one owner, not leaked).

## Results
- keke tbfwl-lint-2 (68d8eec): tb_foreign_low_words 1/1, tb_foreign_words 1/1, rt heap_and_sync 25/25. clippy failed
  on an unused `use crate::tb` (process.rs) and static on opaque-audit (direct malloc/free in the new test): both
  fixed in the final commit, lint NOT re-run on it.
- Plants: handoff/TB-FOREIGN-WORDS-LOW-plants.txt, 4 plants (errno, heap word, node, dtors gates removed), job
  keke:tb-foreign-words-low-plants-1 started on 68d8eec (code identical to final head except the import and a
  comment); result not read.

## Not done (next run, in this order)
1. `cargo xtask clippy` + `static` on the final head; `cargo xtask affected --run --base origin/next`.
2. Read plants-1; all 4 must be CAUGHT. No plant covers the SPARE (`None` slot) path: needs an ENOMEM injection.
3. Counts: errno (`__errno_location`) and malloc/free percall against the primed base. Expected cost: the
   `owned()` gate in `errno_location` and `cache_word`, which on TB-FOREIGN-SAFETY measured 4 instructions (GOT
   load of OWNED_HERE, no `cmpb` fold), so likely +3..+4 per call: the brief requires no movement.
4. D-X.60 record; the foreign-loader probe (D-22.10 harness) for errno.

## RULING NEEDED
- Zero-cost gate for malloc/free (recommend): fold "not owned" into the malloc-debug flags word (a FOREIGN bit set
  from the start in the products, cleared by `init_initial`): every C heap entry already tests `DEBUG.active()`, so
  own threads pay nothing; foreign processes take the general path with gated pages. Rust-internal heap users
  (rust_alloc, process.rs) then need the gated Pages. For errno: `__errno_location` in csys asm with
  `cmpb $0, liella_machine_owned(%rip)` (+2, the only Rust inline asm site rule forbids it in Rust).
