# TIMING-SWEEP-LIBM-1107 (libm timing sweep, Ivy Bridge + N100)

State: branch task/timing-sweep-libm-1107, cut from origin/next 66b8d3ba. Commits: 4c0fec7c (sweep summary fix +
test + plants file), then this handoff, the results file and the report json. Results:
results/timing-sweep-libm-1107-table.md (summary, `sweep compare` output, per-host analysis, combined `## Table`).

## Commands and hosts
- Stage (mei:~/w/s1, job mei:tsl1107-stage rc 0): `cargo xtask bench --stage ~/work/TIMING-SWEEP-LIBM-1107/libm.tar
  --only cbench_libm` at 66b8d3ba (tree 6c7f3ee8bcf2fa04; tar sha256 e2126541...). Relayed via local scratch to
  kotori and nozomi, unpacked whole into ~/work/TIMING-SWEEP-LIBM-1107/tree, sha checked.
- Timing (each through `~/bin/timing`, script ~/work/TIMING-SWEEP-LIBM-1107/run.sh =
  `./xtask-bin sweep run --out ~/work/TIMING-SWEEP-LIBM-1107/out --only cbench_libm`):
  kotori:tsl1107-kotori (Ivy Bridge, Xeon E3-1265L v2), 02:37:28-02:39:15Z, rc 0, 5 rounds/row, load 0.06 at start.
  nozomi:tsl1107-nozomi (N100), 02:37:28-02:38:45Z, rc 0, 3 rounds/row, load 0.06 at start.
- Bookings: ops 101 (kotori) and 102 (nozomi), 150 min, cancelled by me when the runs ended. Both hosts were idle
  (slots-status: 0 running), so kotori was picked over nico (also idle) because the 1106/1106E sweeps ran there.
  Both jobs started while their bookings were still `settling`. `~/bin/timing` took every slot of each host, and
  nothing else ran on either host.
- Compare (kotori, staged xtask-bin, tables only): `./xtask-bin sweep compare --new ivb=../out/sweep.tsv --old-table
  ../base1105.md --label 1105`. base1105.md is generated from docs/benchmarks.md (appendix `cbench_libm`: instr ratio
  and the "now time vs glibc (E3-1265L v2)" column) into a `## Table` with `instr IVB`/`time IVB` columns.
  `--new n100=...` is refused ("no previous table for class n100"). That is correct: N100 has no libm baseline.
- Analysis script (scratch, not in the repo): ~/w/.scratch/TIMING-SWEEP-LIBM-1107/analyze.py (families,
  precisions, slowest 20, the combined table).

## Results (detail in the results file)
- Ivy Bridge: 5 faster / 1 parity / 160 slower (166 rows), median time ratio 6.98. 11-05 over the same rows:
  2 / 0 / 164. No regressions. Improved: atanhf, sinhf, tanhf, tgammaf. `exp` 1.86 -> 2.90 (>10% worse, still slower).
  9 reopen rows (instructions <= 1.05, time > 1.05): coshf, j0f, j1f, log10f, tanf, y0, y0f, y1, +1 (list (b)).
- N100: 2 faster / 4 parity / 160 slower, median 5.83.
- Spread: N100 median 0.009, p90 0.026. Ivy Bridge median 0.009, p90 0.366, max 0.944; a few rows are noisy at
  5 rounds (y0/y1 0.36).
- Family medians (Ivy Bridge / N100): long double 114.7 / 83.6, complex 274 / 239, f32 2.27 / 2.66.

## Tool fix (step 3)
The first real `sweep run` printed "all 166 selected ratio_host rows measured (2 faster, 7 parity, 160 slower)". Those
counts add up to 169: the libm bench also counts add_baseline, mxcsr_pair and mxcsr_wrapper_seq, which sweep.tsv
keeps as counted rows outside the selection. Fix (4c0fec7c): the summary counts only the selected rows and names
the other rows after the counts. Fixture test in `the_per_host_table_holds_ratios_spread_counters_and_flags`. Plants:
mei:tsl1107-plants rc 0, 3/3 CAUGHT, no WARN lines. clippy and static are clean on mei. Gate: mei:tsl1107-gate
(direct tier, --base origin/next), still running at hand-back.

## Open / next
- `sweep compare` has the same counting issue: it reads sweep.tsv only, so its counts and spread include the
  calibration rows ("judged 169", spread p90 0.824 against 0.366). Next step: a `selected` column in sweep.tsv that
  compare filters on (sweep.tsv format change, small).
- Libm work next: long double (x87) and complex families are the largest gaps. Look at the reopen list (b) rows
  (instructions at or below glibc, time above) and at the `exp` row that got slower since 11-05.
- Not to redo: the timing runs (rc 0 on 66b8d3ba); the 11-05 baseline conversion (analyze.py writes it).
