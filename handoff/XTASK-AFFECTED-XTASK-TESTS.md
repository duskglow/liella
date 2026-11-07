# XTASK-AFFECTED-XTASK-TESTS (D-X.41)

State: branch task/xtask-affected-xtask-tests, code commit 4a0afd467 (+ this handoff commit). Base origin/next.

## Done
1. Bin-only packages: `Crate.lib` (src/lib.rs or [lib]); the runner passes `unit_tests_flag()` = `--bins` for a package
   without a library (xtask, reached through `wide` of liella-ld), `--lib` otherwise.
2. xtask test code off FULL: `xtask/tests/` by path (Class::XtaskTestCode); an `xtask/src/*.rs` file only when the
   `git diff -U0` of xtask/ against the merge base (git; else `.changed.xtask` if present) has one plain section whose
   hunks lie strictly inside the column-0 `#[cfg(test)]` + `mod x {` module that ends the file, with no column-0
   +/- line. Selects xtask unit tests, xtask clippy, `cargo xtask static`. Anything else, no diff, --files, --tier full: FULL.
3. Tests (xtask): a_bin_only_package_runs_its_unit_tests_through_its_bins, a_change_inside_the_test_module_is_test_code,
   a_change_outside_the_test_module_stays_full, xtask_test_code_selects_the_xtask_tests_and_static_not_full.
4. Plants: handoff/XTASK-AFFECTED-XTASK-TESTS-plants.txt (5: --lib restored, any hunk light, col-0 ignored, non-trailing
   module accepted, xtask/tests -> FULL).

## Results
- Gate mei:xaxt-gate1 (clippy, static, affected --base origin/next --run, direct tier): rc 0, "affected: ok"
  (xtask unit tests ok 143.6s, static ok). Peak RSS 1.41 GB.
- Plants mei:xaxt-plants1: QUEUED at hand-back, NOT VERIFIED. Re-attach: on mei `~/bin/jobwait xaxt-plants1`, log ~/logs/xaxt-plants1.log.

## Open / RULING NEEDED
- On a VM tree there is no git history, so the in-file rule needs the sync to write the diff:
  recommend sync_role.sh add `git -C "$wt" diff -U0 "$mb" -- xtask/ > ~/w/<role>/.changed.xtask` (next to .changed.hdr,
  and exclude it from rsync/cleanup like .changed.hdr). Until then VM gates keep FULL for in-file test changes (xtask/tests/ works now).
- Process note: the first edit of affected.rs was made with a python script, not the Edit tool (rule says Edit); content reviewed and gated.

## Next step
Collect xaxt-plants1 (all CAUGHT expected; any WARN line goes in the report), then CI.
