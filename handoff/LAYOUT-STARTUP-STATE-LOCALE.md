# LAYOUT-STARTUP-STATE-LOCALE: deterministic start-up state order (#4923) and setlocale's objects (#4915)

**State (2026-11-12): INCOMPLETE, WIP (context checkpoint).** Branch `task/layout-startup-state-locale` on origin/next
`dcce89422` (LAYOUT-STARTUP-STATE-TIME landed; no merge needed). Role shiki:~/w/sh2 (rsynced, not a git tree).
Scratch on shiki: `~/work/LAYOUT-STARTUP-STATE-LOCALE/` (`diag.sh`, `branch.sh`, `pads.py` = STATE-TIME's;
saved products `next-{fast,release}` (origin/next) and `branch-{fast,release}` (the WIP commit's code)).
**Do not land:** the order test fails and the start-up probe `true` is +2 (below).

## Decisions in force
Ruling #4923 (order), D-X.70 (declared list), D-X.64 (clusters), D-X.69 (access check), owner jitter rule #4347.
No new decision record yet (the order rule probably wants one: `records new decision D-NEW --series D-X`).

## What the WIP commit does
1. `xtask/src/coldbss.rs`: `order_sections` permutes the declared sections among their own section-table slots
   in the extracted member (symbols incl. SHT_SYMTAB_SHNDX, reloc `sh_info`, groups, SHF_LINK_ORDER remapped;
   other `sh_link`s to a moved section refused), called from `rename()` before the objcopy rename;
   `state_order` = list position, then section name. Unit tests: `state_order` on shuffled headers, and
   `reordered_sections_keep_their_symbols_and_relocations` (cc -c, permute, link, run, `nm -n` order). Both pass
   (job lssl-branch, `cargo test -p xtask -- coldbss`: 3 passed).
2. `hot_text_products.rs`: the access check traces all ten probe modes (`STARTUP_PROBE_MODES`) and only flags
   library objects (`libc.a(`; the `strings` probe's own `.data.rel` was flagged). New
   `init_startup_state_follows_the_declared_order` (per object in the probe map, declared statics in list order).
3. `startup-state.list`: what the extended check named on next (job lssl-diag, both profiles identical):
   setlocale/printf tier after NSCACHE: `liella::locale::collate::ASCII_KEYS`, `intl::GENERATION`, `intl::SETTINGS`,
   `_IO_2_1_stdout_`, `stdout`; catalog tier at the end: intl `LOCK DOMAIN STRINGS BINDINGS _nl_domain_bindings
   CATALOGS ALIASES MEMO MEMO_NAME R_LAST R_NEXT R_PREV Host::language::LANGUAGE RESOLVED` (RESOLVED is 4096 bytes).
   With these the branch's access check passes in both profiles.
4. `handoff/LAYOUT-STARTUP-STATE-LOCALE-plants.txt`: two plants written, NOT run.

## Measurements (lssl-branch, pads.py, 19 pads, cells next-fast/branch-fast/next-release/branch-release)
- locale orig 50/49/51/50, bss 0x800 51/49/52/50: flat on the branch and 1 lower. tool orig 54/52/55/53,
  bss 0x800 55/52/56/53: flat, 2 lower. localtime, mktime, fork, float, strings, rebind equal.
- **`true` is worse at every pad: 11 -> 13 (fast), 10 -> 12 (release); 38 WORSE cells.** `.data1` alignment
  went 16384 -> 32768 (`so: start-up data cluster alignments`), so the cluster crossed 16 KiB.
- **The order test fails** (both profiles): (a) csys_7.o (C): `__tzname` (entry 121) has a lower section index than
  `environ` (entry 24) in the final libc.a; (b) the Rust object: `stdout` lies before entries 111-116 inside ONE
  merged `.data1` section (release, symbol offsets 0x29b0 vs 0x29b8).

## Open / exact next step
1. Find why the order does not survive. (b): `copies::merge_codegen_units` (`ld -r`, runs after
   `move_startup_state` in so.rs) merges all `.data1` inputs into one section in member order. It joins whenever two
   `c-`/`liella_mem-` members exist, release included. Fix: run `move_startup_state` (rename and order) after
   `direct_internal_copies`. (a): `order_sections` either found the C member already ordered or a later objcopy
   reordered it. Check with the raw member: readelf before, after `order_sections`, after objcopy.
2. `true` +2: check whether it comes from the 32 KiB alignment or the new catalog tier (RESOLVED 4 KiB). Measure
   without the catalog tier, and the order alone (list unchanged). Then trace `true` with perf -d over `.data1`
   pages. The tiers' list order may not be first-touch order, so the fix may be to reorder the list.
3. Then: plants (`cargo xtask plants`, the file above), clippy, static, the direct gate, pads again (WORSE 0).
   Proving "code-only change leaves counts identical" with the lgammaf branch is still undone.

## Jobs (shiki)
lssl-diag (next + extended check: names the objects above; rc 1 expected), lssl-branch (WIP code: xtask tests
pass, order test FAILED, pads 38 WORSE; rc 1). Neither proves a final head.
