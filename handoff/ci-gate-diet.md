# CI-GATE-DIET handoff

State: branch task/ci-gate-diet, code commit 398f5b39 (plus this handoff commit). Host mei, role s1.

## What changed
1. Export pass (xtask/src/main.rs, nextest.rs): before the second nextest run, `cargo nextest list
   --list-type binaries-only --message-format json` (same features) lists the test binaries. A binary
   whose bytes do not contain `liella_boundary` is left out with `-E not binary_id(..)`, and the log
   prints it as `once: <id>`. Why this is sound: the pass changes only liella-boundary's features
   (`export = []` enables nothing in other crates), and rustc links a dependency only into binaries
   that use it. Fail-safe: an unreadable or empty binary, a failed listing, or a caller's own `-E`
   means every binary runs. Test: `a_binary_holding_boundary_code_is_never_run_once`. PASS0_ONLY
   (#889) is unchanged.
2. Tail: complex_ld_cr_products 8 -> 20 shards. shard.rs now takes `SHARDS` from the file that
   includes it, and the other three users keep 8. A compile-time check requires the shard tests to be
   exactly 0..SHARDS. These shards also get nextest priority 90, so they start early. Bessel j0/j1:
   8 -> 17 shards (prime). j0 s0 took 432 s against about 200 s for the other shards. Same inputs
   overall: `the_shards_check_every_line_exactly_once` and
   `the_bessel_shards_take_every_operand_exactly_once` check that every index lands in exactly one
   shard, and the per-shard share asserts are unchanged.

## Numbers (mei, FULL)
Before (CI log ci-gate-ce0a9ce6b-1793757016): stubs 0.2 s, clippy 12.9 s, xtask test 1537.5 s;
pass 1 618 s (5635 tests), pass 2 709 s (5601 tests); longest test 504 s.
After: job `ci-gate-diet-gate` on mei (~/logs/jobs.tsv, log via jobrun). NOT YET READ: still running
at hand-back. To get the after numbers, read `Summary [` lines, `once:` lines and the step table
in its affected log.

## Verified
clippy --all-features -D warnings rc 0, xtask static rc 0, the partition tests pass (mei).
Plants job `ci-gate-diet-plants` (4 plants: marker check off, fail-safe off, share off-by-one,
shard map wrong).

## Open / next step
- Read the gate's numbers. If a shard still runs over ~180 s, raise SHARDS or BESSEL_SHARDS.
  If complex_ld_cr_products holds boundary code (it is not in the `once:` list), pass 2 still runs
  it; that is correct by the rule, and the 20 shards bound it.
- The direct tier (affected.rs pass0_only) is not touched: out of scope.
