# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-10` (run 10; continues -9's head 6ac3a5bc7; origin/next a0c020ee9 merged in
e5f4b3df2). Code head b8ba3a3f5 (then handoff/report commits). **HELD: INCOMPLETE: LAYOUT-JITTER-CLASS** (coordinator
ruling 11-13: the story waits for trace-at-build, LAYOUT-RETHINK LT-1/LT-4). Decision in force: **D-X.58**
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

## Run 10 (merge with next a0c020ee9 and measure; no product code change)
- Step 0 on 6ac3a5bc7: clippy + static green (mei tb-foreign-safety-10-lint-0).
- e5f4b3df2 merge: conflict in crates/boundary/src/ld.rs only. next (DL-TLS-THREADS) moved `__libc_stack_end`
  below its new `thread_tls_init`/`thread_tls_release` hooks; kept `STARTED_HERE` (D-X.58), dropped the old
  `__libc_stack_end` copy. The new hooks write only blocks Liella's own C library mapped and are reachable only
  through the `StartInfo` this loader gives the program it started, so D-X.58's ownership condition holds for them
  without another gate (this covers the "DL-13 hooks need the gate" follow-up below for these two hooks).
- b8ba3a3f5: `xtask so` refused two stale fast lines of run 8 in hot-text.absent (iconv::load, nscache::real_secs:
  out-of-line again after the merge); removed.
- **Static red (needs a ruling, not fixed):** next's new pin check (hottext `check_pinned`) rejects run 9's hand-placed
  key `thread::each_block<publish_global closure>` (locale tier): not `#[inline(never)]`. The fixes the check offers
  are an inline attribute (coordinator 11-13: no inline hints) or a hot-text.unpinned line (a ratchet, #4997/#5024:
  lines may only be removed). The two rulings conflict here; left red, RULING NEEDED in the report.
- **Faults (Fast products, Skylake-SP host, mei ~/work/TB-FOREIGN-SAFETY-10/measure.sh = run 9's faultmap.py, branch
  vs next a0c020ee9 built by wl_prep):** locale **52 vs 50 (bound 51)**, tool 55 vs 54 (bound 56), float 3 vs 4,
  true 11, fork 5, localtime 18, localtime+mktime 18, mktime/strings/rebind 0: all equal otherwise. Locale text 10
  pages vs 8: on both heads `setlocale_locked`, the `global_locale::Files` closures and `find_cached` lie in cold text
  (outside the hot group); on next they share one cold page (0x68d), on the branch three (0x60c, 0x621, 0x681).
  The run-9 `candidates` page is gone (it is in the hot group now). Layout jitter of cold text, not chased (ruling).
- Merge proof (mei tb-foreign-safety-10-merge-tests, `affected --only`): ld tls/ldso_run/ldso_link 23/23,
  boundary dl_tls_threads/tb_foreign_words/heap_threads 11/11, core hot_text_products 23/24 (the locale bound, 52).
- No plants: no new product code (a merge resolution that drops a duplicate definition, and two list lines that
  `xtask so` itself checks).
- Jobs (mei, role s1): lint-0, prep-next, faults-1, merge-tests, clippy-2. Superseded: lint-1 (on e5f4b3df2: static
  red on the pin check, still red), gate-1 (on e5f4b3df2: escalated to FULL by the list files, stopped at the stale
  absent lines).

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

## Run 8 (short)
- 100c3b9d6 merge (ld.rs both kept; hot-text.list, startup-data.keys next's). 23806b6fe **process-flag** unsafe
  category (`owned()` = `*OWNED_HERE.as_ptr()`, only in src/thread.rs; policy.rs, docs/architecture.md S2, D-X.58
  body). e27ef6381 startup-state.list names `OWNED_HERE` and `symbol liella_machine_owned`. 4d6206d76 five
  hot-text.absent lines (two removed in run 10). Jobs keke n1: gate-3, plants-4, percall-3, percall-rel-new3/base2.
- Release per-call counts: `LIELLA_FULL_PROFILE=1 wl_percall.sh --rev <sha>-release --base none`; next counted from a
  scratch copy of the primed source with a minimal `.synced` (~/work/TB-FOREIGN-SAFETY-8/start2.sh on keke).

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
Held until trace-at-build is the default (LAYOUT-RETHINK LT-1/LT-4; coordinator ruling 11-13: no key-walker
extension, no inline hints). Then: merge next, rebuild, and let the traced layout place the setlocale path; re-run
measure.sh-style fault map (locale must be <= 51, the bound; next is at 50). The hand-placed `each_block`
key of run 9 then goes (the trace places it), which also clears the static pin check (or apply the ruling on
`hot-text.unpinned` if it must stay). Then the direct-tier gate (it escalates to FULL on list files) and land.
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
