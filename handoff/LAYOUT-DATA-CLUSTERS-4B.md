# LAYOUT-DATA-CLUSTERS-4B: -4's layout merged with next, two dead hot-text keys pruned

**State (2026-11-11):** branch `task/layout-data-clusters-4b` (on `task/layout-data-clusters-4`). Commits of this run:
`e24ab3ee7` merge of origin/next `1d8b14190` (clean, no conflicts), `c5dba2678` prune, `8d26806a4` handoff, then
`1e73c5143` a second merge of origin/next `28a903a01` (next moved during the run: HOTTEXT-DEAD-KEY-REPAIR; one
conflict, the import line of hottext.rs's test module, resolved as the union of both lists), then this update.
The table below was measured on c5dba2678; ldc4b-gate-2 and ldc4b-counts-head-3 (queued, see Jobs) re-run the gate
and the head counts on 1e73c5143. Role mei:~/w/s1 (Skylake-SP). Decisions in force: -4's D-X.68 (and D-X.64/D-X.67 under it);
none added. Earlier: `handoff/LAYOUT-DATA-CLUSTERS-4.md` (its "Not done" list is still the plan for -5/-6).

## What was done (mechanical, as briefed)
- `cargo xtask so` per profile on the merged tree (job ldc4b-so-pre): fast stopped with "hot-text.list: 2 key(s)
  match no function section of the fast-profile libc.a" (the train 592 red), release built (117 unmatched keys,
  all allowed: other-profile traces or hot-text.absent).
- `tools/hottext/prune.py crates/libc-so/hot-text.list target/product/fast/hot-text.matched
  target/product/release/hot-text.matched` dropped exactly the two expected keys and nothing else:
  `liella::locale::find_cached` and `attach_charmap` over `<liella_legacy::global_locale::Files,
  liella_boundary::locale::Glue>`. Neither is a cluster boundary in `hot-text.clusters` (clusters name keys, so
  positions re-resolve). No other change: no layout change, no new keys, no tuning.
- No plants: the change deletes two list lines that match nothing in either profile (the build's dead-key check
  is what catches them; the brief names no plants).

## Results (mei, code head c5dba2678)
- clippy + static: ldc4b-lint rc 0 (c5dba2678); ldc4b-lint-2 on the final head (handoff commit).
- Gate ldc4b-gate rc 0: `affected --run --base origin/next` escalated to FULL (entry.S, xtask, product key lists):
  32 suites ok, 11566 passed, 0 failed, 1259 ignored; conformance 127/0; peak memory 16G, oom_kill 0. The workspace
  run includes `hot_text_products` (static-PIE and data-phase tests) on fast products.
- Start-up probe minor faults, same host, `hot_text_products` fault tests with --nocapture per profile
  (scratch `~/work/LAYOUT-DATA-CLUSTERS-4B/counts.sh`; next = origin/next 1d8b14190 synced into the same role):

| products | true | locale | tool | fork | localtime | lt+mktime | float | strings | mktime | rebind |
|---|---|---|---|---|---|---|---|---|---|---|
| fast next (ldc4b-counts-next) | 17 | 54 | 57 | 7 | 20 | 20 | 2 | 0 | 0 | 0 |
| fast head (ldc4b-counts-head-2) | **12** | **51** | **54** | **6** | **19** | **19** | 3 (+1) | 0 | 0 | 0 |
| release next | 16 | 55 | 58 | 6 | 19 | 19 | 4 | 0 | 0 | 0 |
| release head | **11** | **52** | **55** | 6 | **17** | **17** | **3** | 1 (+1) | 0 | 0 |

  Every run was stable (warm-up and 5 runs equal). Totals: fast -12, release -13 faults over the probes.

## Open (for -5/-6, not touched here per the brief)
1. **release `strings` 1 vs 0**: `init_first_string_calls_after_printf_take_no_text_fault` FAILS on release
   products (test_rc 101 inside ldc4b-counts-head-2; the job itself exits 0 since it only measures). It is -4's open
   item 2 (#4631), seen since -2 (ldc3-map). The gate builds fast products, where it passes.
2. **fast `float` 3 vs 2**: next improved by one fault since -3 measured it (next 3 then, 2 now); head is still
   at 3. Not mapped.
3. -4's items 1 and 3 (startup-data.keys regeneration with the `data` region; counts table) remain.

## Jobs (mei)
JOBS (final code head 1e73c5143): ldc4b-lint-3 (rc 0), ldc4b-gate-2 (FULL, queued), ldc4b-counts-head-3 (queued
after the gate by `~/work/LAYOUT-DATA-CLUSTERS-4B/after_gate.sh`), ldc4b-counts-next (base, rc 0).
Superseded: ldc4b-so-pre (diagnostic build that writes the matched files; fast so failed inside on purpose),
ldc4b-counts-head (its filter missed the strings probe), ldc4b-lint and ldc4b-lint-2 (older heads), ldc4b-gate and
ldc4b-counts-head-2 (green / measured on c5dba2678, before next moved).
counts.sh exits 0 when both product builds succeed: it is a measurement; the release strings test failing inside it
(test_rc 101) is open item 1, not a job failure.

## Next step
-5: map the release `strings` fault and the fast `float` +1 (perf page-faults -d, head vs next, one host), then -4's
item 1.
