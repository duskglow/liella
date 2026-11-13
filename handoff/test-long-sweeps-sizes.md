# TEST-LONG-SWEEPS-SIZES: full sizes for the double special-function sweeps (#5061-#5063)

State: branch `task/test-long-sweeps-sizes` (stacks on TEST-LONG-SWEEPS-BOUNDARY-2's commits; cut from origin/next,
never merged). HEAD: see `handoff/reports/TEST-LONG-SWEEPS-SIZES.json`. Runner table and two test comments only
(`xtask/src/longsweeps.rs`, `crates/math/tests/fast_{special,bessel}.rs`), no libc code, no decision record (a local
test runner row). Background: `handoff/test-long-sweeps-boundary.md`.

## What was done

- #5061: `fast_special` `double_erf_erfc_fast_tier_matches_the_accurate_path`, `fast_bessel`
  `double_bessel_fast_tier_matches_the_accurate_path` and `fast_gamma` `double_gamma_fast_tier_matches_the_accurate_path`
  are no longer NOT_SWEEPS rows. Each is a SWEEPS row at 10^5 operands per function and rounding mode
  (`LIELLA_ERF_SWEEP`, `LIELLA_BESSEL_SWEEP`, `LIELLA_GAMMA_SWEEP` = 100000).
- Timings (kanon, Broadwell-EP Xeon E5-2699 v4, host idle: load 0.3 to 3.5 of 35 CPUs; libtest `finished in`).
  At the gate's default size (`tlss-small1`): erf 2000 1.17 s, Bessel 1500 1.51 s, gamma 1500 1.27 s. At 10^5,
  through `cargo xtask long-sweeps --only` (`tlss-rows1`, all ok): erf 59.44 s (erf 400000/400000 and erfc
  399997/400000 settled and identical), Bessel 98.49 s, gamma 84.19 s. Recorded rounded up: 60, 99, 85.
- Under #5062 (nightly only below 60 s) all three rows are in the weekly `exhaustive` group, not `long`. erf
  is at the limit (59.44 s rounds up to 60). The weekly group grows by 244 s.
- #5062 and #5063 (quick rows moved to nightly; the s10 and core rows timed) were already done on this branch by
  TEST-LONG-SWEEPS-BOUNDARY-2 (commits f9f9a495b, 38a1c74f3). Nothing was left to do for them.

## Jobs (kanon role kn1)

- `tlss-lint1` (head 8c2d14352) and `tlss-lint2` (code head ed8691bbe): `cargo xtask clippy`, `cargo xtask static`,
  `cargo test -p xtask --bin xtask longsweeps::` (7 passed) and `long-sweeps --group all --list`: rc 0.
- `tlss-small1`: default-size timing, rc 0. `tlss-rows1`: one run of each new row at 10^5, rc 0.
- `tlss-plants1`: REFUSED by the tool (the bessel pattern no longer matched after `.secs` was added); fixed.
  `tlss-plants2`: 2 plants, 2 CAUGHT, 0 WARN (`handoff/TEST-LONG-SWEEPS-SIZES-plants.txt`).
- No affected gate was run. The brief's jobs list (clippy, static, plants, row runs) does not include one, and the
  change touches the runner table and two comments only.

## Open

- INFORMATIONAL: to make erf nightly, a size of about 9*10^4 would take about 54 s. Recommend keeping 10^5 weekly as
  the ruling says.
- The weekly-job ruling from TEST-LONG-SWEEPS-BOUNDARY-2 (group longer than the 150 min slot cap) still stands.

Next step: none for this story.
