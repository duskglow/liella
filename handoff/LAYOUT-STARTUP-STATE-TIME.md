# LAYOUT-STARTUP-STATE-TIME: first localtime_r / mktime / setenv statics declared in the start-up state (#4901)

**State (2026-11-12):** branch `task/layout-startup-state-time`, code commit `d11addb43` on origin/next `c1ae12c2d`
(no merge needed), then this handoff, the plants results and `handoff/reports/LAYOUT-STARTUP-STATE-TIME.json`.
Role mei:~/w/s1 (Skylake-SP). Scratch on mei: `~/work/LAYOUT-STARTUP-STATE-TIME/` (`diag.sh`, `htp.sh`, `pads.py`;
saved products `next-{fast,release}`, `branch-{fast,release}`).

## Decisions in force
D-X.70 (the declared list), D-X.64 (clusters), D-X.69 (access check), ruling #4901, owner jitter rule #4347. No new
record: entries added to the declared list and one more traced mode in the existing check.

## What changed
1. `crates/libc-so/startup-state.list`: nine entries after the first-zone-load block. The ruling named four
   (`time_compat::GENERATION`, `sys::TSC_STATE`, `process::TABLE_ON`, `process::ENV_OWNED`). The same check found five
   more: `process::ENV_CAP`, `ENV_VALUES`, `ENV_REPLACED`, `symbol __daylight` and `symbol __timezone`.
2. `hot_text_products::init_startup_data_accesses_are_in_the_data1_cluster` traces `localtime_mktime` as well as `true`.
   That mode runs setenv("TZ"), the first localtime_r and the first mktime. Every `.data`/`.bss`/`.lbss` input section
   is on its own 64 KiB boundary, and any small own-section static the trace faults outside `.data1` is named. On next
   (job lsst-diag) it named exactly those nine, in both profiles. On the branch it names none in either profile.
   No regen was needed: the list is hand-kept and `cargo xtask so` places it.

## Measurements (job lsst-probes, `pads.py`: relay harness, warm-up + 5 runs, lowest kept, no ASLR)
Probes: true, locale, tool, fork, mktime, localtime, localtime_mktime, float, strings, rebind. Pads: orig, pre and
cold 0x800/0x2000/0x8000/0xF000, data 0x800-0x3000, bss 0x800-0x3000, lbss 0x800/0x2000. Both profiles.
- **No cell is worse than next** (`WORSE cells: 0`, 760 cells).
- First localtime_r (and localtime_r + mktime), fast: next 20 at orig, 21/22/22/22 at the `.bss` pads. The branch reads
  20 at every one of them. Release: next 17, then 18/19/19/19 at the `.bss` pads; the branch reads 17 at all of them.
  The sum over pads falls from 381 to 374 (fast) and from 332 to 325 (release). Every other pad is equal.
- tool, fast, bss 0x800: 56 -> 55. Everything else is identical.
- At orig the counts equal next's, so LOCALTIME_BOUND (20) and LOCALTIME_MKTIME_BOUND (20) are unchanged; there is no
  new level to tighten to.

## Open (not this story)
- setlocale still has `.bss` jitter: locale 50 -> 51 and tool 54 -> 55/56 at the `.bss` pads, both profiles and both
  trees. Next step: trace `locale` and `tool` in the same check and declare what it names (LAYOUT-DATA-CLUSTERS-11
  proposed the same thing).
- A `bss` region in `init_data_clusters_hold_at_every_text_phase` would turn the pad equality into a test once the
  setlocale objects are declared too.

## Jobs (mei)
lsst-diag (next + the check: fails as expected, names the nine), lsst-htp (branch: all of hot_text_products on fast,
the two list checks on release, rc 0), lsst-plants (2/2 CAUGHT), lsst-probes (per pad, rc 0), lsst-lint (workspace
clippy -D warnings, xtask clippy, static, rc 0), lsst-gate (direct tier, --base origin/next). The results are in the report json.
