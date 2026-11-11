# LAYOUT-DATA-CLUSTERS-8: -7 merged with next; start-up instructions and faults against next

**State (2026-11-11):** branch `task/layout-data-clusters-8` = -7's `a4b0c7331` + merge of origin/next `07a4db6ac`
(`d9c6ac8be`) + `4aec9013d` (list entries, bounds tightened) + `38253a532` (bounds and NAME_COUNT taken back) +
this handoff. Net code change against the merge: `time_compat::HEAD` and `CURRENT` declared in
`crates/libc-so/startup-state.list`. Role keke:~/w/n1 (Zen 1). Scratch on keke (and mei for probes):
`~/work/LAYOUT-DATA-CLUSTERS-8/`: `measure.sh <tag>` (both profiles' products, every hot_text_products probe, a copy
of the page_faults probe, cbench_startup --counters x3), `probes.py <dir>` (every probe mode of copied probes, run
as the test runs them, under `setarch -R`), `map.sh <exe> <mode> <out>` + `resolve.py` (perf `-e page-faults -d`,
data addresses resolved against nm). Probe copies in `next/`, `head/`, `head2/`; maps in `*/*/map-*.txt`.

## Decisions in force
D-X.70 (the declared list, ruling #4694), D-X.64 (clusters), D-X.50. No new record: entries added under D-X.70.

## Results
Step 0 `ldc8-lint0` (clippy + static, inherited head): rc 0.
Start-up instructions (keke, cbench_startup --counters, 3 runs each, all equal): next 07a4db6ac **2142**,
merged head d9c6ac8be **2141**, 4aec9013d 2141 (layout only, no code: -5..-7 add nothing on top of 4C).

Faults, fast / release, every run identical; keke and mei (Skylake-SP) read the same count for every probe:

| tree | true | locale | tool | fork | float | localtime | lt+mktime | strings |
|---|---|---|---|---|---|---|---|---|
| next 07a4db6ac | 12 / 11 | 52 / 52 | 55 / 55 | 6 / 6 | 4 / 3 | 19 / 17 | 19 / 17 | 0 / **1** |
| merged d9c6ac8be | 11 / 10 | 53 / 53 | 56 / 56 | 5 / 5 | 4 / 3 | 19 / 19 | 19 / 19 | 0 / 0 |
| 4aec9013d (keke) | 11 / 10 | 53 / 53 | 56 / 56 | 5 / 5 | 4 / 3 | 19 / 17 | 19 / 17 | 0 / 0 |

Every data pad (cold, lbss, data) reads the same count, both profiles. Next's release products fail the strings
tests (fixed on this branch by -6's #4677 probe change).
- **FORK_BOUND 6 on Skylake-SP:** holds (fork child 5, both profiles; next 6).
- **Fast float +1 (#4703):** absent after the merge (head = next, 4 / 3, both hosts). Nothing to remove.
- **Release localtime +2:** perf maps: the first localtime_r writes `time_compat::HEAD` and `CURRENT` on `.bss`
  pages start-up no longer touches. Declared in the list: 17, next's level. 38253a532 keeps this.
- **locale / tool +1 (OPEN):** the maps show start-up loses `program_invocation_name`'s fault (-1) and setlocale
  reads then writes `global_locale::NAME_COUNT` (+2). Declaring NAME_COUNT changed nothing (53 / 56), so the
  tightened LOCALE/TOOL bounds failed (`ldc8-measure-head2`); both taken back in 38253a532. Within the bounds
  (55 / 58), but one above next.

## Jobs (keke unless named)
On 38253a532: ldc8-gate-2 (direct tier, --base origin/next), ldc8-plants-2 (one plant, marked equivalent: the
release-only localtime level has no gate guard). Diagnostics: ldc8-lint0, ldc8-measure-next (rc 1: next's
release strings tests), ldc8-measure-head, ldc8-measure-head2 (rc 1: the tightened bounds), ldc8-probes-keke,
mei:ldc8-probes-mei, ldc8-map-next-fast, ldc8-map-1. Stopped: ldc8-gate, ldc8-plants (stale head).

## Next step
1. locale / tool +1: map `page_faults locale` on 4aec9013d's products (`map.sh`, head2/) against next/: find
   where setlocale's extra fault went once NAME_COUNT was in `.data1` (a neighbour on the same page, or the
   `.bss` page itself still touched first by a read). Fix by declaration, then tighten LOCALE/TOOL to 52 / 55.
2. From -7: fold the two `.data1` list tests into one list-driven test; regen.sh header note.
