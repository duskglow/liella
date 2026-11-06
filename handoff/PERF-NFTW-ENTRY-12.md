# PERF-NFTW-ENTRY-12 (continues -11; branch task/perf-nftw-entry-12, role keke n2)

COMPLETE: fts_physical_1000 and nftw_phys_1000 are at most 1.04x glibc in time on an Ivy Bridge host and an AVX2-class host; no code change.

Timing (booked hosts, bench --compare --no-build --counters --runs 15 cbench_fstree, glibc and Liella interleaved, ns/op median):
- Ivy Bridge (nico): nftw_phys_1000 1.00 (1995936 vs 2002903), fts_physical_1000 1.04 (2279841 vs 2182510). Instructions 1.03 / 1.06.
- AVX2 class (nozomi, Intel N100): nftw_phys_1000 1.00, fts_physical_1000 1.03 (1426647 vs 1383658). Instructions 1.03 / 1.06.
Jobs: pne12-build (keke, bench programs), pne12-time-nico, pne12-time-nozomi, pne12-check (clippy + static on the head).
The register entry (docs/saturation/13-next-perf-stories-from-the-10.md, PERF-NFTW-ENTRY-12) has the per-phase split and the first-principles bound.

-11 verify failure: pne11-plants3 ran plants on the reverted-away copy (a survivor in code no longer on the head); the head has no new code, so no plants.
Decisions in force: ruling #2886 (cold side vector reverted). Do not redo: cold side vector, status byte copy, Node hot/cold split.
Next step: none required. Optional, under 0.3%: merge the two TryVec pushes in the fts build closure.
Rulings: none. Bookings 32 and 33 cancelled; nothing left running.
Note: `ops timing-book` waited ~35 minutes behind the sweep's bookings (30, 31); the package (xtask + target/liella + sources, 23 MB) was staged in ~/work/pne12 on both timing hosts meanwhile.
