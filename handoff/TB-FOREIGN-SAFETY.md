# TB-FOREIGN-SAFETY: Liella's thread-block words never trusted or written in a block Liella did not make

State: branch `task/tb-foreign-safety-3` (run 3; continues `task/tb-foreign-safety-2`), origin/next 4070276c5 merged
(ruling #4253). HEAD: the handoff commit on top of 32eacf1ff. NOT READY: the head's fast `cargo xtask so` fails the
hot-text dead-key check (below).
Decision in force: **D-X.58** (records/decisions/D-X/D-X.58.toml; rulings #4212, #4213, #4214).

## The design (D-X.58, short)
Ownership = process-level byte `liella_machine_owned` (machine/points.S), set by `init_initial`; the asm entries
read/write/pread/pwrite/close test it (`cmpb` + `je`). Rust reads a crate-private mirror `thread::OWNED_HERE`, stored
beside it (run 3: one rip-relative `cmpb`; the extern byte cost a GOT load and `movzbl` too). Elsewhere
`thread::words()` gives the thread registry words (64-entry mmap chunks keyed by thread pointer + TID). Per word:
0x9c flags never read; 0xa0, 0xa8, 0xb0-0xc0, 0xc8, 0x128, 0x130 from the registry; 0x138 view never read.

## Run 3
- Inherited jobs (run 2, head 3902244d5): tbfs2-gate-2 green (11382 passed, 127 conformance), tbfs2-plants-2 7/7
  CAUGHT, 0 WARN: coordinator note #4254 (period plant survived, 2 WARN-NO-MSG) was already settled by plants-2.
- Merged origin/next; `cargo xtask codemap` regenerated docs/codemap/symbols.tsv (bb0df1770). clippy+static clean
  (tbfs3-lint-1).
- Dead hot-text keys after the merge (inlining moved): pruned with tools/hottext/prune.py from both profiles'
  hot-text.matched (iconv::load, iconv::codec_named, ThreadScratch::table; all extra tier) (02b4787e5).
- plants on 02b4787e5: tbfs3-plants-2 7/7 CAUGHT, 0 WARN.
- COUNTS (Skylake-SP host, `bench --compare --counters --runs 3 cbench_hot cbench_locale`; before = origin/next in a
  RAM copy, tbfs3-counts-base-1; after = 02b4787e5, tbfs3-counts-2): +6 instr per locale-word access, not +2:
  ctype_b_loc 12->18 (glibc 10), isalpha_c 23->29 (16), iswalpha_utf8 31->37, localeconv 56->62, nl_langinfo 26->32,
  strcasecmp_* +10, mbrtowc_utf8_* +13..+17, sys_close_ebadf +7.7, sys_read_1 +1.4, mutex +1/+2. Cause (objdump of
  __ctype_b_loc/isalpha): GOT load + movzbl + test + je for the extern byte, and the second cold call (foreign path)
  hoisted push/pop onto the hot path.
- FIX (82ad32ee1, NOT YET COUNTED): `owned()` reads `OWNED_HERE`; `ctype_word` gives 0 in a foreign process so the
  existing unstored path is the one cold call (`class_unstored`/`case_unstored` read the registry word first:
  `foreign_word_or_zero`); `ctype_slot` has one cold `ctype_slot_slow`. Expected +2 on each gated path.
- That rework made ThinLTO name a start-up static `.data.<sym>.0.llvm.<n>`; xtask coldbss stripped the part number
  before the suffix, so `so` failed "start-up state ... SYSINFO_EHDR (0 sections)". Fixed in xtask/src/coldbss.rs
  (32eacf1ff; unit test `startup_state_sections_are_found_once_each` extended, green in tbfs3-check-2).
- tbfs3-check-2 (32eacf1ff): clippy, static, the coldbss test green; `so` (fast) RED: 4 hot-text keys dead:
  `cpu::Features::detect<CSysImpl>` (STARTUP tier, fast), `locale::publish_global` (locale, fast+release),
  `nscache::real_secs<CSysImpl>` (tool), `NsCache::config_epoch<CSysImpl>` (extra).

## Exact next step
1. On mei:~/w/s1 build the release profile too (`LIELLA_FULL_PROFILE=1 cargo xtask so`, it writes
   target/product/release/hot-text.matched even when it fails), then run tools/hottext/prune.py with both profiles'
   matched files. Features::detect is a STARTUP key: prefer `tools/hottext/regen.sh . --build` (both profiles) so
   its inlined caller is traced into the list (startup page-fault bound, ALWAYS FORWARD); prune only the rest.
2. Then: tests tb_foreign_words + s07_locale_symbols, counts again (after in a RAM copy or the role; compare with
   tbfs3-counts-base-1's measure.tsv: /mnt/zram/claude/TB-FOREIGN-SAFETY-3/base/target/liella/bench/measure.tsv,
   join script ~/work/TB-FOREIGN-SAFETY-3/join.py), direct gate, plants (handoff/TB-FOREIGN-SAFETY-plants.txt; the
   ctype-word-ungated pattern still matches once). If the counts still show more than +2, profile one row first.
3. Add the test reference/counts to D-X.58 via a record amendment only if the coordinator asks (records only).

## Open
- RULING NEEDED (owner safety review): the gate's remaining +2 per locale-word access in owned processes is the
  design cost of D-X.58 (isalpha_c already 23 vs glibc 16 before it).
- Known limits (D-X.58 design): a dead foreign thread's resolver state is not freed on entry reuse; TID reuse at the
  same thread pointer looks like the same thread.

## Do not redo
- No ownership test inside the block can be safe (D-X.58 options). Plants 7/7 settled (#4254).
- Jobs on mei: tbfs3-codemap-1 (rc 128: a `git status` in a non-git role, codemap itself wrote fine), tbfs3-lint-1,
  tbfs3-plants-1 (baseline so red: dead keys, fixed), tbfs3-counts-1 (same), tbfs3-plants-2, tbfs3-counts-base-1,
  tbfs3-counts-2, tbfs3-gate-1 (stopped by me: stale head), tbfs3-check-1 (so red: coldbss, fixed), tbfs3-check-2.
