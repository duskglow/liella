# GATE-NEXTEST-PRIORITY-REFRESH: start the tests that end the passes today first

## State
- Branch `task/gate-nextest-priority-refresh` (from origin/next c1259b85b). Code commit 211024e65 (only `.config/nextest.toml`).
- Build role keke:~/w/n1. Jobs (keke): `gate-nextest-priority-refresh-gate` (clippy + static + direct tier,
  `--base origin/next`), then `gate-nextest-priority-refresh-timing` (waits for the gate, then the list identity check
  and one FULL `cargo xtask test` with the old config and one with the new config). Script: keke
  `~/work/gate-nextest-priority-refresh/run.sh` (copy in scratch). Re-attach: `ssh keke '~/bin/jobwait
  gate-nextest-priority-refresh-gate gate-nextest-priority-refresh-timing'`; results: `~/logs/gate-nextest-priority-refresh-timing.log`
  (LIST p1/p2 identical/DIFFERS lines, the `Summary [` lines and last 5 completions of each pass per run),
  full logs `~/work/gate-nextest-priority-refresh/test-{before,after}.log`, list diffs `list-p{1,2}.diff`.
- At hand-back both jobs were still QUEUED for slots on keke (busy host: 4 shared jobs running). Gate and timing
  results are NOT VERIFIED by me.

## The change
The `priority = 90` GATE-SPEED override gains every test with a median of more than a minute in the 10 latest
green FULL CI gates on mei (ci-gate-332a3489b .. ci-gate-c1259b85b, parsed from the `.affected.log` PASS lines)
that was not already listed, plus the xtask bench counters test named by GATE-TIME-PROFILE. Medians (s, p1 = plain
pass, p2 = exports-interposed pass):
- fast_gamma double_gamma_kernels_stay_within_their_bounds_and_rarely_fall_back: p1 339 (max 633)
- complex_f128_products shards: p2 171-214, p1 116-154
- special_ld_products shards: p2 134-183, p1 95-132; special_f128_products shards: p2 116-161, p1 57-91
- fast32::tests::inverse_trig_cores_are_within_their_bounds (liella-math lib): p1 182
- fast_gamma double_gamma_fast_tier_rounds_correctly / _at_the_negative_zeros: p1 107 / 106
- regex_subpatterns back_references_over_unary_strings_finish: p1 95
- complex_products shards: p2 80-94
- heap_threads statistics_never_read_below_the_blocks_live_throughout: p2 80
- xtask affected::tests::csys_falls_back_to_full_when_unsure: p2 75, p1 72
- core_contracts mutex_stress_mixed_entry_points_lose_no_update: p2 73, p1 69
- special_products special_functions_on_special_operands: p2 70
- xtask bench::tests::counters_files_hold_only_their_own_runs_rows: under 45 (36 per GATE-TIME-PROFILE), added as the brief asks.
Not added: dns_products resolv_equals_the_reference (98) is already priority 100 (privileged group); strstr_find,
small_compare, special_cr Bessel, complex_ld_cr_products are already listed; everything else is under 60 s.
- The threads-required = 4 override of special_ld/special_f128 is a separate override and unchanged (nextest resolves
  each setting from the first override that sets it); special_ld_products::pool_matches_the_nextest_config only
  looks for that override block, which still holds both binaries and the `threads-required = 4` line.
- No plants: a config-only ordering change (no code; priority changes only the start order, never what runs); the
  proof of unchanged coverage is the before/after `cargo nextest list` diff in the timing job (both feature sets).

## Open / next step
1. Collect the two jobs (command above). Gate rc 0 and both `LIST pN identical` lines -> the change is proven.
2. Read the before/after `Summary [` lines (one run each: indicative only; keke is a shared host, not a quiet slot).
   If after is not faster, the likely cause is the 16 threads-required 4 shards now starting at once with the
   complex shards; a follow-up could leave special_ld/f128 at default priority.
3. Expected -1 to -3 min per FULL gate: NOT VERIFIED.

## Do not redo
The 10-gate parse (scratch `long.py`, run on mei over `~/logs/ci-gate-*.affected.log`).
