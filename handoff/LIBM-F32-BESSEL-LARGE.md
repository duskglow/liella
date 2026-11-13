# LIBM-F32-BESSEL-LARGE handoff (run 6, LIBM-F32-BESSEL-LARGE-6: INCOMPLETE: LAYOUT-JITTER)

State: branch `task/libm-f32-bessel-large-6` = run 5's head 66c86c79b merged with origin/next d53c0d551 (merge commit
957b3ab03, clean auto-merge of fast32.rs, special.rs, longsweeps.rs; later commits are handoff/ only). Role keke:~/w/r5
(Zen 1). Decision in force: D-10.109 (records/decisions/D-10/D-10.109.toml, unchanged since run 3). Spec:
~/state/briefs/LIBM-F32-BESSEL-LARGE.md "The change". No product code changed in runs 5 and 6.

## Done in run 6 (merge + re-gate, as the coordinator's release note asked)
- Step 0: inherited head 66c86c79b: xtask clippy, all-features workspace clippy, static rc 0 (job lint0).
- Merged origin/next d53c0d551 (now has trace-at-build as the default, LAYOUT-TRACE-DEFAULT / D-X.92). Merged head:
  the same three lints rc 0 (lint1).
- Direct gate (`cargo xtask affected --run --base origin/next`, job gate) rc 1, only on
  `products (cargo xtask so)`: layout contract STARTUP-DATA-WINDOWS reports 13 written start-up data pages, and the fast
  budget is 12. Every test step passed: xtask unit tests, test -p liella (plain and exports-interposed, so
  `init_data_clusters_hold_at_every_text_phase`, run 5's red, now passes), liella-boundary, liella-math, fuzz-check:
  1637 passed, 0 failed, 259 ignored. The gate's closing `cargo xtask static` step also failed, on private-paths,
  for a build artifact and not a tracked file: `tools/hottext/__pycache__/keys.cpython-312.pyc` (written when the
  trace-at-build step runs tools/hottext/keys.py) holds a home path. A role tree has no `.git`, so
  `labnames::tracked_with` falls back to `walk`, and `walk` skips target/ but not `__pycache__/` (which .gitignore
  lists). lint1's static passed on the same tree because it ran before any `so`. That is next's tooling, not this
  branch. Recommendation: skip `__pycache__` in `walk`, or run keys.py with `python3 -B`.

## Why INCOMPLETE: LAYOUT-JITTER (owner ruling #4347, amended 11-13)
origin/next d53c0d551 on the same host (job nextlayout: RAM copy, `cargo xtask so`) has 11 pages, so it passes against
the budget of 12, which has no margin (LAYOUT-TRACE-DEFAULT run 4 set it from its merged head). Page lists (job
nextpages: next's built products, layout-check with the rw budget forced to 1 in the scratch copy only, to make it
list its pages; merged-head list from the gate's target/affected.log):

| | written start-up pages |
|---|---|
| next d53c0d551 (11) | 0x853 855 856 857 858 85a, 874 875 876, 886 887 (x 0x1000) |
| merged head 957b3ab03 (13) | 0x853 854 855 856 857 858 85a 85b, 878 879 87a, 8a2 8a3 (x 0x1000) |

Both lists hold the same objects: start-up .data.rel.ro / .got bytes spread over 0x853000-0x85b000, printf's
`Env::text_only::C` alone on its page, cpu/stdio/locale data, and the 64 KiB `heap::RESERVE_MEM` starting 0xc18 into
a page (two pages). On the merged head they sit about 0x4d0 bytes higher (text_only::C 0x856630 -> 0x856b00), so the
scattered .data.rel.ro bytes reach 0x854000 and the .got bytes reach 0x85b000: +2 pages. This is placement alone. The
branch changes no start-up code (f32 Bessel band in liella-math, one xtask sweep row). LAYOUT-TRACE-DEFAULT's handoff
names the fix (LT-2, which clusters the data by phase: RESERVE_MEM page-aligned at the end of the rw cluster,
text_only::C beside the printf data). Not chased here: no section moves, no budget change, no regen.

## Jobs (keke)
lint0 (inherited head), lint1 (merged head), gate (rc 1: the contract above), nextlayout (next: 11 pages, rc 0),
nextpages (next's page list). Scripts: ~/work/LIBM-F32-BESSEL-LARGE-6/{lint,nextlayout,nextpages}.sh. Run 5's
plants (6/6 CAUGHT) and percall (large 405.5-417.5 instr/call, 1.31-1.39x glibc, from 1.75e5-1.78e5) are on its head
42c5541ce; the band's code has not changed since.

## Open / next
- Once data clustering (LT-2) is on next, or the rw budget has room for placement: merge next, then rerun the direct
  gate only. Nothing else is left for this story.
- Follow-up (not this story): jnf(0|1, x) for |x| >= 2^19 still takes the accurate path (as on next). Give its refusal
  the same one-call rest path, after measuring the jnf rows first.
Not to redo: the table enumeration (current for LG_B = 8), the design, run 3's dispatch experiments, the merges, the
plants, the percall.
