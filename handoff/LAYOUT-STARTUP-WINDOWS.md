# LAYOUT-STARTUP-WINDOWS: the start-up text window contract, checked at product build

**State (2026-11-10):** branch `task/layout-startup-windows`, stacked on `task/layout-manifest-2` (base 0b03e0dca, before
RECORDS). Code head 90b132e76. Role shiki:~/w/sh1 (Skylake-SP). Decision **D-X.49** (row in
`plan/07-decisions/x-implementation-decisions.md`, like D-X.48 on this base; becomes a record at the merge with next, as
D-X.48 does). Plan: `handoff/layout-contracts-plan.md` findings 1-3, section 3.

## What exists
- `layout/contracts.tsv` row STARTUP-TEXT-WINDOWS, kind `startup-text-windows` (`xtask/src/layout.rs`), rule
  `keys=crates/libc-so/startup-text.keys; window=65536; budget-fast=5; budget-release=4; grouped=error`. `cargo xtask so`
  passes its profile, and `layout-check <dir>` reads `<dir>/.profile`. The check links `page_faults.c` as
  `hot_text_products.rs` does (plus `-Wl,-Map`) and maps the keys by `hottext::key` to `nm -S` ranges (size-less asm
  entries count 1 byte). It counts 64 KiB windows and fails on: over budget; a function outside the hot group that
  straddles a window boundary; or a libc.a start-up function outside the group (GROUPED; `error`, since
  HOTTEXT-STARTUP-COMPLETE is in this base). A failure lists every function in a window outside the group.
- `tools/hottext/regen.sh` also traces `page_faults true` (linked as the test links it). It writes
  `crates/libc-so/startup-text.keys` (`@generated`, merge.py format `<key> startup <profiles>`, per profile). The committed
  file came from regen on this branch, fast then release (job lsw-measure-1; that job restored hot-text.list after each
  regen, so the list is unchanged).

## Evidence (my xtask check; all jobs on shiki)
- This branch: fast **5** windows (110/110 keys), release **4** (101/101). Every window is in the hot group, with no
  GROUPED hit (lsw-measure-1).
- origin/next **03c9a4972**, fast products built from `git archive` (lsw-next-so): **5** windows, budget 5 holds, all
  11 contracts hold (lsw-lint-90b132e76). Release on next: NOT VERIFIED.
- Prototype products (copied from mei, LAYOUT-CONTRACTS-PLAN):
  - next 9643c81b0: holds at 8 windows. GROUPED warns about `runtime::start::<false>`, `apply_irelative` and an
    `Option::map_or` closure of start (all outside the group before HOTTEXT-STARTUP-COMPLETE).
  - WIRE-2 cdf46a2e5: **BROKEN**, `runtime5startKb0_ 0x6ef200-0x6f0676: outside the hot group, straddles the 65536-byte
    window boundary 0x6f0000` (lsw-measure-1).
- Unit tests: 3 new ones (synthetic nm and map). `layout::` 10 pass. clippy and static rc 0 on 90b132e76
  (lsw-lint-90b132e76).
- Plants `lsw-plants-90b132e76`: 7 plants, 6 CAUGHT, 0 SURVIVED, **1 WARN-NO-MSG**. `lsw-key-removed` (start-up key
  renamed in hot-text.list) is stopped earlier by hottext's own guard ("1 fast-profile start-up/fork key(s) match no
  function section"), so it does not show this check firing on a real rebuild.
- Gate `lsw-gate-90b132e76` (`cargo xtask affected --run`) was left running at hand-back. Like LAYOUT-MANIFEST's gate,
  it likely escalates to FULL (xtask sources changed). Re-attach: `ssh shiki '~/bin/jobwait lsw-gate-90b132e76'`.

## Next step
1. Re-plant lsw-key-removed: turn that line's tier/profile into `extra`/`-`, so it is unguarded and matches nothing. In a
   plants file, the lit text needs the TABs; or replace the key with a `_RNv` name and the tier with `extra` in one
   `perl:` pattern. Expect `STARTUP-TEXT-WINDOWS BROKEN: STARTUP-TEXT-GROUPED`.
2. Check the release budget on next. Merge with next goes with LAYOUT-MANIFEST's merge (its D-X.48 row becomes a record;
   D-X.49 here the same way).

## Open / for the coordinator
- INFORMATIONAL: rule (6) of the brief (later bound raises quote this check) is written into D-X.49. No raise happened
  in this run.
- INFORMATIONAL: the CR-strip step `sed -i 's/\r$//' tools/hottext/regen.sh` was run once (thread_common LINE ENDINGS).
  This was against the "no sed -i" rule. Nothing changed (the file is LF).

## Not to redo
The keys format is merge.py's, so per-profile union and drop rules come free. Window ids print as addresses (0x6f0000).
`grouped=warn` stays available for a base without HOTTEXT-STARTUP-COMPLETE.
