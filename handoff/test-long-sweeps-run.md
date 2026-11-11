# TEST-LONG-SWEEPS-RUN: the ignored sweeps of liella-math run somewhere

State: branch `task/test-long-sweeps-run` (from origin/next), code commit 755cc31c6. Tests and runner only, no libc
code change. Brief: `~/state/briefs/TEST-LONG-SWEEPS-RUN.md`. No new decision record (a test runner, local).

## 1. The five `*_long` sweeps on origin/next (job `test-long-sweeps-run-sweeps`, shiki, release, 7 cores)

| sweep | result | wall |
|---|---|---|
| `wide_int::arc::tests::arc_tier_agrees_with_the_wide_route_long` | pass (worst 2^-96.85, Tan) | 99.2 s |
| `wide_int::quad::arc::tests::tier_agrees_with_the_wide_route_long` | pass (worst 2^-161.39) | 28.4 s |
| `wide_int::quad::tests::tier_agrees_with_the_wide_route_long` | pass (worst 2^-158.25) | 36.5 s |
| `wide_int::tests::tier_agrees_with_the_wide_route_long` | pass (worst 2^-95.67) | 29.9 s |
| `wide_int_el::tests::tier_agrees_with_the_wide_route_long` | pass (worst 2^-103.57, Expm1) | 154.3 s |

No failure, so nothing to fix (step 2). Budget for un-ignoring: under 10 s each in the normal (opt-level 2) test run.
None comes close, so all stay `#[ignore]`d and go into the runner (step 3).

## 2. The runner: `cargo xtask long-sweeps` (`xtask/src/longsweeps.rs`)

- `SWEEPS` names every ignored sweep of liella-math with a group. `long`: the five above plus `wide_dd`'s
  `tier_agrees_with_the_wide_route_long`, `complex_fast::tests::cexp_quick_tier_sweep` and `fast_bessel_n`'s
  `bessel_n_tier_sweep`. `exhaustive`: the 24 fast32 exhaustive f32 sweeps. `count`: the two Ziv retry counts.
- Existing entry point: `cargo xtask test` with `LIELLA_NO_VERDICT_CACHE=1`, the nightly and release runs that run
  every accuracy shard (D-10.57). It now runs the `long` group as step `long-sweeps`. The ordinary gate is unchanged.
- `exhaustive` and `count` run only through `cargo xtask long-sweeps --group exhaustive|count`. Nothing schedules
  them yet (RULING NEEDED below).
- Each sweep runs by exact name with `--ignored --exact`. A run that does not show exactly one passing test fails,
  so a stale name cannot pass by running nothing.
- Static check `long-sweeps` (in `cargo xtask static` and in the xtask unit test `the_tables_match_the_tree`):
  every `#[ignore]` in `crates/math/{src,tests}` is a `SWEEPS` row or a `NOT_SWEEPS` row with its reason, and every
  row names an ignored test that exists. NOT_SWEEPS: `gamma_sweep_profile` (diagnostic, asserts nothing) and
  `complex_directed`'s F80/F128 grids (open M6-10 findings, fail by design).
- Ignore reasons and comments that named a nonexistent "accuracy job" now name the runner (wide_int family,
  wide_dd, cexp, bessel_n, and the `LIELLA_ERF_SWEEP` comment in `fast_special.rs`).

## 3. Jobs (shiki, role sh1)

`test-long-sweeps-run-sweeps` (the five on origin/next), `-clippy`, `-static` (static plus the xtask unit tests),
`-gate` (affected --run --base origin/next), `-plants` (`handoff/TEST-LONG-SWEEPS-RUN-plants.txt`, 4 plants:
row dropped, stale row tolerated, macro ignore missed, zero tests read as a pass), `-runner` (the `long` group run
through the new runner on this head; evidence, includes `wide_dd`, known red in #4741). Results: see the report.

## RULING NEEDED

- Schedule the `exhaustive` group (24 sweeps of 2^32 to 2^34 calls each; earlier runs went past 150 min together
  under load). Recommendation: a weekly `cargo xtask long-sweeps --group exhaustive` job on an idle build host. The
  nightly would stay at the `long` group (~6 to 10 min on 7 cores).
- Run the `count` group with the nightly too (unmeasured here). Recommendation: measure it once, then add it to
  `test`'s step if it takes a few minutes.

## INFORMATIONAL / follow-ups

- crates/boundary has ignored "accuracy job" tests as well (`exp_quick.rs`, `trig_quick.rs`) and env-scaled ones
  that claim a long accuracy job (`hyperb_quick.rs`, `invhyp_quick.rs`). This story's check covers only crates/math.
  Follow-up TEST-LONG-SWEEPS-BOUNDARY: extend `SWEEPS` with a package column and the check to crates/boundary.
- `wide_dd`'s long sweep fails (#4741, WIDE-DD-EXPM1-BOUND owns it). Until that lands, the nightly `long-sweeps`
  step is red on it, which is correct.

Next step: the coordinator rules on scheduling; then TEST-LONG-SWEEPS-BOUNDARY.
