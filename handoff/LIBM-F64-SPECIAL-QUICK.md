# LIBM-F64-SPECIAL-QUICK(-2): erf/erfc quick phase (part 1 of the special-function quick phases)

State: branch `task/libm-f64-special-quick-2` (from `task/libm-f64-special-quick`, base origin/next 7d31b0621); code
head 130f3f328; role mei:~/w/s3. Decision D-10.84 (`records/decisions/D-10/D-10.84.toml`, corrected for the new
structure); also D-10.13 (erf fast tier), D-10.51 (two-phase double). Plants: handoff/LIBM-F64-SPECIAL-QUICK-2-plants.txt.

## What is in

* `crates/math/src/fast_special.rs`: the quick phase (`erf_quick_dd`, degree 12, Estrin, bound 2^-62 / erfc on (0,1)
  2^-61, proved per slot by `special_tables::quick_tests`) now runs INSIDE `erf_nearest`/`erfc_nearest`'s own
  `|x| >= 2^-5` branch (helpers `erf_quick_near`, `erfc_quick_near`); `erf_dd` split into `erf_table_dd` /
  `erf_small_dd`; the table fallback (`erf_table_near`, `erfc_table_near`) is `#[cold] #[inline(never)]` because
  sharing one body made LLVM keep the ERF_T row loads live across the quick phase (SSE2: ~10 stack spills, one-call
  trace, 172 vs 147). `erf_tiered`/`erfc_tiered` are gone; `hot/erf.rs` calls `*_nearest` again.
* Test `quick_phase_rounds_correctly` runs 10 hard cases (quick `(zh, zl)` on or within 2^-12 ulp of a midpoint, exact
  value on the other side; a 2^28-operand search found 3867, ~1 in 69000, worst 4.9e-4 ulp, inside the bound). The
  rounding-test plant is now CAUGHT (#4533).
* erfc percall MISMATCH (#4534) explained: `erfc(0x1.b12e88p+0)` (the erfc row's ex[17]): Liella 0x1.11c9655380463p-6,
  reference ...462p-6; libquadmath erfcq and erfcl both put the exact value 0.6948 ulp above ...462, so Liella's is the
  correctly rounded one (not a defect; on next too). `validation/percall/libm_erf.c` self-check now prints `%.15g`
  (as the long double probes); `crates/boundary/tests/erf_quick.rs::erfc_probe_operands_round_correctly` pins both
  checked operands in both builds. All check columns now `ok`.

## Counts (mei, Skylake-SP; instructions per call; glibc / next 7d31b0621 / head 130f3f328; job -counts3)

| case | FMA | SSE2 (`hwcaps=-FMA`) |
|---|---|---|
| erf_table | 159.3 / 162.0 / 120.0 | 174.0 / 235.0 / 148.0 |
| erf (bench) | 157.9 / 162.0 / 120.0 | 172.6 / 235.0 / 148.0 |
| erfc_below1 | 162.4 / 173.4 / 128.9 | 177.1 / 249.1 / 159.4 |
| erfc (bench) | 162.4 / 197.4 / 167.2 | 177.1 / 307.0 / 245.8 |
| erfc_above1 | 194.3 / 244.0 / 244.0 | 214.1 / 422.0 / 422.0 |
| erf_series | 63.0 / 109.0 / 97.0 | 63.0 / 153.0 / **154.0 (+1)** |

**Open (why INCOMPLETE): SSE2 erf_series is +1 against next** (FMA is -12). Ratchet: it must return to 153. The
tiered version (b3dfc3948's first form with shared finish) measured 153 but spilled on the table path; moving the
series branch first (130f3f328) did not change it. Next step: trace one SSE2 erf_series call on head and on next and
diff the streams. The head trace works (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA bash validation/wl_trace.sh --list
--spec validation/percall/libm_erf.c . erf_series erf` in the role); next's needs a git tree (wl_trace refuses the primed
`~/wl/src/<sha>` dir) or a disassembly of `~/wl/products/7d31b0621.../` (`erf_nearest`'s series block). Candidates: the
`?` early return of the cold call adding a block merge, or `erfc_low_near`/sign selection order.

## Jobs (mei)

`-lint0` (inherited head: clippy + static, rc 0), `-search1` (hard-case search, evidence), `-counts3`, `-gate3`
(records check, the changed tests, clippy, static, `affected --run --base origin/next`), `-plants3` (2 plants).
Superseded: `-gate`, `-plants`, `-counts` (b3dfc3948: SSE2 spills), `-gate2`, `-plants2`, `-counts2` (9f90d407a), `-trace`,
`-trace2` (diagnostics). Scratch probe for the next phase (not committed, untested): a per-call spec for lgamma/tgamma/
j0/j1/y0/y1 at the bench's ranges was drafted (validation/percall/libm_special.c shape as libm_erf.c, cases
lgamma/_pos/_neg, tgamma/_pos/_neg, j0, j1, y0, y1); add it and profile both builds first.

## Not done (next: LIBM-F64-SPECIAL-QUICK-3)

1. SSE2 erf_series +1 (above). 2. Booked timing Ivy Bridge / N100 for erf/erfc (target <= 1.3x IVB). 3. erfc above 1
(`e^(−x²)·Y`; plan in D-10.84's "Not in this decision"; quick Y from ERFCX_T restricted to x < 8, `e^(−x²)` from the
exp quick kernel with `zl` as `y·(1 + zl)`). 4. erf series below 2^-5 (SSE2 153 vs 63). 5. lgamma, tgamma, Bessel four.

## Not to redo

Degree 11 does not meet 2^-60 on ERF_T (2^-57.9); Horner-to-P2-sum-last stalls at 2^-61.03. Uniform operands do not
catch a skipped rounding test (about 1 hit in 69000): keep the hard cases. Quick phase and table path in one body
spills on SSE2: keep the fallback cold and out of line. Series-first block order alone does not fix the +1.
