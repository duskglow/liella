# DL-DLOPEN-TLS-SURPLUS (DL-16d): dlopen of objects with PT_TLS into the static surplus

**State (2026-11-13, run 1, CHECKPOINT at the context limit):** branch `task/dl-dlopen-tls-surplus`, WIP commit
on origin/next 68e72ab32. **Not compiled, no jobs run.** Role: keke:~/w/n1 (not synced yet).

## Facts found (DL-15 as landed on next)
- ld.so's module table was start-time only (`THREAD_MODS`, `OBJECTS_MAX` entries) and `GENERATION` never moved.
- The live-thread list is libc's (`crates/boundary/src/thread.rs`: `LIST` lock, `HEAD`, `list_insert`; the
  initial thread is listed by `init_initial`). `create_inner` calls the loader's `tls_init` (`LOADER_INIT`)
  BEFORE `list_insert` and outside `LIST`: that is the race of plan §5 point 1 (a create reading the old table
  while a dlopen walks the list without it).
- `list_insert` is a hot-text key (`crates/libc-so/hot-text.list:103`): keep it non-generic, same name.
- DL-16b (`dlsym`) is NOT on next (conformance programs still `PENDING-LIELLA: DL-16b`; StartInfo has no
  dlsym). So "dlsym of STT_TLS in dlopen'd modules" cannot be wired here; `tls_slow`'s table lookup (or a
  `pub(super)` wrapper over it) is what DL-16b's dlsym calls for a `STT_TLS` definition. Tests must reach
  the helper's code without dlsym: the helper's constructor calls a function the program exports
  (`LINK-EXPORT-DYNAMIC: yes`).

## Design (how points 1 and 2 hold)
1. **One lock, libc's `LIST`.** libc moves the TLS fill of a new thread (both the loader's `tls_init` call and
   the static image copy, so the static path keeps the same instructions) inside the `LIST` critical section of
   the insert (inline the insert's body there; keep `list_insert` as is for `init_initial`). ld.so's `dlopen`
   walks libc's list through a new `LIELLA_PRIVATE` export `__liella_thread_walk(cb, ctx)`: under `LIST`, it
   calls `cb(ctx, tp)` per listed thread (plus the caller if unlisted), then `cb(ctx, 0)` still under the lock.
   ld.so's `tls_visit` fills the new blocks (image + zeros) and the dtv slots eagerly per thread; on 0 it
   appends the modules to `THREAD_MODS` and bumps `GENERATION`. A racing create reads the table either before
   the walk (then it is listed, the walk fills it) or after (then `tls_init` fills it): exactly once.
2. **Growable published table:** `THREAD_MODS` now has `ltls::MODULES_MAX` (512) entries, appended only at
   the end of a walk (Release on `THREAD_MODS_LEN`); `tls_init`/`fill_static` read it, so later threads get
   every dlopen'd module.
3. **dtv:** capacity for 512 module slots reserved at start (`Area::with_dtv_slots`, address space only; only
   header + used slots are written). Slot = TP + offset. Eager fill for live threads; lazy repair in
   `tls_slow` (`__tls_get_addr` slow path: refill all slots from the table, then count, then generation).
4. TLSDESC: the existing static descriptor path works once `Obj.tls` is set for the new objects.

## Done in this commit (uncompiled)
- `crates/ld/src/tls.rs`: `Area.used`, `Area::with_dtv_slots`, `MODULES_MAX`, `Surplus<N>` (first fit nearest
  TP, alignment remainder kept, refuses align > TP alignment and variant I, `rollback(mark)`).
- `crates/ld/src/dlopen.rs`: `loadable` no longer refuses `PT_TLS`; `Refusal::StaticTls` is the placement
  failure text.
- `crates/boundary/src/ld.rs`: table of 512, `SURPLUS_LO/HI`, `module_words`, `dtv_slot`, `dtv_words`
  (atomic view), `init_block`, leaner `fill_static` (no big zeroed stack arrays; writes header + used slots),
  `NewTls`, `surplus()`, `module_id(k)`, `initial_place(image)`, `publish_tls(walk, &mut NewTls)`,
  `tls_visit`, `__tls_get_addr` atomic reads + `tls_slow`, dtv capacity in `link()`.

## Next steps (exact)
1. `cargo xtask clippy` on the role; fix (old `fill_dtv`/`Placed` imports may go unused; `pt` in
   crates/ld/src/dlopen.rs may be unused now).
2. libc `thread.rs`: in `create_inner` remove the early `if let Some(t) = img {...}` block; replace
   `list_insert(d)` by `{ let _g = LIST.lock(&CSys); <that block>; link_locked(d) }` with the insert's body as an
   `#[inline(always)] fn link_locked` that `list_insert` also uses. Add `pub(crate) extern "C" fn walk_threads`.
3. Export `__liella_thread_walk` (`export! { runtime "LIELLA_PRIVATE"; ... }` like `__liella_libc_early_init`
   in crates/boundary/src/posix/s01_startup.rs), add it to `LIBC_PRIVATE` in xtask/src/so.rs, add
   `THREAD_WALK` beside `EARLY_INIT` in crates/rt/src/start.rs.
4. `crates/boundary/src/ld/dlopen.rs` `load_set`: after `check_versions`, for each new object with `PT_TLS`:
   check `image_in_load`, `module_id(j)`, `l.surplus.place::<Native>(lo, hi, align, &m)` (Loader gets
   `surplus: ltls::Surplus<{ltls::MODULES_MAX}>` and `tls_mark`; `unwind` does `rollback(tls_mark)`), set
   `o.tls`, fill a `NewTls`; failure text `[path, ": ", Refusal::StaticTls.text()]`. Look up the walk once
   (start objects' syms, `Request::new(THREAD_WALK, Want::Version(NODE))`); none -> StaticTls. After
   `protect_relro`, before publication: `publish_tls(walk, &mut new)`. In `adopt_start` set
   `tls: initial_place(bias + p_vaddr)` for start objects with `PT_TLS`.
5. Tests: `crates/ld/tests/tls.rs` Surplus units (first fit, remainder, gap reuse, align > TP refused,
   rollback); conformance `conformance/c/s27_dlopen_tls_surplus.c` (+ helpers dlt_tls IE/GD/LD/bss,
   dlt_desc with `HELPER-FLAGS: dlt_desc -mtls-dialect=gnu2`, dlt_leak 1 KiB TLS + undefined function x80
   attempts, dlt_big 128 KiB IE for the exhaustion text; thread T1 before the dlopen, T2 after, a creator
   racing 32 creations); replace the `s27_3_c_0` stub in crates/core/tests/s27_dynlink_clauses.rs.
6. Plants (handoff/DL-DLOPEN-TLS-SURPLUS-plants.txt): skip other live threads in `walk_threads`
   (msg: the T1 check line), drop `rollback` in `unwind`, alignment remainder in `Surplus::place`.
7. Decision record: `cargo xtask records new decision D-NEW --series D-27 --story DL-DLOPEN-TLS-SURPLUS`
   (the walk interface, table/dtv capacity 512, eager+lazy dtv). Counts: percall thread_create (static path
   must be equal) and the start-up rows.

## Do not redo
The reading of DL-15's code above; the design (section "Design").

## Plan rows to read
plan/ph14/dl-16-dlopen.md §5, §8 (DL-16d); D-27.20, D-27.36; handoff/DL-TLS-THREADS.md.
