# LAYOUT-DATA-CLUSTERS-5: keys regenerated, two tests added, tracer flaw found; NOT ready to land

**State (2026-11-11):** branch `task/layout-data-clusters-5` (on -4's `26b7cc3cb`; next not an ancestor). Role
mei:~/w/s1 (Skylake-SP). Decision D-X.69 (records/decisions/D-X/D-X.69.toml). Plants file
`handoff/LAYOUT-DATA-CLUSTERS-5-plants.txt` (not run green yet). Scratch: mei `~/work/LAYOUT-DATA-CLUSTERS-5/`
(`regen3.sh`, `counts.sh`, `faultmap.py`: per-profile build + hot_text_products --nocapture + perf fault map).

## Done
1. Keys regenerated per profile (fast, then release; **no `--replace`**: each pass refreshes only its own profile.
   A fast `--replace` pass drops the release keys and the release `cargo xtask so` then fails STARTUP-DATA-WINDOWS).
2. Tests (hot_text_products.rs): `init_data_clusters_hold_at_every_text_phase` gains region `data` (.data padded
   0x800/0x1000/0x2000/0x3000, counts must stay); new `init_startup_data_accesses_are_in_the_data1_cluster` (isolated
   link, `perf record -e page-faults -d` of `true`, names any <= 4 KiB own-section static touched outside .data1).
3. The new test found real leftovers in both profiles: `.bss.program_invocation_name`,
   `.bss.program_invocation_short_name`, `.data.__libc_single_threaded`. Cause: `startup_data.py resolve` keyed the
   smallest symbol at the address, so a zero-size linker marker (`__bss_start`) or one alias (`__progname`) stood in.
   Fixed (980460c7b: sized symbols first, every alias keyed); regen-3 keyed all three.

## Open (why this must not land yet)
- **Tracer oscillation:** regen-3 (traced from products already clustered by regen-1's keys) DROPPED
  `classed::OUTCOME` and `dispatch::FMA` (fast) that regen-1 keyed; the new test then names both (ldc5-htp-2, fast).
  The trace of objects already in `.data1` is not reliable, so keys flip between regenerations. Fix: trace from
  products built without the keyed move (e.g. an env switch that skips `coldbss::move_startup_state` for the trace
  build), not a sticky union.
- **Release regression with regen-3 keys (ldc5-htp-2):** fork child 7 at most pads, 8 at `data 0x3000` (> FORK_BOUND
  7; regen-1 keys: 5-6); tool 56 (regen-1: 55), locale 53 (52). Fast holds (true 12 / tool 54 / fork 6 / locale 51,
  data-phase test green in fast). Map the release fork child (perf -d of `fork`) before choosing keys.
- **Release `strings` 1 vs next 0 (#4631), mapped** (faultmap, mei, release, head vs next): the one fault in the
  window is the probe's OWN `.data` (`len_fn`, ip in `main`). On next that page also held `heap::DEBUG`, touched
  earlier by start-up; with start-up data moved to `.data1` nothing else touches the probe's page. Not libc data.
  Needs a ruling (see report json): change the probe to touch its pointers before `f0`, or place `.data1` next to the
  program's `.data`.

## Counts (mei, minflt, hot_text_products)
| products | true | locale | tool | fork | localtime | lt+mktime | float | strings |
|---|---|---|---|---|---|---|---|---|
| fast next 672acd9aa | 16 | 54 | 57 | 7 | 20 | 20 | 3 | 0 |
| fast head, regen-1 keys (ldc5-htp) | 13 | 51 | 54 | 6 | 19 | 19 | 3 | 0 |
| fast head, regen-3 keys (ldc5-htp-2) | 12 | 51 | 54 | 6 | - | - | - | 0 |
| release next | - | 55 | - | - | - | - | - | 0 |
| release head, regen-1 keys (ldc5-htp) | 12 | 52 | 55 | 5-7 | 17 | 17 | 3 | 1 |
| release head, regen-3 keys (ldc5-htp-2) | 10 | 53 | 56 | 7-8 | - | - | - | 1 |
"-": not read out (context budget); the logs on mei `~/logs/ldc5-*.log` have every line (`minor faults <mode>:`).

## Next step
Make the trace independent of the current clustering (above), regenerate both profiles, map the release fork child,
then run the plants file (plant: `program_invocation_name` key removed, msg names the section), clippy, static and
`cargo xtask affected --run --base origin/next`.
