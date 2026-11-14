# LIBM-F32-BESSEL-LARGE handoff (run 7, LIBM-F32-BESSEL-LARGE-7: COMPLETE)

State: branch `task/libm-f32-bessel-large-7` = run 6's head 5eff9a8e6 merged with origin/next 24a434a17 (merge commit
88b65f024, clean auto-merge; it brings STARTUP-DATA-PAGE-ALIGN-3 and TRACE-PYCACHE-3, which the branch was held for).
Later commits are handoff/ only. Role mei:~/w/s2 (Zen 1 host). Decision in force: D-10.109
(records/decisions/D-10/D-10.109.toml, unchanged since run 3). Spec: ~/state/briefs/LIBM-F32-BESSEL-LARGE.md
"The change". No product code changed in runs 5, 6 and 7.

## Done in run 7 (as the coordinator's release note asked: merge next, gate and layout contracts, nothing else)
- Step 0, inherited head 5eff9a8e6: xtask clippy, all-features workspace clippy, `cargo xtask static`: rc 0
  (job libm-f32-bessel-large-7-lint0).
- Merged origin/next 24a434a17 (88b65f024). Same three lints on the merged head: rc 0 (libm-f32-bessel-large-7-lint1).
- Direct gate on 88b65f024 (`cargo xtask affected --run --base origin/next` under `~/bin/gate 16G`, job
  libm-f32-bessel-large-7-gate): rc 0. Steps: clippy, xtask unit tests, products (`cargo xtask so`), test -p liella
  (plain and exports-interposed), liella-boundary, liella-math, fuzz-check, static: all ok. 6 suites, 1639 passed,
  0 failed, 259 ignored. Peak memory 3.0G, oom_kill=0.
- Layout contracts (from the products step): all 12 contracts of layout/contracts.tsv hold. STARTUP-DATA-WINDOWS:
  1 read-only start-up data window of 64 KiB (ro-budget-fast 1), 7 written start-up data pages (rw-budget-fast 7),
  77 of 77 start-up objects found. Run 6's red (13 pages against 12) is gone with the page-aligned placement.
- The __pycache__ private-paths hit from run 6 did not recur (TRACE-PYCACHE-3 is on next; the lint script and the
  gate also removed tools/hottext/__pycache__ before static, as the brief asked).

## Earlier evidence (code unchanged since)
Run 5's plants (6/6 CAUGHT) and percall (large band 405.5-417.5 instr/call, 1.31-1.39x glibc, from 1.75e5-1.78e5
before) ran on head 42c5541ce on keke; the band's code is byte-identical on this head. Plants file:
handoff/LIBM-F32-BESSEL-LARGE-5-plants.txt. Not rerun here (the release note said nothing else).

## Open / next
- Nothing left for this story: land it.
- Follow-up (not this story): jnf(0|1, x) for |x| >= 2^19 still takes the accurate path (as on next). Give its refusal
  the same one-call rest path, after measuring the jnf rows first.
Not to redo: the table enumeration (current for LG_B = 8), the design, run 3's dispatch experiments, the merges, the
plants, the percall.

Jobs (mei): libm-f32-bessel-large-7-lint0, libm-f32-bessel-large-7-lint1, libm-f32-bessel-large-7-gate. Script:
~/work/LIBM-F32-BESSEL-LARGE-7/lint.sh.
