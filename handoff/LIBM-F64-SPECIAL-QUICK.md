# LIBM-F64-SPECIAL-QUICK: erf/erfc quick phase (part 1 of the special-function quick phases)

State: branch `task/libm-f64-special-quick` from origin/next 7d31b0621; code commit baeb0fbe6; role shiki:~/w/sh1.
Decision D-10.84 (`records/decisions/D-10/D-10.84.toml`); also D-10.13 (erf fast tier), D-10.51 (two-phase double).

## Profile before (Zen 1 build host, `validation/percall/libm_erf.c`, instructions per call, glibc / Liella)

| case | FMA | SSE2 (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`) |
|---|---|---|
| erf (bench, [-5, 5]) | 157.9 / 162.0 | 172.6 / 235.0 |
| erf_table | 159.3 / 162.0 | 174.0 / 235.0 |
| erf_series (< 2^-5) | 63.0 / 109.0 | 63.0 / 153.0 |
| erfc (bench) | 162.4 / 197.4 | 177.1 / 307.0 |
| erfc_below1 | 162.4 / 173.4 | 177.1 / 249.1 |
| erfc_above1 | 194.3 / 244.0 | 214.1 / 422.0 |

Job `libm-f64-special-quick-base` (also a one-call wl_trace of erf and erfc: the call runs `taylor`'s degree-14
Horner chain, ~80 cycles of latency on Ivy Bridge, then four double-double steps, each a Dekker product in SSE2).
Target (DESIGN §5.4): the table path's FP ops from ~130 (SSE2) / ~58 (FMA) to ~45 / ~30, critical path from
14 dependent mul+add pairs plus four Dekker steps to ~10 dependent ops (Estrin), one Dekker product off the path.

## What is in (commit baeb0fbe6)

* `crates/math/src/fast_special.rs`: `erf_quick_dd` (ERF_T row to degree 12, Estrin split a/b1/b2 joined by t⁴,
  `P0 + P1h·t` exact by one product and Fast2Sum, low parts first, `t²·r` last), `erf_quick`, `erfc_quick`
  (`1 ± erf`, −6 < x < 1), `erf_tiered`/`erfc_tiered` (quick, then the unchanged `erf_nearest`/`erfc_nearest`).
  Bounds `RHO_ERF_Q = 2^-62` (proved 2^-62.017 worst slot, near x = 1), `RHO_ERFC_Q_LOW = 2^-61` (proved 2^-61.55),
  multipliers checked exactly. No new table.
* `crates/mathtools/src/special_tables.rs`: `chain`, `quick_bound` and test `quick_tests::quick_phase_bounds`
  (per-slot proof with the generator's exact slot data; test-only code, the generated table is unchanged).
* Wiring `crates/boundary/src/hot/erf.rs` (both builds call `*_tiered`); test `crates/boundary/tests/erf_quick.rs`
  (both builds, 280k operands each, entries vs the accurate path). Math unit test: fallback 542 of 63246 (0.86%)
  with the earlier, looser multipliers.

## Jobs (shiki)

`-gate` (lint.sh: records check, the three new tests, `cargo xtask clippy`, `cargo xtask static`; then `cargo xtask
affected --run --base origin/next`), `-plants` (2 plants: quick rounding test skipped, polynomial's last term dropped),
`-counts` (both builds, rc 0). After (glibc / Liella): FMA erf_table 159.3 / 118.0 (was 162.0), erf 157.9 / 118.0,
erfc_below1 162.4 / 131.3 (173.4), erfc 162.4 / 170.5 (197.4), erfc_above1 194.3 / 250.0 (244.0, +6), erf_series
63.0 / 113.0 (109.0, +4); SSE2 erf_table 174.0 / 143.0 (235.0), erf 172.6 / 143.0, erfc_below1 177.1 / 156.5
(249.1), erfc 177.1 / 245.0 (307.0), erfc_above1 214.1 / 426.0 (422.0, +4), erf_series 63.0 / 155.0 (153.0, +2).

**Blocking (why INCOMPLETE): two rows got slower** (erfc_above1, erf_series): `*_tiered` repeats the range checks
before the unchanged tier. Exact fix: drop `erf_tiered`/`erfc_tiered` and call the quick phase INSIDE
`erf_nearest` (after its `ax >= ERF_ONE` branch: `if ax >= SMALL { if let Some(v) = fast64::near(erf_quick_dd(ax),
C_ERF_Q, 0) { return signed v } }`, before `erf_dd`) and inside `erfc_nearest`'s `x < 1` branch (after the `-6`
check, same for `ax >= SMALL`), so the series and above-1 paths see no new compare; rewire hot/erf.rs back to
`*_nearest`; plant targets unchanged; rerun clippy/static/gate/plants/counts. Superseded: `-lint1` (record `|`), `-lint2`,
`-lint3` (mathtools dead-code lint).

## Not done (next: LIBM-F64-SPECIAL-QUICK-2)

1. Booked timing on Ivy Bridge and N100 for erf/erfc: NOT run (budget). Target <= 1.3x IVB time.
2. erfc above 1 (`e^(−x²)·Y`, 40% of the bench's erfc calls; 244/422 vs 194/214): quick Y from ERFCX_T the same way
   (the bound at degree 12 is 2^-59.95, worst in the wide slots above 8: restrict to x < 8 or check per range) and
   `e^(−x²)` from the exp quick kernel with `zl` folded in as `y·(1 + zl)` (not through `rl`: `rl·rh` would be 2^-52).
3. erf's series below 2^-5 (SSE2 153 vs 63): `x·(Q0 + z·R)` with one exact product.
4. lgamma, tgamma, j0/j1/y0/y1: not started.

## Not to redo

Degree 11 does not meet 2^-60 on ERF_T (2^-57.9); the first quick_bound structure (Horner to P2, sum last) stalled at
2^-61.03 (rounding-limited at x ≈ 1, `h²·p2/erf ≈ 2^-11`): the `P2h + q` / low-parts-first order is what gets 2^-62.
