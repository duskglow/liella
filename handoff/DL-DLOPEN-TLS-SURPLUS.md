# DL-DLOPEN-TLS-SURPLUS (DL-16d): dlopen of objects with PT_TLS into the static surplus

**State (2026-11-13, run 2 = DL-DLOPEN-TLS-SURPLUS-2):** branch `task/dl-dlopen-tls-surplus-2` on origin/next
68e72ab32 (no merge needed: next had not moved). Code complete, compiled, clippy + static + records clean on
427931bf2; the new conformance program passes on the reference and on Liella (10 of 10 repeat runs).
Role keke:~/w/n1 (Zen 1). Gate, plants and the thread_create count were started on 427931bf2 at hand-back
(context checkpoint); their verdicts are for the dispatcher.

## Decisions in force
- Pending record `records/pending/DL-DLOPEN-TLS-SURPLUS-2-1.toml` (series D-27, amends D-27.20, D-27.36): the whole
  design (one lock = libc's thread-list lock; `__liella_thread_walk` LIELLA_PRIVATE; table and dtv room for 512
  modules; eager dtv fill + lazy repair in `tls_slow`; place before relocation, rollback in `unwind`).
- How plan §5 points 1 and 2 hold: `create_inner` runs `tls_init` (or the static image copy) inside the `LIST`
  critical section that links the thread (`link_locked`, inlined; `list_insert` kept for `init_initial`, same name,
  non-generic). `dlopen` walks the list under `LIST` and appends to `THREAD_MODS` in the walk's final callback
  (still under `LIST`). A racing creation is either listed before the walk (the walk fills it) or reads the
  appended table after it: exactly once. Threads created later read the table: every module.
- `__liella_thread_walk` is looked up only by the first `dlopen` that loads TLS (no start-up lookup).
- DL-16c (DL-DLOPEN-CONCURRENT) overlap: at `load_set`'s publication point this branch adds only the
  `publish_tls` call (plus `place_tls` before relocation and one `rollback` line in `unwind`).

## Code map (this story)
`crates/ld/src/tls.rs` (`Surplus`, `MODULES_MAX`, `Area::with_dtv_slots`), `crates/boundary/src/ld.rs`
(`THREAD_MODS` 512, `fill_static`, `publish_tls`, `tls_visit`, `tls_slow`, `module_id`, `initial_place`),
`crates/boundary/src/ld/dlopen.rs` (`place_tls`, `thread_walk`, `adopt_start` places, `unwind` rollback),
`crates/boundary/src/thread.rs` (`create_inner`, `link_locked`, `walk_threads`), export in
`posix/s01_startup.rs`, `xtask/src/so.rs` LIBC_PRIVATE, `crates/rt/src/start.rs` THREAD_WALK.
Tests: `conformance/c/s27_dlopen_tls_surplus.c` + helpers `dlt_tls dlt_desc dlt_leak dlt_big` (clause test
`s27_3_c_0` in crates/core/tests/s27_dynlink_clauses.rs), `crates/ld/tests/tls.rs`
`tls_surplus_first_fit_alignment_gap_reuse_and_rollback`. Plants: handoff/DL-DLOPEN-TLS-SURPLUS-plants.txt (7).

## Open questions / informational
- `dlsym` of `STT_TLS` in `dlopen`ed objects: DL-16b (`dlsym`) is not on origin/next, so it is not wired. Its
  answer is the object's `Obj.tls` (`Placed { module, tp_offset }`, set by `place_tls`): address = TP + offset +
  st_value for the calling thread. To do when DL-16b lands (or in DL-16b).
- `tls_slow` (lazy repair) is reached only when a thread's vector fell behind, which the eager walk prevents
  except in a race with a concurrent `__tls_get_addr`; no deterministic test reaches it (no plant on it).
- The race itself (fill under `LIST`) has no deterministic plant; the 32 racer threads check the result.
- Dynamic thread_create is not counted by wl_percall (static only); the dynamic path's change is the move of the
  existing `tls_init` call under the lock it already took for the insert.

## Not to redo
The design; the reading of DL-15; the Surplus maths (masks, power-of-two alignment checked first).

## Measured (keke, Zen 1; wl_percall static release products, instructions:u, sd 0; job -percall)
| case | glibc | base 68e72ab32 | 427931bf2 |
|---|---|---|---|
| thread_create_join | 2249.6 | 35940.1 | 35942.2 (**+2.1: over the brief's "equal or better"**) |
Plants so far: walk-skips-live-thread CAUGHT (T1 and the racers fail: the race window is exercised).

## Next step (exact)
1. Read the jobs on keke: `~/bin/jobwait dl-dlopen-tls-surplus-2-gate dl-dlopen-tls-surplus-2-plants`
   (logs ~/logs/<label>.log; plants verdicts at the end of the plants log). A red gate: fix, rerun only the
   failing suite. A SURVIVED plant: strengthen the test.
2. Take back the +2.1 on the static create path (profile first: `validation/wl_trace.sh` or a disassembly of
   `create_inner` on base vs head). Likely cause: `img` (4 words) now lives from the top of `create_inner` to the
   lock section (spills). Static programs never dlopen TLS, so the static image copy can stay at its old place
   (before the descriptor stores, outside the lock); only the loader call (`t.addr == 0`) needs the lock. Try that
   shape, keep `link_locked` inline, re-count; keep only a shape with new <= base.
3. Then rerun the plants (the code moves) and the gate on the final head.

## Plan rows to read
plan/ph14/dl-16-dlopen.md §5, §8 (DL-16d); D-27.20, D-27.36; the pending record above.
