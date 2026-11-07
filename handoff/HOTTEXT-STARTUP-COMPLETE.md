# HOTTEXT-STARTUP-COMPLETE: every start-up function in the hot group, and a stale key fails the build

State: branch `task/hottext-startup-complete` (from origin/next e6f60a3ff), commits 7de835e09 (code; its subject
says "WIP:" though the code is complete: no rebase allowed to reword it), 118c2812b (regenerated list), 6d1cfa3cf (D-X.47
and the test fix), then this report. Decision in force: **D-X.47** (extends D-X.20, D-X.32). Plan:
`handoff/layout-contracts-plan.md` finding 1 and section 2c C1.

## What changed
* `crates/libc-so/hot-text.list`: each line is `<key>\t<tier>\t<profiles>`; tiers startup, fork, stdio, strings,
  locale, tool, extra (the first probe that touched the key: p_true startup, p_fork fork, p_puts/p_cat/p_fmt stdio,
  p_str strings, p_loc locale, p_locuse/p_nss and extra programs tool, anything else extra); profiles fast, release,
  `-` (a key of the older list that no current trace hits: kept, unguarded). Order unchanged in kind (HOT-ORDER).
* `tools/hottext/regen.sh` reads the profile from `target/liella/.profile`, tags each trace with its tier;
  `keys.py --tagged`; `merge.py <old> <new> <profile>` unions profiles, keeps the earlier tier, drops a key whose
  only profile was the one regenerated and that it no longer hits, keeps `-` keys.
* `xtask/src/hottext.rs`: `parse_list`, `unmatched`, `unmatched_error`; `group(.., profile)` fails when a
  startup/fork key traced in the profile being built matches no function section ("renamed, now generic or inlined:
  run tools/hottext/regen.sh"), prints the count of the others. `so.rs` passes the profile only for the products
  (rustflags None); the bench's x86-64-v3 archive is not guarded (inlining differs). Two new unit tests.
* `init_cold_startup_functions_are_grouped`: `start::<false>` must be grouped; every `apply_irelative` section that
  exists must be grouped (release inlines it into start). Fails on origin/next's list (checked, both profiles).

## Measurements (Skylake-SP host, one role, test counts 5 runs each, every run identical)
| profile | startup | setlocale+printf | tool | fork | .text faults of `page_faults true` (perf -d) |
|---|---|---|---|---|---|
| fast, origin/next list | 21 | 60 | 62 | 7 | 8 (start::<false> alone in window 0x54) |
| fast, this list | 18 | 57 | 60 | 7 | 5 |
| release, origin/next list | 20 | 64 | 65 | 6 | 8 |
| release, this list | 17 | 58 | 61 | 6 | 4 |
Every other section's fault count is the same. NEGATIVE (not kept): `regen.sh --replace` (no old keys) lost 130 keys
and read fast 20 / 63 / 65 / 6, release 19 / 63 / 66 / 6: setlocale+printf and tool worse, so the list was merged
into the old one instead. No bound lowered: one host measured (the test's rule needs both hosts deterministic).
Method: `perf record -e page-faults -c 1 -d` of the probe after a warm-up, `setarch -R`, mapped to the probe's
sections and nearest symbols (scratch script pfaddr.py; job labels hsc-measure-2/3 on mei, outputs in
`~/work/HOTTEXT-STARTUP-COMPLETE/`).

## Plants (`handoff/HOTTEXT-STARTUP-COMPLETE-plants.txt`, job hsc-plants-1)
start-key-removed (caught by the grouped test, see deviation), startup-key-renamed (`so` with the new message),
guard-inverted and guard-any-profile (hottext unit tests). Results: see the hand-back / job log.

## Deviation from the brief (INFORMATIONAL)
Brief step 5 asked that removing the `start::<false>` key fail `so` with the new message. With the key gone no
listed key is unmatched, so the guard cannot see it; `init_cold_startup_functions_are_grouped` catches it instead.
Making `so` catch removal would need a hard-coded must-group set in xtask (a second copy of the test's list) or
STARTUP-TEXT-GROUPED (every traced start-up function grouped), which is the LAYOUT-STARTUP-WINDOWS story.

## Open / next
* 140 `-` keys (older list, no current trace hits them) include the stale `_RNvNtC15liella_boundary7runtime5start`
  (line 5). They still lower the counts (the `--replace` run above). Next: find which programs they came from (extra
  regen.sh arguments, earlier probes), trace those so they get a tier and profile, then drop the rest.
* The counts at the WIRE-2 head were not re-measured with this list (start::<false> is grouped now, so its window
  no longer moves with growth in cold text).
* Do not redo: the cause (finding 1) and the before/after counts above.
