# DL-TLS-THREADS (phase 14 DL-15): threads with TLS in dynamically linked programs

**State (2026-11-13, DL-TLS-THREADS-2):** branch `task/dl-tls-threads-2` (= `task/dl-tls-threads` 9769a5c36 plus
this run's handoff commits; no code change in this run). Code: 0460cc7fe (final code tree 1f09927be, which reverts
the cost fix bbe7c0530). Decision: records/pending/DL-TLS-THREADS-1.toml (D-NEW, series D-27, amends D-22.10 and
D-27.20; waits for the coordinator's id). Rulings #5285 (cost: land, fix forward in DL-TLS-THREADS-COST), #5286
(StartInfo interface: accepted), #5287 (FXSAVE trampoline: keep) are in force. Role this run: mei:~/w/s2.
Measurement report: handoff/DL-TLS-THREADS-2-report.md.

## What the branch does (unchanged since DL-TLS-THREADS)
- ld.so (`crates/boundary/src/ld.rs`) passes `tls_size`, `tls_align`, `tls_init`, `tls_release` in `StartInfo`;
  `thread_tls_init(tp)` copies every initial module's image, zeros to `p_memsz` and builds the dtv below TP.
- libc (`thread.rs`, `runtime.rs`) reserves the loader's area in the thread mapping and calls `tls_init` before the
  clone, `tls_release` after the destructors; without the interface D-22.10's `EAGAIN` stays.
- `R_X86_64_TLSDESC` applied; `csys/src/machine/tlsdesc.S` holds `_static` and the branch-free `_dynamic` trampoline.
- Tests: `crates/boundary/tests/dl_tls_threads.rs` (2 tests, reference and Liella), `crates/ld/tests/tls.rs`.

## Why dl-tls-threads-gate2 was red (settled in this run)
`hot_text_products init_data_clusters_hold_at_every_text_phase` (fast profile): tool 55 at the `cold 0x800` pad,
54 at the other nine. Reproduced on mei with mei-built products (and identical counts on keke from the same
products: the products decide, not the host). Cause: text-window jitter outside the hot group, not this change's
data. The tool mode runs four functions that are in no hot group on origin/next either (`text::builtin_charset`,
`CtypeFile::wide`, a `global_locale` `OnceCell::try_init` closure, `global_locale::setlocale_locked<CSysImpl,Glue>`).
This change's code growth reorders cold `.text` and puts `setlocale_locked` at 0x64f580; the 0x800 pad pushes it
across the 0x650000 64 KiB boundary (one more window). On origin/next the same pads give 55/56 at `cold 0x4000` and
`0x5000`, which the test does not use. **The branch merged with origin/next passes the test on mei in both
profiles** (all ten count phases equal: fast tool 54, release 56).

## Open
- Owner ruling on how #4347 (start-up layout jitter) applies to this story (asked by the coordinator). Until then:
  no regen loops, no moved sections, no bound change.
- DL-TLS-THREADS-COST (fix forward of #5285): re-applying bbe7c0530 alone makes `locale::publish_global` a dead
  hot-text key (its body moves into `thread::each_block::<publish_global::{closure}>`, outside the group): fast
  tool 54 -> 55 and locale 51 -> 52 at every phase. It needs the hot-text regen/repair in both profiles.
- DL-16d needs a live-thread list and a growable module table (`tls_init` reads the start-time table). Surplus bytes
  of a reused mapping are not cleared. `record_thread_tls` assumes TLS variant II.

## Do not redo
The cause analysis above, the four-tree pad sweep (scripts and results: mei and keke `~/work/DL-TLS-THREADS-2/`).

## Next step
Land as soon as the #4347 ruling allows (on the train it is merged with next, where the test passes on mei). Then
DL-TLS-THREADS-COST, then DL-16d / DL-19 on `StartInfo::tls_*` and `liella_machine_ld_tlsdesc_dynamic`.

## Plan rows to read
D-22.10, D-27.20, D-27.22, plan/ph14/dl-16-dlopen.md §5, handoff/layout-stability-rethink.md (method).
