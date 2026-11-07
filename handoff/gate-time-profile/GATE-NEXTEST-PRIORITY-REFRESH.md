# GATE-NEXTEST-PRIORITY-REFRESH: start the tests that end the passes today first

Id: GATE-NEXTEST-PRIORITY-REFRESH
Kind: code
Priority: 55
Size: S
Host: -
Model: opus
Effort: medium
BUDGET: 40 tool calls / 90k

## Why
GATE-TIME-PROFILE (handoff/gate-time-profile.md, 40 green FULL CI gates) found that both nextest passes end on
long tests missing from the GATE-SPEED priority list in .config/nextest.toml (`priority = 90`):
- pass 1 ends on a `special_ld_products` shard (117-150 s, threads-required 4) in most trains;
- both passes end on xtask unit tests `affected::tests::csys_falls_back_to_full_when_unsu...` (51-80 s) and
  `bench::tests::counters_files_hold_only_...` (36 s), which start among the last;
- not in the list either: `fast_gamma double_gamma_kernels_stay_within_their_bounds_and_rarely_fall_back`
  (median 343 s, p90 454, the longest test of pass 1), the `complex_f128_products` shards (217-228 s in pass 2),
  `special_f128_products` shards, liella-math `fast32::tests::inverse_trig_cores_are_within_their_bounds` (184 s).

## The change
Add them to the `priority = 90` override (exact names from the latest CI gate log, not from this brief's
truncated ones), and any other test over ~60 s in the last 10 green gate logs that is not listed (list them in
the hand-back with their medians). Priority changes only the start order, never what runs (the comment in
nextest.toml says so; keep its wording). Check the threads-required = 4 special_ld/special_f128 shards still keep
their pool limit (special_ld_products::pool_matches_the_nextest_config).

Expected: -1 to -3 min per FULL gate (tails of both passes), NOT VERIFIED; measure it.

## Proof that coverage is unchanged
- `cargo nextest list --workspace` before and after: identical test sets for both passes (with and without
  `--features liella-boundary/export,liella-boundary/selftest`); attach the diff (empty).
- No plants needed for a config-only ordering change (no code); say so with the reason, per docs/plants.md.
- Timing: one FULL `cargo xtask test` run of the two passes before/after on a quiet slot, reporting the two
  `Summary [` lines and the last 5 completions of each pass; one run each is indicative only, say so.

## JOBS
gate (direct tier; touches only .config/nextest.toml), the before/after timing job. `JOBS:` line in the hand-back.

## Hand-back first line
`COMPLETE: priority refresh (pass1 <before> -> <after> s, pass2 <before> -> <after> s, indicative)`, or INCOMPLETE:.
