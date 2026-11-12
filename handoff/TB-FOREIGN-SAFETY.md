# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-8` (run 8; continues -7; origin/next 9042cc44f merged in 100c3b9d6, allowed by
the brief). Code head 4d6206d76 (then handoff/report commits). Decision in force: **D-X.58**
(records/decisions/D-X/D-X.58.toml; rulings #4212, #4213, #4214, #4241, #4253, #4287, #4779, #4828, #4888, #4889).
**Ruling #4779 is met on the Release build (#4889): +2 instructions per call on mb_cur_max, toupper, iswalpha and
towupper against next's tip.**

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (csys/src/machine/points.S, `.hidden`, now in its own
`.data.liella_machine_owned`), set by `init_initial` (crates/boundary/src/thread.rs) when the runtime started the
process. Rust reads the crate-private mirror `thread::OWNED_HERE` through `thread::owned()`, now a **plain read**
(`*OWNED_HERE.as_ptr()`, unsafe category *process-flag*, owner ruling #4888), so the gate is one `cmpb`. In a process
another C library started, `thread::words()` / `foreign_words()` give the thread's registry words. Gated sites: see
-6's table (unchanged).

## Run 8
- Step 0 on the inherited head 555a7ca52: clippy + static green (keke tb-foreign-safety-8-lint-0).
- 100c3b9d6: merge origin/next. Conflicts: ld.rs (both sides kept), hot-text.list and startup-data.keys (next's
  taken, hot-lists lane).
- 23806b6fe: **process-flag**. `owned()` reads `OWNED_HERE` non-atomically; xtask/src/policy.rs knows the category,
  permits it only in `src/thread.rs` and only with the body `*OWNED_HERE.as_ptr()`; docs/architecture.md S2 row; the
  D-X.58 body records the category, the write-before-first-thread argument and #4779/#4889. Recorded in D-X.58 (this
  story's own record, not landed yet) instead of a new D-NEW record.
- e27ef6381: next's `startup-state.list` named `thread::OWNED` (gone on this branch): now `OWNED_HERE` and
  `symbol liella_machine_owned` (both written at start-up), the asm byte in a section of its own.
- 4d6206d76: `hot-text.absent` gets five keys this branch's code inlines in one profile (fast: locale publish_global,
  iconv convert/load, nscache real_secs; release: regex ThreadScratch::table). next's hot-text.list untouched.
- Jobs (keke, role n1): gate-3, plants-4, percall-3 (Fast), percall-rel-new3 + percall-rel-base2 (Release). Superseded:
  gate-1/-2, plants-1/-2, percall-1/-2, percall-rel-new/-new2 (xtask so refused the merged lists, fixed above),
  percall-rel-base (wl_percall refuses a dir that is neither git tree nor role dir), plants-3 (process-flag-elsewhere
  WARN-NO-MSG: the planted read did not compile, so static stopped before the audit; plant fixed for plants-4).
- Release per-call counts without tooling: `LIELLA_FULL_PROFILE=1 wl_percall.sh --rev <sha>-release --base none`
  (products cached under `<sha>-release`, no Fast cache touched); next's tip counted from a scratch copy of the primed
  `~/wl/src/9042cc44f...` with a minimal `.synced` marker (~/work/TB-FOREIGN-SAFETY-8/start2.sh on keke).

## Counts (instructions:u per call, validation/percall/locale.c, keke, base = next 9042cc44f)
| case | glibc | next Release | run 8 Release | delta | next Fast | run 8 Fast | delta |
|---|---|---|---|---|---|---|---|
| mb_cur_max | 15 | 12 | 14 | **+2** | 12 | 15 | +3 |
| toupper_ascii | 24 | 24 | 26 | **+2** | 24 | 27 | +3 |
| iswalpha_2byte | 51 | 30 | 32 | **+2** | 29 | 32 | +3 |
| towupper_2byte | 41 | 29 | 31 | **+2** | 28 | 31 | +3 |
| wcwidth_2byte | 45 | 48 | 50 | +2 | 50 | 53 | +3 |
| wcrtomb_2byte | 244 | 91 | 97 | +6 | 111 | 117 | +6 |
| mbrtowc_ascii / _2byte | 211 / 234 | 42 / 56 | 53 / 68 | +11 / +12 | 42 / 56 | 54 / 69 | +12 / +13 |
| mbstowcs_short | 452 | 562 | 572 | +10 | 578 | 591 | +13 |
| regexec_short | 8407 | 1272.7 | 1263.7 | -9 | 1248.6 | 1261.6 | +13 |
Fast keeps +1 over Release (the GOT load of an exported symbol, -7's note). The `cmpb` was not re-checked in the
disassembly this run (the objdump grep found no symbol); the +2 counts are the evidence.

## Exact next step
Land. Follow-ups (not this story): mbrtowc/mbstowcs/wcrtomb read `owned()` several times per call (+6..+12): hoist one
per entry if #4779 is extended to them. wl_prep's Release profile option (#4889 follow-up). DL-13's
`__liella_tls_init`/`release` need the same gate and foreign-block test when they land (#4503, #4781).

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.
- ld.so: in a process it started, every thread block is taken as its runtime's.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). An atomic byte load never folds into `cmpb`.
- One `with_view` closure shared by many entries is outlined; use a cold function per helper (`isw_int`, `wtrans`).
- A plant that plants code into another file must compile there, or `cargo xtask static` stops at the build.
- Never `--allow-drop` a key `xtask so` does not call dead (start::program, run 5).
