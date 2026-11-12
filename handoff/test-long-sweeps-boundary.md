# TEST-LONG-SWEEPS-BOUNDARY: every crate's long and environment-scaled tests under long-sweeps (#4752)

State: branch `task/test-long-sweeps-boundary` (cut from origin/next), code head 390fa3e91 (this note and the
report are on top of it). Tests and runner only (`xtask/src/longsweeps.rs`, `xtask/src/main.rs` row), no libc
code. Brief `~/state/briefs/TEST-LONG-SWEEPS-BOUNDARY.md`. No decision record (a local test runner; the earlier
TEST-LONG-SWEEPS-RUN wrote none either). Background: `handoff/test-long-sweeps-run.md`.

## What changed

- The static check `long-sweeps` now scans `src` and `tests` of EVERY directory under `crates/` (it covered
  `crates/math` only). Crates it newly covers that have ignored or scaled tests: `crates/boundary`,
  `crates/core`, `crates/spectest`. The others (kernel, abi, rt, mem, legacy, ld, ld-so, thread-db, native,
  mathtools, libc-so, libm-so) have none today and are scanned too.
- Scaled tests: a test that reads a number from an environment variable (`env::var("V")` and a `parse` on the
  same line, in its body or, transitively, in a same-file function it calls) must be a `SWEEPS` row whose `env`
  sets every such variable (its full size), or a `NOT_SWEEPS` row with why. A row that sets a variable its test
  does not read is rejected too. So an ignored test whose size depends on a variable the runner does not set
  fails the check, and so does any non-ignored test that scales with one and has no row.
- Rows carry `env` (set on the run) and `secs` (measured test time); the package comes from the file's
  `crates/<dir>/Cargo.toml`; runs use `--include-ignored --exact`, so a scaled test that is not ignored runs too.
  `--only <test>` (repeatable) runs or lists named rows; `--list` prints package, env and secs.
- New rows, all in group `exhaustive` (the weekly job; the nightly is unchanged): boundary exp_quick, trig_quick
  (10^8, their defaults), hyperb_quick x2 and invhyp_quick and erf_quick (10^6 per region, chosen: their docs say
  "a larger value" with no number), log_quick x2 and math quick_log (`LIELLA_QUICK_LOG_N=10000000`, the
  documented 10^7 run), pow_quick (`LIELLA_POW_SWEEP=25000000`, documented), s10_math_symbols fast-entry sweep,
  9 one-pass sweeps (`LIELLA_SWEEP_STRIDE=1`), atan2f pairs, hypotf sweep; core `glibc_diff` and printf
  differential (defaults). Existing math rows now set `LIELLA_BESSEL_N_SWEEP` and `LIELLA_SWEEP_STRIDE=1` (the
  check found they read them).
- NOT_SWEEPS additions: exp_quick count loop (perf loop), spectest's `$name` pending stubs, core fixtures
  (stdio_short_io reference_values, enomem harness case, s23 DATEMSK child), three tests that pass a child an
  expected value (no_proc_lane x2, s25_2_c_3), and math fast_special/fast_bessel/fast_gamma (by-hand widening, no
  long size documented: see ruling below).

## Jobs (mei role s4)

- `tlsb-lint3` (head 390fa3e91): `cargo xtask clippy` and `cargo xtask static` rc 0.
- `tlsb-plants3` (390fa3e91): 5 plants, 5 CAUGHT, 0 WARN (`handoff/TEST-LONG-SWEEPS-BOUNDARY-plants.txt`).
- `tlsb-gate3` (390fa3e91): `affected --run --base origin/next`; result in jobs.tsv (see hand-back).
- `tlsb-sweeps` (rows from head 7b62dd4f6, test code identical to the final head): one run of the 24 new rows,
  RAM copy, 6-CPU slot, load 15-25. 10 rows done, all ok, test seconds recorded as `secs`: log_family FMA bound
  484, trig 58, pow 36, hyperb 23, invhyp 16, log_family builds 12, exp 5, erf 5, math quick_log 3, hyperb ranges 1.
  Still running at hand-back: the 12 s10_math_symbols rows (the fast-entry sweep alone ran over 45 min) and the
  two core rows. Log: mei `~/logs/tlsb-sweeps.log` (`grep -E "^long-sweep:|finished in"`).
- Superseded: tlsb-xt1/xt2 (dev runs), tlsb-lint (lab-names: a host name in a comment), tlsb-plants (patterns
  stale after the secs edit), tlsb-plants2 (one survivor, fixed), tlsb-gate/tlsb-gate2 (stopped, older heads).

## Open

- RULING NEEDED: full sizes for math fast_special/fast_bessel/fast_gamma (`LIELLA_*_SWEEP`, NOT_SWEEPS today;
  fast_special's comment suggests 10^7 by hand). Recommend: rows at 10^5 after one timing run.
- RULING NEEDED: the quick-phase rows that take under a minute (exp, erf, hyperb, invhyp, pow, quick_log) could
  move to the nightly `long` group. Recommend: yes for those under 60 s; kept weekly here (no nightly cost).

Next step: when `tlsb-sweeps` ends, add `.secs(n)` (its `finished in` figures, rounded up) to the 14 remaining
rows with one commit; nothing else to redo.
