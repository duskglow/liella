# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-11` (run 11; continues -10's head e3cbaf738; origin/next d53c0d551, with
trace-at-build the default, merged in 408d989ed). Code head 408d989ed; plants file a3f2723cd (then handoff/report
commits). **Released and ready to land** pending the escalated gate (below). Decision in force: **D-X.58**
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

## Run 11 (released: trace-at-build is the default on next; merge, drop the hand key, gate)
- Step 0 on e3cbaf738 (mei tb-foreign-safety-11-lint-0, rc 1): clippy green; static red only on hottext's pin check
  for run 9's hand key `thread::each_block<publish_global closure>`, as run 10 recorded (cleared by the merge).
- -10's red verify (lint-0 tested a tree with next's dlf_* helpers) is moot: lint-1 below ran on the synced final code head.
- 408d989ed merge of next d53c0d551 (LAYOUT-TRACE-DEFAULT, DL-16 dlopen TLS). Conflicts: hot-text.list, next's
  file taken whole, so the hand key is gone (ruling #5344; no hot-text.unpinned line); ld.rs `__tls_get_addr`, next's
  atomic dtv view (`dtv_words` + `tls_slow`) under the branch's `STARTED_HERE` test; the comment names that test.
- Checked, not changed: next's `tls_visit` (dlopen's per-thread TLS publish) reads the listed block's `FLAG_OWN`.
  It is reached only through `publish_tls` with `__liella_thread_walk` looked up among the loader's start-time objects
  (`dlopen.rs` `thread_walk`), i.e. only in a process this loader linked, where D-X.58 takes every block as its
  runtime's (same reasoning as run 10's two hooks). If that ever becomes reachable another way, it needs the
  process-level gate and a foreign-block test (INFORMATIONAL in the report).
- mei tb-foreign-safety-11-lint-1 (workspace clippy --all-features, xtask clippy, xtask static) on 408d989ed: rc 0.
- mei tb-foreign-safety-11-merge-tests (`xtask so` + `affected --only` hot_text_products, tb_foreign_words,
  dl_tls_threads, heap_threads, ld tls/ldso_run/ldso_link/dlopen/dlopen_rules): 101 passed, 0 failed. **All 25
  hot_text_products pass, so the setlocale + printf bound (51) is met on the traced build** (run 10: 52, red).
- Plants (handoff/TB-FOREIGN-SAFETY-11-plants.txt, the two D-X.58 plants on the merged `__tls_get_addr`): mei
  tb-foreign-safety-11-plants-1: 2 CAUGHT, 0 SURVIVED, no WARN. Gate: mei tb-foreign-safety-11-gate-1 (`affected --run --base origin/next`, escalated
  to FULL by the list files and csys, as every run of this story).

## Runs 8-10 (short; full text in git history of this file on task/tb-foreign-safety-10)
- Run 8: merge with next, the **process-flag** unsafe category (`owned()` = `*OWNED_HERE.as_ptr()`, only in
  src/thread.rs), startup-state.list names `OWNED_HERE` and `symbol liella_machine_owned`, hot-text.absent lines.
  Release per-call counts: `LIELLA_FULL_PROFILE=1 wl_percall.sh --rev <sha>-release --base none` (table below).
- Run 9: fault map (mei ~/work/TB-FOREIGN-SAFETY-9/faultmap.py) found two cold text pages on the setlocale path: the
  outlined `thread::each_block<publish_global closure>` and an outlined `locale::name::candidates`; a hand key
  for the first (da75b81fe) brought locale to 51. `#[inline]` hints tried and dropped (other paths left the hot group).
- Run 10: merge with next a0c020ee9 (ld.rs: `STARTED_HERE` kept; the two DL-TLS-THREADS hooks are reachable only
  through this loader's `StartInfo`, so no extra gate), two stale hot-text.absent lines dropped (b8ba3a3f5). Static
  red on the hand key's pin check; locale 52 vs next 50 (bound 51), cold-text jitter: held for trace-at-build.

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
Land once mei tb-foreign-safety-11-gate-1 (escalated FULL) and plants-1 are green. If the gate is red, fix only a
failure this branch causes (run that suite alone); a start-up bound red on unchanged start-up code is the jitter rule's
`INCOMPLETE: LAYOUT-JITTER` with numbers, not a chase.
Other follow-ups (not this story): mbrtowc/mbstowcs/wcrtomb read `owned()` several times per call (+6..+12): hoist one
per entry if #4779 is extended to them. wl_prep's Release profile option (#4889 follow-up). Any later loader TLS
hook reachable other than through this loader's `StartInfo` needs the D-X.58 gate and a foreign-block test (#4503,
#4781); the two DL-TLS-THREADS hooks merged in run 10 are reachable only that way.

## Open (D-X.58 known limits)
- A dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the same thread pointer looks
  like the same thread.
- ld.so: in a process it started, every thread block is taken as its runtime's.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). An atomic byte load never folds into `cmpb`.
- One `with_view` closure shared by many entries is outlined; use a cold function per helper (`isw_int`, `wtrans`).
- A plant that plants code into another file must compile there, or `cargo xtask static` stops at the build.
- Never `--allow-drop` a key `xtask so` does not call dead (start::program, run 5).
