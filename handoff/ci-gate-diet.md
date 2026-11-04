# CI-GATE-DIET handoff (after CI-GATE-DIET-3)

State: branch task/ci-gate-diet-3 (continues task/ci-gate-diet-2, stacked on task/ci-gate-diet).
Code: 398f5b39 (run 1), ff19b460 (run 2), 9e729fbf + 17f3825e (run 3). The commits above them change only
handoff/. Host mei, role s1. Decisions: none new (no D-row; local test-harness detail).

## What changed
1. Export pass (xtask/src/main.rs, nextest.rs), run 1: the second nextest run leaves out test binaries
   whose bytes do not contain `liella_boundary` (`once: <id>` in the log). It fails safe: an unreadable
   binary, a failed listing or a caller's own `-E` runs every binary.
2. Tail, run 1: complex_ld_cr_products 8 -> 20 shards, Bessel j0/j1 8 -> 17 shards.
3. Run 2: the Bessel shard map is checked against the code the sweep uses (shared helpers plus the
   compile-time `bessel_list_is_exact`).
4. Run 3 (fixes the CI red at f68c3f2b4, train 323): `support/shard.rs` read `super::SHARDS`, so
   `complex_f128_products` (new on next; it includes the helper and defines no SHARDS) failed with E0425.
   The helper now stands alone: `SHARDS = shards_of(env!("CARGO_CRATE_NAME"))` looks the target up
   in a `WIDE` table (only `complex_ld_cr_products`, 20), else `DEFAULT_SHARDS` (8). The includers'
   local `const SHARDS` are gone. Each `shards!` list is now checked at compile time with
   `shard::list_is_exact` (exactly `0..SHARDS`, in order) in complex_products, special_ld_products,
   special_f128_products and complex_ld_cr_products. complex_ld_cr_products also asserts that its
   count is 20, not the default, so a renamed target fails the build. Inputs and verdicts are unchanged.

## Verified (mei, role s1, on 17f3825e)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` rc 0, `cargo xtask static` rc 0
  (foreground, ~/work/ci-gate-diet-3/clippy2.log, static2.log).
- Plants `ci-gate-diet-3-plants` rc 0. Standalone check: a scratch test target that includes
  shard.rs and defines no SHARDS builds and gets 8 (this is what the CI red needed). Plants, 5 of 5
  CAUGHT with the expected message: WIDE entry renamed, DEFAULT_SHARDS 9, `same` always true,
  f128 list short by one, ld_cr list duplicate. Script: mei:~/work/ci-gate-diet-3/plants.sh.
- FULL gate `ci-gate-diet-3-gate` (cargo xtask clippy && cargo xtask test, under ~/bin/gate):
  started 11:3x Z, detached. Read its row in mei:~/logs/jobs.tsv and ~/logs/ci-gate-diet-3-gate.log.
- NOT VERIFIED here: the branch merged with next. My branch does not contain complex_f128_products
  (no merge allowed), so the standalone check stands in for it. CI's merge build is the real test.

## Numbers (from run 1; unchanged by run 3)
pass 2: 5601 tests in 233 binaries -> 4791 in 166 (69 binaries run once). Wall time: NOT VERIFIED
(every measurement overlapped CI gates; see run 2 notes: before `ci-gate-diet-2-before-2`, after
`ci-gate-diet-2-gate`, both indicative only).

## Open / next step
- If the gate is green and CI is green: compare CI's own FULL gate logs on mei before and after this
  lands (the two `Summary [` lines and the longest PASS lines). If a shard runs over ~180 s with mei
  quiet, widen it: add or raise its `WIDE` entry in support/shard.rs and extend its `shards!` list
  (the compile-time checks enforce both), or raise BESSEL_SHARDS and extend `bessel_shards!`.
- A new products test can include support/shard.rs as it is and gets 8 shards; it should assert
  `shard::list_is_exact` on its list like the others.
- A clean before/after needs CI paused for about an hour, or a quiet build host.
- The direct tier (affected.rs pass0_only) is out of scope and untouched.
- Do not redo: the export-pass rule, the Bessel map checks, the plants of runs 2 and 3.
