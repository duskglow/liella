# LONG-SWEEPS-ROW-LIST

Branch task/long-sweeps-row-list. `cargo xtask long-sweeps --group G --list` prints one row name per line (the names
`--only` accepts, run order, nothing else); the old tab table is now `--table`. `select()` is shared by `--only`,
`--list`, `--table`; `list_text()` builds the stdout. Test: `list_prints_the_groups_rows_and_only_accepts_each`
(no spec clause). Groups are long, exhaustive, count (no "nightly" group: nightly runs long+count).

Jobs on keke:n1: LSRL-clippy (clippy+static, rc 0), LSRL-plants (2 CAUGHT, handoff/LONG-SWEEPS-ROW-LIST-plants.txt),
LSRL-gate (direct tier, was still running at hand-back: `~/bin/jobwait LSRL-gate` on keke), LSRL-list (output below).
Output: exhaustive = 37 rows (fast32::erf::tests::erff_fast_path_agrees_exhaustively ... num::glibc_diff::differential_millions);
long = 19 rows (wide_int::tests::tier_agrees_with_the_wide_route_long ... num::format::tests::scaled_general_matches_the_general_path_on_millions);
count = 2 rows (complex::ziv_tests::random_sample_retry_count, accuracy_suite_retry_count). Full text: keke ~/logs/LSRL-list.log.
Next: collect LSRL-gate. Not verified: gate result.
