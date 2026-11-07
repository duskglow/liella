# GATE-PASS2-MATH-SHARD: the interposed pass runs shard 0 of the heavy math products sweeps

## State
- Branch `task/gate-pass2-math-shard` (from origin/next e0c6b146). Code head 9840ce8b (f21f708b code, 9840ce8b plants);
  the commit after it adds only this handoff and handoff/reports/GATE-PASS2-MATH-SHARD.json.
- Role keke:~/w/n3, synced at 9840ce8b.

## What changed (D-X.35, owner ruling #3794)
- `xtask/src/nextest.rs`: `PASS1_SHARD0` = liella-math `complex_ld_cr_products`, `complex_f128_products`,
  `special_ld_products`, `special_f128_products`, `complex_products`. The pass-2 filter (`pass1_filter`, used by
  `cargo xtask test`) adds `(binary_id(liella-math::<t>) & test(/_s[1-9][0-9]*$/))` per target (not for a run-once
  binary, which is already left out whole). `pass1_shard_filter` gives `affected`'s interposed pass the same term for
  the targets it runs (`xtask/src/affected.rs`, under nextest only; the `cargo test` fallback runs all, as it already
  ignores PASS0_ONLY's binary filter in `xtask test`).
- The shard chosen: shard 0 of each target's existing `shards!` split (line `i` goes to shard `i % SHARDS`; 1/20 of
  complex_ld_cr's lines, 1/8 of the others'). Why it still exercises the interposed functions: every shard does the same
  harness work - build and run the C programs, read and split their whole output into lines and fields, then check its
  own lines with the multiple-precision oracle (allocating `Big`s, formatting, BTreeMap tallies, the shard tally files).
  Only the number of checked lines differs, and shard 0's lines are spread over every function and rounding mode.
  Non-shard tests in those binaries (`the_shards_check_every_line_exactly_once`, `pool_matches_the_nextest_config`)
  still run in both passes.
- Rule documented in `docs/architecture.md` (Interposed run row) and D-X.35: a suite runs in pass 2 only to the extent
  that it exercises the interposed functions.
- Tests: `nextest::tests::pass2_runs_shard_0_of_the_heavy_products_targets` (new: each target has `_s0`, is sharded,
  no other fn name ends in `_s<digits>`; `pass1_shard_filter` per crate/target), `pass0_only_targets_are_pure_rust`
  (expected full filter string updated).

## Results (keke, Zen 1 EPYC 7551, 60 vCPU)
- gp2ms-check (clippy, static, `cargo test -p xtask --bin xtask -- nextest::`): rc 0, 8 passed.
- gp2ms-plants (`handoff/GATE-PASS2-MATH-SHARD-plants.txt`): 5 plants, 5 CAUGHT, 0 SURVIVED, 0 WARN. Not planted: the
  one-line call in affected.rs, which no unit test reaches.
- Test lists (`cargo nextest list --workspace`, files in keke:~/work/GATE-PASS2-MATH-SHARD/): pass 1 6127 tests
  (pass 1's command is untouched by the change, so before = after); pass 2 (export features, filter without the
  run-once binaries, which the gate adds at run time and this change does not touch) 6074 before, 6027 after:
  47 removed (complex_ld_cr 19, the other four 7 each), all `_s1`.. shards of the five targets, 0 added. Kept in pass 2
  of the five: each `_s0`, plus the two non-shard tests above.
- Pass-2 wall time before/after: job gp2ms-timing (running at hand-back; started 04:39Z, load ~12). Read with
  `grep '^TIMING' ~/logs/gp2ms-timing.log` on keke. It times `cargo xtask nextest --workspace --features
  liella-boundary/export,liella-boundary/selftest -E <old|new filter>` back to back on the same warm build. These runs
  include the run-once binaries the real gate leaves out, so the absolute numbers exceed the gate's pass 2; the
  difference is the effect. NOT VERIFIED on the CI host itself (the role is on keke): the CI gate's next `Summary [`
  line for pass 2 gives that number (profile median before: 807 s, 12414 test-seconds).
- gp2ms-gate: `cargo xtask clippy && static && ~/bin/gate 16G cargo xtask affected --run --base origin/next`; it starts
  when gp2ms-timing ends (keke:~/work/GATE-PASS2-MATH-SHARD/gate.sh), so the timing shares the role with nothing of ours.

## Open / next
- Collect gp2ms-timing (TIMING lines) and gp2ms-gate. A red gate outside xtask/liella-math pass 2 is not this change.
- Do not redo: the list diff above. Plan rows: D-X.35 (plan/07-decisions/x-implementation-decisions.md).
