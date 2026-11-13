# DL-LIBC-START-8 (continues DL-LIBC-START-7): merge with next, hot lists regenerated

State: branch `task/dl-libc-start-8` (on -7's head 8e053d309), role kanon:~/w/kn1. Code head 8c95bc90c.

## Why -7 was red
A CI merge conflict, not a fault: DL-LIBC-EARLY-6 (ld.rs, start.rs, so.rs) and STRTO-FENV-FLAGS-3 (hot-text.list,
startup-data.keys) landed while -7 was in flight.

## Commits
- 2112ed590 merge origin/next (b31418181). Both sides kept: `StartInfo` carries the program's `init`,
  `init_array`, `init_array_len` (D-27.24) and the loader's `rseq_size` (D-27.26); ld.so's `LD_EXPORTS` =
  `__tls_get_addr`, `__libc_stack_end`, `__rseq_offset/size/flags`. hot-text.list and startup-data.keys: next's.
- f8bb199ea the auto-merge kept two ld.so definitions of `__libc_stack_end` and two stores of `sp` (E0428). Kept
  the definition that is mangled under feature `runtime` (the C library's copy wins there) and EARLY's store (with
  the rseq objects, `Release`, before any relocation).
- 6eb94bfd6 `tools/hottext/regen.sh . --build` per profile (fast, then release; merge mode). Three keys place
  nothing in the merged tree: `getopt::next<CArgs, CLongs>` (fast-traced; `nm` of the fast libc.a shows only the
  argp instantiation; dropped with `--allow-drop`), `thread::list_insert` and `catalog::textdomain` (untraced
  `extra` keys, in neither profile's hot-text.matched; dropped by `tools/hottext/prune.py` given both files).
  Their hot-text.unpinned lines removed (the static check requires it; the ratchet allows removal).
  probe-text.keys (+2) and startup-data.keys (+1) as the regen wrote them.
- 8c95bc90c the fast trace also added five generic keys next's list does not have (locale `find_cached`,
  `attach_charmap`; recheck `TscGate::fresh`; catalog `variants::each`; intl `record_for`): none is
  `#[inline(never)]`, unpinned only shrinks, so `cargo xtask static` refused them. Removed by a script over the
  static check's own list. hot-text.list is now next's minus the three dead keys (no reorder, no other change).
- handoff commit (this file, the json, DL-LIBC-START.md).

## Jobs (kanon)
- dl-libc-start-8-lint0 rc 0: clippy + static on the inherited head (step 0).
- dl-libc-start-8-regen rc 1 (E0428 above), -regen2 rc 1 (getopt key refused), -regen3 rc 1 (fast list written;
  the post-check failed on list_insert/textdomain), -regen-rel rc 1 (release list written; same two keys). These
  are the regeneration steps, superseded by the head checks below.
- dl-libc-start-8-check rc 1 (static: the five unpinned keys), superseded.
- dl-libc-start-8-check2 rc 0 on 8c95bc90c: workspace clippy (all targets, all features), `cargo xtask clippy`,
  `cargo xtask static`, `LIELLA_FULL_PROFILE=1 cargo xtask so` and `cargo xtask so` (dead-key check enforced; all
  12 layout contracts hold).
- dl-libc-start-8-gate: `cargo xtask affected --run --tier full` (the brief asks for the FULL gate). Result: see the
  hand-back.
- dl-libc-start-8-plants: handoff/DL-LIBC-START-8-plants.txt (3 on the merge sites + -7's 3). Result: see hand-back.
- Counts: not run. The merge adds no behaviour beyond the two sides already measured (-6 counts, EARLY-6 counts).

## RULING NEEDED / INFORMATIONAL
- RULING NEEDED: `regen.sh` adds traced keys that `cargo xtask static` (check_pinned) then refuses, so every merge
  regen can go red on static. Recommend: merge.py skips (and names) a new Rust key that is neither
  `#[inline(never)]` nor in hot-text.unpinned, or a story pins the five keys above with counts.
- INFORMATIONAL: fast-profile inlining in liella_boundary moves with unrelated code in the same crate (three keys
  went dead, five generics became sections); the start-up layout jitter class (#4347).
- INFORMATIONAL: `StartInfo.rseq_size` -> `LOADER_RSEQ` has no test that can observe it while threads in dynamic
  programs are forbidden (D-22.10); a plant on it would survive. DL-14 threads should add one.
- #4807 (Liella's own libc_nonshared.a): ACCEPTED by the owner, DL-LIBC-NONSHARED queued.

## Refusals
None. Slip, for the record: one shell call carried a stray `sed -i.bak -n '1p' /dev/null` (no file of the tree or
the role touched; the rules forbid `sed -i`, every edit in this run went through Edit/Write or a scripted filter).
