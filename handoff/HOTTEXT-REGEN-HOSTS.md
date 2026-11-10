# HOTTEXT-REGEN-HOSTS: the hot-text list must not depend on which host traced it

Branch `task/hottext-regen-hosts` (from `origin/next` 2e33a3cb9), code head 49caf6bed. Decision in force: **D-X.61**
(new; keeps D-X.56, D-X.49, D-X.47). Plants file: `handoff/HOTTEXT-REGEN-HOSTS-plants.txt`.

## What changed
- `tools/hottext/merge.py`: every key carries a 4th column, the `<profile>@<host class>` pairs whose trace hit it.
  A trace is the whole truth only for its own pair. A key is dropped only when no pair is left. A 3-column line
  (older lists) reads as `<profile>@?`: a hit resolves it, a miss keeps it.
- `tools/hottext/regen.sh`: host class = `<vendor>-<family>-<model>` from /proc/cpuinfo (`HOTTEXT_HOST_CLASS`
  overrides), passed to merge.py for hot-text.list and startup-text.keys.
- `xtask/src/hottext.rs` `parse_list` and `xtask/src/layout.rs` `startup_keys` accept the column. parse_list
  checks that the pairs name exactly the profiles column. `tools/hottext/startup_data.py` reads 4-column lines.
- Lists regenerated (fast profile) on keke (amd-23-1, Zen 1), then on mei (intel-6-85, Skylake-SP), from
  origin/next's lists. Compared with origin/next: **0 keys added, 0 removed, same order.** Only the provenance
  column is new. `liella_machine_fill_erms` is reached on Skylake-SP only and is now kept by `fast@intel-6-85`.
  Without this change a Zen 1 regen would drop that pair. Pairs: 478 fast@amd-23-1, 479 fast@intel-6-85,
  2 fast@?, 419 release@?, 63 keys with none.

## Why union, not forced dispatch (D-X.61)
Forcing each dispatch path in the probes covers CPU dispatch only, not the locale data and catalogs a host has. It
also needs a test-only dispatch override in the library and misses each new dispatch point without warning. The
union needs no knowledge of why hosts differ.
Limit: the layout position of a key that only one host class reaches follows the trace order around its anchor.
The key set, tiers and provenance do not depend on host order (tested).

## Results
- Tests (no spec clause: build tooling): `merge_keeps_a_key_another_host_class_reaches`,
  `regen_on_two_host_classes_in_either_order_gives_one_list`, extended `list_lines_carry_tier_and_profiles`.
- mei:hrh-xt1 (hottext tests, clippy, hottext-check, static) rc 0. mei:hrh-gate1 (affected --base origin/next on
  73b1e6c) rc 0. mei:hrh-plants1 4/4 CAUGHT, 0 WARN. Final head: mei:hrh-gate2 (clippy, static, affected) and
  mei:hrh-plants2 4/4 CAUGHT, 0 WARN, on 49caf6b.
- Two-host regen: keke:hrh-regen-keke-r1 and mei:hrh-regen-mei-r1. The fast pass passed on both. Each job then
  exited 1 in the release product build (below). mei:hrh-regen-mei-r2 (seeded with keke's list) rc 0, and its
  output is the committed list. keke:hrh-regen-keke-r2 failed on a stale build lock. keke:hrh-regen-keke-r2b
  (keke seeded with mei's r1 list, i.e. the other host order) rc 0. Compared with the committed list: **0
  sorted-line differences** in both hot-text.list and startup-text.keys (same keys, tiers and provenance).
  startup-text.keys has the same order too; hot-text.list order differs (the documented limit above).

## RULING NEEDED
- Brief step 5 (pin start-up keys with `#[inline(never)]`, plus a static check that fails on an inlinable key
  function) conflicts with owner ruling HOTTEXT-PINNING (D-X.56: the start-up tier stays unmarked because marking
  cost 3 to 45 instructions per start-up row). A key scheme cannot help: a function that is inlined away has no
  symbol to key. Recommend: owner re-rules. Either (a) mark only the generic and closure start-up keys that broke
  (`Features::detect`, the intl closure), measured with bench counters first, or (b) keep the tier unmarked and
  let the dead-key build check plus `regen.sh --build` repair them. Not implemented.

## INFORMATIONAL
- The release product build in the regen trees (`LIELLA_FULL_PROFILE=1 cargo xtask so --hottext-regen` after the
  fast regen) breaks STARTUP-TEXT-WINDOWS on both hosts: 5 windows against budget-release 4. The build reads the
  profiles column only, which this change does not alter. That points to an existing problem, but a release build
  of plain origin/next was not run here to confirm it. Release pairs stay `release@?` until a release regen runs.
- `startup-data.keys` regenerated on mei differs from origin/next (NSCACHE gains `fast`). It is not committed: it
  has its own merge in startup_data.py and is outside this story.

## Next step
Collect mei:hrh-gate2 (clippy, static, affected on 49caf6b). mei:hrh-plants2 is 4/4 CAUGHT. Then get the step-5
ruling. After that, a release-profile regen on both host classes once the release STARTUP-TEXT-WINDOWS break is fixed.
