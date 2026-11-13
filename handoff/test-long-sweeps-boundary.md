# TEST-LONG-SWEEPS-BOUNDARY(-2): every crate's long and environment-scaled tests under long-sweeps (#4752)

State: branch `task/test-long-sweeps-boundary-2` (TEST-LONG-SWEEPS-BOUNDARY's branch plus this run; cut from
origin/next, never merged). HEAD: see `handoff/reports/TEST-LONG-SWEEPS-BOUNDARY-2.json`. Tests and runner
only (`xtask/src/longsweeps.rs`), no libc code, no decision record (a local test runner). Briefs
`~/state/briefs/TEST-LONG-SWEEPS-BOUNDARY.md` (the change) and `-2.md`. Background: `handoff/test-long-sweeps-run.md`.

## What the story did (both runs)

- The static check `long-sweeps` scans `src` and `tests` of every crate under `crates/` (it covered math only).
  Ignored tests, and tests that read a size from an environment variable (in the test or a same-file helper it
  calls), must be a `SWEEPS` row setting every such variable, or a `NOT_SWEEPS` row with why. Rows carry `env`
  and `secs`. `--only <test>` runs or lists the named rows.
- Run 2: `secs` is `Secs::{Unmeasured, Took(n), Over(limit)}`. Ruling #5062 is applied and checked: a measured
  row under 60 s (`NIGHTLY_SECS`) runs nightly in `long`. A row in `long` that took 60 s or more, or ran over its
  limit, is refused (`group_mismatches`, called from `mismatches`, so `static` and the unit tests both see it).
  Moved to `long`: boundary exp 5, trig 58, hyperb 23 and 1, invhyp 16, erf 5, log builds 12, pow 36; math
  quick_log 3; core printf differential 7.
- Run 2 timings (Broadwell-EP host kanon, 35 CPUs, each row alone on its 7-CPU slot under `timeout 1800`, from a
  RAM copy built once; scripts `~/work/TEST-LONG-SWEEPS-BOUNDARY-2/{prep,row,chain,launch}.sh` there, logs
  `~/logs/tlsb2-row*.log` and `tlsb2-chain*.log`; libtest `finished in`, rounded up). s10:
  double fast-entry sweep OVER 1800 (`.over(1800)`), expf 1136, exp2f 1155, exp10f 1187, logf 941, log2f 939,
  log10f 1058, asinf 1334, acosf 1135, atanf 1305, atan2f pairs 68, hypotf 864. Core: glibc_diff 102,
  printf differential 7.

## Why the last run's sweeps job ended rc 124

kanon's (and every host's) `~/bin/slots` runs each queued job under `timeout 150m`: one job running all 14 rows
in a row cannot finish (the s10 one-pass rows alone are ~3.5 h). The weekly `--group exhaustive` job has the
same problem: its measured rows add up to 13 508 s (3.75 h: s10 12 922 incl. the 1800 s over-limit row, core
102, log FMA bound 484), not counting the math fast32 exhaustive rows, which are not timed yet. See RULING NEEDED below.

## Jobs (kanon role kn1)

- `tlsb2-lint0` (inherited head a5136cbdb): clippy + static rc 0. `tlsb2-prep`: the RAM copy and its build.
- Timings: `tlsb2-row01`, `-row04` to `-row08`, `-row11` and `tlsb2-chain1` to `-chain4` (evidence).
  Stopped by me and retimed in a chain because their slot shared CPUs 0-6 with another job: row02, row10, row12,
  row13. Stopped before they started their test: row03, row09, row14, xt1. row05 (logf, 1087 s) shared CPUs for
  about 2 min at its start; it was retimed alone in chain4 (941 s, the recorded figure).
- `tlsb2-xt2`: `cargo test -p xtask --bin xtask longsweeps::` 7 passed, and `--list`.
- Code head 38a1c74f3: `tlsb2-lint1` (workspace clippy -D warnings, xtask clippy, static) rc 0; `tlsb2-plants1`
  5 plants, 5 CAUGHT, 0 WARN; `tlsb2-gate1` (affected --run --base origin/next) rc 0: 32 suites, 11658 passed,
  0 failed, conformance 125 passed, peak 16G with page cache, no OOM.

## Open

- RULING NEEDED: the weekly `exhaustive` group runs at least 3.75 h, more than the 150 min slot cap. Recommend: the
  weekly job runs one jobrun job per row (or a few chains of rows totalling under 150 min each), as this run did.
  Recommend also deciding whether the s10 double fast-entry sweep (`LIELLA_FE_SWEEP_CHUNKS=200`, over 30 min)
  keeps its size or is split into chunks.
- INFORMATIONAL: kanon's slot queue has N=5 slots but pins only 4 distinct CPU sets (0-27). A fifth concurrent
  job always got CPUs 0-6, already held by another job (#1185/#1189, #1193/#1194, #1193/#1195), and CPUs 28-34
  sat idle. Any timing taken while five jobs run there is suspect.
- Ruling #5061 (fast_special/fast_bessel/fast_gamma at 10^5) is TEST-LONG-SWEEPS-SIZES, not this story.

Next step: none for this story. TEST-LONG-SWEEPS-SIZES takes ruling #5061. Before a weekly job is scheduled, the
weekly-job ruling above is needed.
