# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-9` (run 9; continues -8 on its head 561c13e0b; origin/next 9042cc44f merged in
100c3b9d6 by run 8). Code head da75b81fe (then handoff/report commits). Decision in force: **D-X.58**
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

## Run 9 (fault bound: -8's gate-3 red, setlocale + printf 52 > 51)
- Step 0 on 561c13e0b: clippy + static green (mei tb-foreign-safety-9-lint-0).
- Fault map (scratch tool mei ~/work/TB-FOREIGN-SAFETY-9/faultmap.py: the test's link and relay harness, then
  `perf record -e page-faults -d` attributed to section + symbol), Skylake-SP host, Fast products, branch vs next
  9042cc44f (primed and built on mei with wl_prime_base + wl_prep): locale 52 vs 50. Text +2, .bss -1, other +1.
  The two new text pages: (1) `thread::each_block<publish_global closure>`: D-X.58's gated `words()` made
  `each_block` too big to inline; `publish_global` is inlined into setlocale (absent key, -8) and the out-of-line
  `each_block` copy sat in cold text. This story's own code. (2) `locale::name::candidates<Data, find_named
  archive closure>` (`...s3_0`): inlined into `find_named` on next, outlined on the branch. Its source is unchanged
  (inliner tipping from growth elsewhere); its v0 name does not parse in keys.py / hottext.rs `key`, so a key would
  keep byte-offset back-references (not build-stable).
- da75b81fe: one named hand edit of hot-text.list (ruling #4849): the `each_block<publish_global closure>` key in the
  locale tier after publish_global, profile fast (traced there). Result (same tool): locale 51 (bound 51), true 11,
  tool 55, fork 5, localtime 19, localtime+mktime 19, float 4, mktime/strings/rebind 0: all equal to next except
  locale +1 (the candidates page). Plant: handoff/TB-FOREIGN-SAFETY-9-plants.txt.
- Tried and dropped: `#[inline]` on `each_block` and `name::candidates` (locale 51, but `find_cached`, `time::load`,
  `after_fork_in_child` left the hot group: localtime 21 > 20, fork 6; and 3 dead keys for `cargo xtask
  hottext-repair`). Not committed.
- Instruction counts: no code change this run (one layout key), so -8's per-call counts stand (not re-measured).
- Jobs (mei, role s1): lint-0, so-1..so-4 (diagnosis builds), prep-next, repair-1 (on the dropped hint tree),
  lint-1, gate-1, plants-1 (the last three on da75b81fe).

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
Land (locale fault count at its bound, 51; next 50). Returning to 50 needs the `candidates ... s3_0` page placed:
teach the v0 key walker (keys.py and xtask/src/hottext.rs `key`, kept in step) closures nested in generic paths so
that name gets a build-stable key, then list it after `find_named` in the locale tier (or trace-at-build, LT-1/LT-4).
Other follow-ups (not this story): mbrtowc/mbstowcs/wcrtomb read `owned()` several times per call (+6..+12): hoist one
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
