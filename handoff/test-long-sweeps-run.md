# TEST-LONG-SWEEPS-RUN(-2): the ignored sweeps of liella-math run somewhere

State: branch `task/test-long-sweeps-run-2` (TEST-LONG-SWEEPS-RUN's branch plus a merge of origin/next), code head
61d5dac09. Tests and runner only, no libc code change. Spec: `~/state/briefs/TEST-LONG-SWEEPS-RUN.md`; this run's
brief `~/state/briefs/TEST-LONG-SWEEPS-RUN-2.md` (merge only, wire rulings #4750/#4751). No decision record (a local
test runner).

## What exists

- `cargo xtask long-sweeps [--group long|exhaustive|count|all] [--list]` (`xtask/src/longsweeps.rs`): runs each
  ignored sweep of liella-math once in release by exact name, fails naming any sweep that failed or did not run
  exactly one test.
- Groups. `long`: the five wide_int family `*_long` sweeps, `complex_fast::tests::cexp_quick_tier_sweep`,
  `complex_fast::ls_tests::clog_csqrt_quick_tier_sweep` (new from next, added in -2), `fast_bessel_n`'s
  `bessel_n_tier_sweep`. `count`: the two Ziv retry counts. `exhaustive`: the 24 fast32 f32 sweeps.
- `cargo xtask test` with `LIELLA_NO_VERDICT_CACHE=1` (nightly and release) runs `long` and `count` (ruling
  #4751, added in -2) as step `long-sweeps`. The ordinary gate is unchanged.
- Static check `long-sweeps`: every `#[ignore]` in `crates/math/{src,tests}` is a `SWEEPS` or `NOT_SWEEPS` row,
  every row names an existing ignored test. It caught next's new clog/csqrt sweep on this run's merge.

## -2 (this run)

- Merge of origin/next. Conflict in `crates/math/src/wide_dd.rs`: WIDE-DD-EXPM1-BOUND un-ignored the wide_dd sweep
  (now `tier_agrees_with_the_wide_route`, 40 000 operands, ordinary run). Kept next's side and dropped the runner's
  wide_dd row. Conflict in `xtask/src/main.rs`: kept both static checks (`float-casts` from next, `long-sweeps`).
- Ruling #4751: count group measured on shiki (Haswell-EP, 26 threads, load ~1), job `test-long-sweeps-run-2-count`:
  230 s wall, of which ~3.8 min release build; `random_sample_retry_count` 1.2 s and `accuracy_suite_retry_count`
  1.9 s of test time (the runner's per-sweep lines, 194.8 s and 34.7 s, include their builds). Added to the nightly.
- Ruling #4750: the weekly `cargo xtask long-sweeps --group exhaustive` on an idle build host is a schedule outside
  the repository (liella-ops); the runner's docs name it. The coordinator schedules the job.

## Jobs (-2, shiki role sh1, head 61d5dac09)

- `-lint` (merge head 0a803f8ae): static red, `long-sweeps` named next's unlisted `clog_csqrt_quick_tier_sweep`.
  Fixed in 61d5dac09. `-lint2`: clippy, static and the xtask `longsweeps::` tests green.
- `-count`: count group, rc 0 (numbers above).
- `-plants` (`handoff/TEST-LONG-SWEEPS-RUN-plants.txt`; results in `handoff/TEST-LONG-SWEEPS-RUN-2-plants.txt`):
  5 plants, 4 CAUGHT, 1 EQUIVALENT, 0 SURVIVED, 0 WARN.
- `-runner` (`long` group on the final head): wide_int 32.9 s, wide_int_el 167.0 s, quad 39.0 s, arc 100.5 s,
  quad::arc 30.4 s, cexp 167.9 s, clog_csqrt 350.1 s, all ok. bessel_n and the job's rc are in `~/logs/test-long-sweeps-run-2-runner.log`.
- `-gate` (affected --run --base origin/next): its result is in jobs.tsv, see the hand-back.

## Open / follow-ups

- Schedule the weekly exhaustive job (ops side, #4750).
- TEST-LONG-SWEEPS-BOUNDARY (#4752): crates/boundary's ignored "accuracy job" tests (`exp_quick.rs`,
  `trig_quick.rs`) and env-scaled ones (`hyperb_quick.rs`, `invhyp_quick.rs`): extend `SWEEPS` with a package
  column and the static check to crates/boundary.

Next step: the coordinator schedules the weekly exhaustive job; then TEST-LONG-SWEEPS-BOUNDARY.
