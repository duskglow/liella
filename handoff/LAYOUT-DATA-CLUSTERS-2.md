# LAYOUT-DATA-CLUSTERS-2: written start-up state out of .lbss, relro head dropped — INCOMPLETE (localtime fast +1)

**State (2026-11-11):** branch `task/layout-data-clusters-2` (on `task/layout-data-clusters`, which stacks
TEXT-PIN-9/RODATA-2/-3, none in origin/next). Code 1db846eec; D-X.64 rewritten (records only, on this branch only).
Role mei:~/w/s1. Plants: `handoff/LAYOUT-DATA-CLUSTERS-2-plants.txt`. Earlier record: `handoff/LAYOUT-DATA-CLUSTERS.md`.

## Why the last plants run failed (ldc-plants-3 rc 2)
The baseline test `init_written_startup_objects_are_in_the_data1_cluster` skipped every symbol whose section name did
not contain the symbol, `.data1` included, so it counted 0 moved objects on any tree. Fixed (`.data1` is exempt).

## Why HEAP/EXIT_HANDLERS were in .lbss (checked, not assumed)
Not the code model (`small`, spec/abi/rust-abi.toml): xtask's own `move_cold_bss` (D-X.22) renames every `.bss.*`
section >= 1 KiB to `.lbss.*`, and it ran BEFORE `move_startup_state`, so keyed EXIT_HANDLERS (1048 B) never reached
the cluster; HEAP (5376 B) is over the 4 KiB keyed cap. EPOLL is an `AtomicI32` = -1 (.data, 4 B), already in .data1.
RESERVE_MEM (64 KiB) is never written at start-up; it only shared a page with HEAP's head.

## What the code does now (D-X.64)
1. so.rs: `move_startup_state` before `move_cold_bss`. 2. coldbss.rs: HEAP in `STATE_KEY_SUFFIXES`; `STATE_C_SECTIONS`
= [`.bss.environ`] by section name (sound: 8 zero bytes, symbols keep their section). 3. fold.S: fold-limit line,
cmp_sse words, fill_erms_from/set_mid defined in `.data1`. File cost: HEAP 5376 + EXIT_HANDLERS 1048 + environ 8 + fold 64 B.
4. global_locale::SLOTS (112 KiB, locale paths write slot 0) stays in .lbss: the only start-up writer there, 1 page.
5. Relro head dropped (ruling #4577): tail beside `.got` cannot be named in GNU ld's default script (`.data.rel.ro`
takes `.local*` then all other inputs in link order; then `.dynamic`, `.got`). entry.S/startup_ro.rs relro code removed.
6. Tests: data1 test also flags `.lbss.` and requires HEAP, environ, fold_limit, cmp_sse_null, set_mid in .data1; the
phase test drops the relro check and adds `lbss` pads (0x800, 0x2000: `.lbss` shifted on the page grid), equal counts
over cold+lbss pads.

## Measured (mei, scratch ldc_measure.py; base = origin/next aad3df74d job ldc2-base-measure, after = 1db846eec job ldc2-after)
| orig link | true | locale | tool | fork | localtime | lt+mktime | float | tool rwPages |
|---|---|---|---|---|---|---|---|---|
| fast next | 17 | 55 | 58 | 7 | 20 | 20 | 3 | 10 |
| fast this | 12 | 51 | 54 | 6 | **21** | **21** | 3 | 7 |
| release next | 15 | 54 | 57 | 6 | 19 | 19 | 4 | 9 |
| release this | 11 | 53 | 56 | 6 | 19 | 19 | 3 | 8 |
Cold pads (0x800..0xF000): identical counts in both profiles (fast 12/51/54/6/21, release 11/53/56/6/19). Written
pages at most next's everywhere. Remaining tool written pages (fast): .data1 (2), SETTINGS (.data, unkeyed), stdout /
_IO_2_1_stdout_ (.data, first-touch keying misses them, #4602), strcmp_rescans and dispatch::FMA (.bss), SLOTS (.lbss).
Release tool Dr 2 vs next 1 with no relro rename: NOT attributed (text/rodata stack or got phase).

## Open / next step
1. localtime fast 21 vs next 20 (also 21 at cold pads, 20 at 3 of 4 pre pads: text phase suspected, NOT VERIFIED).
   Perf the localtime mode by class (ldc_measure PERF_MODES += localtime) on next vs this; previous run saw the same
   +1 on the stack base 81c825fdf vs its branch.
2. Coordinator #4602: key objects by access, not first touch (tools/hottext/startup_data.py), and a test shifting .data
   order; stdout/_IO_2_1_stdout_/SETTINGS/strcmp_rescans/FMA are the candidates seen here.
3. Release Dr 1 -> 2 attribution. Bounds untouched.

## Jobs (mei)
ldc2-lint0 (inherited head clippy+static rc 0), ldc2-base-measure (rc 0), ldc2-check (clippy+static ok, unit filter
syntax rc 1, superseded), ldc2-unit (9 pass), ldc2-after (rc 0), ldc2-plants, ldc2-gate (running at hand-back).
