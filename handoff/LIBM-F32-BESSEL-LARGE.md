# LIBM-F32-BESSEL-LARGE handoff (run 5, LIBM-F32-BESSEL-LARGE-5: INCOMPLETE: LAYOUT-JITTER)

State: branch `task/libm-f32-bessel-large-5` = run 4's branch (`task/libm-f32-bessel-large-4`, 5840b50c1) merged with
next 51af84e06 (merge commit 8ebbe6c89); jobs ran on 42c5541ce (merge + plants file; later commits are handoff/
only). Role mei:~/w/s4 (Skylake-SP). Decision in force: D-10.109 (records/decisions/D-10/D-10.109.toml, unchanged
since run 3). Spec: ~/state/briefs/LIBM-F32-BESSEL-LARGE.md "The change".

## Done in run 5 (the merge; no product code changed)
- Step 0: clippy (`cargo xtask clippy` and the all-features workspace clippy) and static green on the inherited head
  5840b50c1 (job lint0).
- Merged origin/next. One conflict, xtask/src/longsweeps.rs `SWEEPS`: next gave the fast32 asinf/acosf/atanf
  exhaustive rows `.env(STRIDE1)`; this chain had added the `bessel_large_table_is_the_refused_set` row (group long)
  just above them. Resolved by keeping both (next's three rows with STRIDE1, this chain's row and comment). Nothing
  else needed a hand merge.
- Merged head: clippy, all-features clippy and static (which runs the long-sweeps check) rc 0 (lint1).
- Plants (handoff/LIBM-F32-BESSEL-LARGE-5-plants.txt, run 4's six unchanged, the tool's results in its header):
  6 CAUGHT, 0 SURVIVED, 0 WARN.
- Percall (mei, instructions per call, glibc / base next 51af84e06 / head 42c5541ce): large j0f
  296.2/175440.4/405.5, j1f 291.7/178324.1/406.0, y0f 302.1/175430.4/408.0, y1f 317.8/178328.1/417.5 (1.31-1.39x
  glibc); mid j0f 286.5/233.3/233.3, y1f 299.3/265.7/265.7; tiny j0f 54.3/109.0/109.0, j1f 67.8/134.3/134.3, y0f
  98.2/345.7/345.7 (mid and tiny equal to next). Base and head give the same value on every row; the MISMATCH lines against glibc are on next too.
- Run 4's exhaustive sweeps job (kanon, libm-f32-bessel-large-4-sweeps) ended rc 0 (table check 34 s, the four
  sweeps 851-3406 s); the band's product code has not changed since.

## Why INCOMPLETE: LAYOUT-JITTER (owner ruling #4347)
Gate (direct tier, base origin/next): liella-math, liella-boundary, xtask unit tests, fuzz-check and static ok;
1601 passed, 2 failed, 256 ignored. Both failures are one test,
`hot_text_products::init_data_clusters_hold_at_every_text_phase` (in `test -p liella` and in its exports-interposed
pass): `tool` reads 54 at nine phases and 55 at `cold 0x8000` (cold 0x800/0x2000/0xf000, lbss 0x800/0x2000, data
0x800/0x1000/0x2000/0x3000: 54). The same test on next 51af84e06 (job nextlayout, a RAM copy of the primed base
source, `cargo xtask so` then the test): 54 at all 10 phases, rc 0. This branch changes no start-up code (the f32
Bessel band in liella-math, one xtask sweep row). Its code and table shift the layout of the linked probe, and one
`cold` phase then puts a start-up page across a boundary. handoff/layout-stability-rethink.md section 2 measured
`tool` 55-58 over the `cold` pads on next from padding alone. Per the ruling: not chased (no section moves, no bound
change, no regen). The story waits until the layout fix lands, then is re-gated.

## Jobs (mei)
lint0 (inherited head), lint1, plants, gate (rc 1: the test above), percall, nextlayout (next's measurement).
Scripts: ~/work/LIBM-F32-BESSEL-LARGE-5/{lint,final,nextlayout}.sh.

## Open / next
- Once the layout fix (LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS follow-up) is on next: merge next, rerun the direct
  gate only. If init_data_clusters still moves there, find which page the extra `tool` fault is in (perf
  page-faults -d at cold 0x8000 against cold 0x2000), which is layout work, not this story.
- jnf(0|1, x) for |x| >= 2^19 still takes the accurate path (as on next). Follow-up: give its refusal the same
  one-call rest path (measure the jnf rows first).
Not to redo: the table enumeration (current for LG_B = 8), the design, run 3's dispatch experiments, the merges.
