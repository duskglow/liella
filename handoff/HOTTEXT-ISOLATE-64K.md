# HOTTEXT-ISOLATE-64K (-2): hot group 64 KiB anchor, REJECTED (measured worse, reverted)

Branch task/hottext-isolate-64k-2 (continues task/hottext-isolate-64k; stacked on LAYOUT-MANIFEST-2, merged with
origin/next 03c9a4972 at the RECORDS cutover). Role kanon:~/w/kn1. Briefs: ~/state/briefs/HOTTEXT-ISOLATE-64K.md,
~/state/briefs/HOTTEXT-ISOLATE-64K-2.md. Head: see `handoff/reports/HOTTEXT-ISOLATE-64K-2.json`.

## State
- The anchor (empty `.text.sorted.h0`, align 65536, SHF_GNU_RETAIN, in the start-up member), its `group-anchor`
  layout kind and the HOT-GROUP-ANCHOR contract row are REVERTED: `xtask/src/hottext.rs` equals next's.
- D-X.51 (`records/decisions/D-X/D-X.51.toml`) now records the rejection with the numbers. NEGATIVE saturation
  record `records/pending/HOTTEXT-ISOLATE-64K-2-1.toml` (section 7, SR id to be assigned by the coordinator).
- D-X.48 (LAYOUT-MANIFEST, the layout contracts) turned into a record: its table row lived only on this stack and
  went with next's generated view at the merge; the layout static check needs it.

## Why the last run's jobs failed (all three causes fixed)
- measure-8beaf85 rc 1: `static` layout check could not resolve D-X.51 (the synced tree had the row uncommitted);
  now records. plants-8beaf85 rc 3: the hga-no-retain plant had a literal `|` (the plants field separator), so the
  tool refused the file. wlc-8beaf85 rc 2: base ddc2dbdaf was never primed on kanon.

## Measurements (Skylake-SP host; base = next 03c9a4972 in a RAM copy, change = 05f4baaf0; fast / release)
| row | next | anchor |
|---|---|---|
| start-up only faults | 18 / 17 | 19 / 18 |
| setlocale + printf faults | 56 / 57 | 58 / 61 |
| coreutils-shaped tool faults | 59 / 60 | 61 / 63 |
| fork child faults | 6 / 6 | 6 / 6 |
| init_first_mktime_after_localtime_r_takes_no_fault | pass / pass | FAIL / pass |
| probe hot group start (mod 64K), windows | 0x40dbc0 (0xdbc0) 10 / 0x413280 (0x3280) 10 | 0x420000 (0) 9 / 0x430000 (0) 9 |
| start-up tier windows | 2 / 1 | 1 / 1 |
| probe file | 5720960 / 4376992 | 5794392 / 4495776 |
| libc.so.6 file, LOAD segments | 5742000 / 5184616, 4 | 5762192 / 5266536, 5 (R E split in two) |
Cause (from the maps): in the base the text segment starts at 0x401000 and the group at 0x40dbc0, so `_start`, `.init`
and the cold text start-up touches share the group's first 64 KiB window; with the anchor they sit in a window of their
own (0x410000) and the aligned text segment shifts the .rodata/.data window cuts. wl_count was not run (the change was
reverted; the branch now changes no product code against next).

## Jobs (kanon)
Evidence: hottext-isolate-64k-2-measure-base (rc 0), hottext-isolate-64k-2-measure-new (rc 1: the failing fault test
above; evidence, not a final-head job). Plants hottext-isolate-64k-2-plants stopped by me (the code it planted was
reverted). Final head: hottext-isolate-64k-2-lint-1fcb391 (records check, clippy, static), hottext-isolate-64k-2-gate-1fcb391.
Scripts: kanon ~/work/HOTTEXT-ISOLATE-64K/measure.sh, groupmap.py; ~/work/HOTTEXT-ISOLATE-64K-2/base.sh.

## Do not redo
- An anchor (or `. = ALIGN(0x10000)`) directly before the group with cold text in front of it: measured worse.
- objcopy 2.42 cannot set SHF_GNU_RETAIN or the alignment of a section it adds (#4141).

## Next step (a new story, not this branch)
Keep the group's first window shared with the start-up code: sort the start-up object's own text (`_start`, crt1 is
ours) into the group's first tier, or anchor only where the text before it is never touched; measure with
measure.sh on both profiles and the start-up rows above. Plan rows: D-X.20, D-X.47, D-X.51.
