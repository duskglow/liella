# LAYOUT-TEXT-PIN-8 (brief A, continuation 8): the +2 start-up faults are mapped; the cause is a list regen without the tool program

**State (2026-11-11):** branch `task/layout-text-pin-8` = task/layout-text-pin-7 6c3dc34b3 + this note, the plants
file and the report (no code change). Role keke:~/w/r5 (Zen 1 host). Decision D-X.63. Earlier notes:
handoff/LAYOUT-TEXT-PIN-7.md ... LAYOUT-TEXT-PIN.md. Checkpoint hand-back at ~150k context (65 calls).

## Done this run
0. clippy + static on the inherited head: green (job layout-text-pin-8-lint).
1. **Fault map** (perf `-e page-faults -c 1 -d`, `relay <mode>` as the test runs it, `setarch -R`, env LANG only;
   scripts fmap.sh / classify.py in ~/work/LAYOUT-TEXT-PIN-8 on keke; per-tree data in its next/ br/ mix/):
   | tree (fast products, same host) | tool | locale | true | .text | .data | .lbss |
   |---|---|---|---|---|---|---|
   | origin/next b49dfd6f8 (RAM copy) | 57 | 54 | 17 | 8 | 4 | 6 |
   | this branch (base 8253a107c) | 60 | 57 | 15 | 10 | 6 | 5 |
   | next + this branch's diff (`git diff 8253a107c HEAD`, applied clean) | 59 | 56 | 16 | 10 | 4 | 6 |
   (fault columns = tool mode.) **The .data +2 is not this branch:** on next's code the branch reads next's 4 (the
   branch's base orders the one Rust object's .data so that `heap::DEBUG` and `cpu::ERMS_LARGE_FROM` sit on two pages;
   group() renames text only). **The branch's own +2 is .text:** `text::builtin_charset`, `CtypeFile::{byte_class,
   wide,...}` and `OnceCell<Option<&[u8]>>::try_init` (global_locale Files<Glue>) link outside the hot group and fault
   once per window (0x4c0000, 0x4f0000, 0x720000); on next they are in the group (next misses only `find_cached`).
2. **Cause:** commit 6cd6ce097 (-6's regen, both profiles) dropped 14 keys next keeps (builtin_charset,
   CtypeFile::wide, Zone::kind_at, Rule::at, names::cache Table::{put,bump,set_file,file_in_change}, nscache
   put/config_epoch, nss group_line run/find_line, liella_sys_fcntl, Payload::next). merge.py drops a key neither
   profile's trace hit; -6 ran `regen.sh . --build` with **no extra program**, and these keys come only from the tool
   path (the built-in probes never run them: this run's fast regen on keke also traced no builtin_charset). The
   precedent (handoff/PERF-READDIR-ENTRY.md) passes the page_faults probe: `regen.sh <tree> "<page_faults built like
   the test> tool"`. Not the host (locale-archive, locale -a, nsswitch identical on keke, shiki, mei) and not codegen
   (no target-cpu=native).
3. Plants file fixed (handoff/LAYOUT-TEXT-PIN-8-plants.txt): a compiling realign mutant (the -5 defect), and the
   label plant runs all of `affected::tests`. NOT run yet.

## Open (next step, in order)
1. On the role: link page_faults as hot_text_products does (fmap.sh lines 12-13), then
   `bash tools/hottext/regen.sh . --build "<abs path>/page_faults tool" "<abs path>/page_faults locale"`, then
   `LIELLA_FULL_PROFILE=1` the same (link the release probe against the release libc.a). Check the list regains
   builtin_charset / CtypeFile::wide; `git diff` the four generated key files; fault-map again (expect tool <= 58,
   locale <= 55; next+branch should then read <= next). Copy the regenerated files back to the worktree (scp), commit.
2. Plants: `cargo xtask plants handoff/LAYOUT-TEXT-PIN-8-plants.txt --dir /dev/shm/claude/<id>/plants-1` (keke = Zen 1).
3. Direct-tier gate `--base origin/next`, then hand back. Consider a regen.sh guard: refuse a run with no extra
   program when the list holds tool-tier keys, or drop only keys whose tier's probes ran (RULING NEEDED below).
4. INFORMATIONAL: 2 sections carry the `.text.liella_cold.` rename; regen.sh's sed does not strip that prefix, so such
   a function can never re-enter the list by trace (latent).

## Not to redo
The three fault maps above (data in ~/work/LAYOUT-TEXT-PIN-8 on keke); the attribution of .data to next's code; the
host/codegen checks. Plan rows: D-X.63, D-X.56 (regen), D-X.47 (tiers), D-4.53.
