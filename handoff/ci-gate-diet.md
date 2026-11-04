# CI-GATE-DIET handoff (after CI-GATE-DIET-2)

State: branch task/ci-gate-diet-2 (stacked on task/ci-gate-diet). Code: 398f5b39 (run 1) and ff19b460
(run 2). The handoff commit on top changes only handoff/. Host mei, role s1.

## What changed
1. Export pass (xtask/src/main.rs, nextest.rs), run 1: before the second nextest run, `cargo nextest
   list --list-type binaries-only` lists the test binaries. A binary whose bytes do not contain
   `liella_boundary` is left out with `-E not binary_id(..)`, and the log prints it as `once: <id>`.
   Fail-safe: an unreadable or empty binary, a failed listing, or a caller's own `-E` runs every
   binary. Test: `a_binary_holding_boundary_code_is_never_run_once`.
2. Tail, run 1: complex_ld_cr_products 8 -> 20 shards (shard.rs takes `SHARDS` from the file that
   includes it; the other users keep 8; shard priority 90). Bessel j0/j1 8 -> 17 shards.
3. Run 2 (ff19b460): `the_bessel_shards_take_every_operand_exactly_once` rebuilt the index filter
   itself, so a wrong filter in the sweep would have passed it, and the hand-written shard list was
   checked only by its count. The sweep and the test now share `bessel_shard_indices` and
   `bessel_share`. A compile-time check, `bessel_list_is_exact`, requires the listed shards to be
   8..17 of j0 and of j1, each exactly once. Inputs and verdicts are unchanged.

## Verified (mei, role s1, on ff19b460)
- clippy --workspace --all-targets --all-features -D warnings rc 0; xtask static rc 0; both
  partition tests pass.
- Plants job `ci-gate-diet-2-plants` rc 0, 7 of 7 CAUGHT, each by its expected message: marker
  check off, fail-safe off, shard.rs share off by one, shard.rs map wrong, Bessel filter wrong,
  Bessel share off by one, Bessel list duplicate (compile-time check). Script:
  mei:~/work/ci-gate-diet-2/plants.sh.
- Run 1's FULL gate `ci-gate-diet-gate` (on 398f5b39) rc 0: 10483 passed, 0 failed, 114
  conformance. Run 1's `ci-gate-diet-plants` never ran: the jobrun call meant to start it only
  printed jobrun's usage message (plants.out). This run replaces it.

## Numbers
Deterministic, before (CI ci-gate-ce0a9ce6b) -> after (ci-gate-diet-gate):
- pass 1: 5635 tests (5668 after; the new shards and partition tests add the rest)
- pass 2: 5601 tests in 233 binaries -> 4791 tests in 166 binaries; 69 binaries run once
  (the `once:` list in the log). complex_ld_cr_products holds boundary code, so it still runs
  in pass 2, as the rule requires.

Wall time: NOT VERIFIED. Before (CI log): xtask test 1537.5 s, pass 1 618 s, pass 2 709 s,
longest test 504 s. ci-gate-diet-gate overlapped two CI gates, run one after the other (load ~20): xtask test 3246 s,
pass 1 1420 s (2.3x the CI pass 1). Its longest tests were Bessel j0 s3/s12 at about 300 s and
complex_ld_cr s8 at 253 s, all under that load. Run 2 booked mei (#24) for a before/after pair
under the same conditions. CI's ci-z gate kept running through the booking, so the booking
was cancelled. The pair still runs detached with CI beside it, so read its numbers as
indicative only:
- before: `ci-gate-diet-2-before-2` (origin/next tree in /mnt/zram/claude/CI-GATE-DIET-2/before,
  log there under target/affected.log)
- after: `ci-gate-diet-2-gate` (role s1, ~/w/s1/target/affected.log); load at start and end
  in mei:~/work/ci-gate-diet-2/load-*.txt. Check jobs.tsv for which CI gates overlapped.

## Open / next step
- Clean wall time: compare CI's own FULL gate logs on mei before and after this lands (same
  role, same conditions). That is the brief's "or use the CI logs". Read the two `Summary [`
  lines and the longest PASS lines. If a shard runs over ~180 s with mei otherwise quiet, raise
  SHARDS (complex_products.rs's include) or BESSEL_SHARDS (and extend the bessel_shards! list;
  the compile-time check enforces this).
- A timing booking does not hold off CI's gate on mei. A clean before/after needs CI paused
  for about an hour, or a quiet build host.
- The direct tier (affected.rs pass0_only) is out of scope and untouched.
