# LIBM-F64-INVTRIG-QUICK: double asin/acos/atan/atan2 quick phase (part 2 of LIBM-F64-INVHYP-QUICK)

State: branch `task/libm-f64-invtrig-quick` (from origin/next), role mei:~/w/s4, brief
`~/state/briefs/LIBM-F64-INVTRIG-QUICK.md`. Decisions in force: D-10.69 (this story,
`records/decisions/D-10/D-10.69.toml`), D-10.13 (fast tier = accurate path's bits), D-10.51 (two-phase
double), D-10.25 (two builds). Head and job results: section "Jobs" below.

## What is in

* `crates/math/src/invtrig.rs`: `atan_quick` (both builds, in the `kernels!` macro) runs first on the
  reduced argument each entry already forms (`atan` below and above 1, `asin`'s and `acos`'s ratios,
  `atan2`'s quadrant tables), with the same rounding test `done` (`EPS = 2^-62`); on a refusal
  `two_tier` runs today's `atan_unit` (tier 2, unchanged) on the same argument, then the entry's accurate
  path. No boundary change: `hot::invtrig` still calls `invtrig::*_nearest`.
* Method (module comment "Quick phase" has the bound term by term): `c = j/64`, `h = t − c` exact,
  `1 + c t = (1 + c²)(1 + g)`, `v = h·R·(1 + s)`, `R = 1/(1 + c²)` from `RECIP` (65 × `[rh, rl]`,
  1040 bytes, const-built; `rh` cut to 26 bits), `s = −g + g²(1 − g)(1 + g² + g⁴)` (series to `g^7`),
  `h·rh` exact by one FMA or `prod_short` (mask split, two exact partial products, no Veltkamp), the
  cubic `v³P(v²)` on `RN(v.hi + vl)`. No division in the kernel (today's kernel has two).
  Bound `2^-63` relative; the reduction alone measured `2^-64.75` (Python double simulation, 2·10^5
  operands, against exact rationals).
* Op count, quick kernel: about 30 FP ops (FMA build), about 36 (SSE2), no division; critical path
  index → `h` → `g` → `s` (4 dependent) → `vl` → `vt` → cubic (4) → two Fast2Sums, about 18 dependent
  ops. The entries still pay their own divisions/roots before it: `atan` `|x| > 1` one division plus
  a product; `asin` one root and `div_dd` (one division); `acos` `div_dd` and `sqrt_dd`; `atan2` two
  divisions (`th` and `tl`).
* Tests `crates/math/tests/invtrig.rs`: new `double_atan_quick_phase_answers_alone_and_agrees_with_the_
  multiprecision_atan` (the hidden entry `atan_quick_only`, no tier 2: >98% answered, each equal to
  `mp::atan` at every node `j/64`, 64 operands around each, 4096 log-uniform). The existing three
  tests cover the four entries in both builds (bit-equality with the accurate path, hard cases).
* Two bugs found on the way (both now plants): `h·rl·s` cannot be dropped (`rl` is `2^-26 R`, so it is
  inside `w`), and `vl` cannot be dropped from the cubic (`vl ≈ −g·v`, up to `2^-8 v`).

## Jobs (mei, code head 15b4e9af8; later commits add handoff/ only)

* `libm-f64-invtrig-quick-gate2` rc 0: records check, `cargo xtask clippy`, `cargo xtask static`, then
  `affected --files crates/math/src/invtrig.rs crates/math/tests/invtrig.rs --run` (direct tier, 44 of
  250 targets). Plain `affected --run` (`-gate`, killed) escalated to FULL because the new
  `records/decisions/D-10/D-10.69.toml` is "not a path the selection knows": a selection gap for record files.
* `libm-f64-invtrig-quick-plants` rc 0: 8 of 8 CAUGHT, no WARN (`handoff/LIBM-F64-INVTRIG-QUICK-plants.txt`:
  the brief's four (asin regime, table index, near always true, atan2 quadrant) plus `rl`, cubic, series and
  `prod_short` terms).
* `libm-f64-invtrig-quick-counts` (head) and `-counts-base` (the same role with this branch's `invtrig.rs`
  diff reversed; role resynced afterwards), Zen 1 build host under load ~59/48, instructions per call,
  Liella base → head (glibc):

  | row | FMA build | SSE2 build (`glibc.cpu.hwcaps=-FMA`) |
  |---|---|---|
  | asin | 183.2 → 178.9 (75.0) | 310.4 → 279.5 (80.2) |
  | acos | 178.8 → 177.1 (80.1) | 282.0 → 259.5 (83.6) |
  | atan | 142.0 → 137.2 (78.7) | 213.1 → 188.6 (109.8) |
  | atan2 | 166.4 → 165.0 (161.7) | 239.6 → 226.3 (197.9) |

  No row slower. Time ratios on that busy host (not a verdict): FMA asin 9.31 → 8.59, acos 7.65 → 7.35,
  atan 3.05 → 2.75, atan2 1.73 → 1.62; SSE2 asin 10.37 → 9.37, acos 8.88 → 8.17, atan 2.56 → 2.07,
  atan2 1.76 → 1.47. Targets (asin/acos <= 1.5x, atan/atan2 <= 1.3x) NOT reached.
* Superseded: `-t1`, `-t2` (the two kernel bugs below), `-t3` (pre-commit check), `-gate` (escalated to FULL, killed).

## Where asin/acos time goes (by elimination, not profiled)

The kernel is now cheap; the gap is the entries' reductions: asin `(1 − a)(1 + a)` as a pair, `sqrt_dd`
(a root and a division), `div_dd` (a division and an exact product), all on the critical path before
the kernel, about 180 cycles per call on the build host against glibc's ~25. A per-phase split
(`validation/wl_percall.sh`) was not run: do it first in the continuation.

## Not done (continuation)

1. Booked timing (Ivy Bridge, N100) not run; the 10^8-operand accuracy jobs in four rounding modes not
   run (the entries are round-to-nearest only through `elem_fast`; other modes go to the accurate path).
2. If counts show asin/acos still far from 1.5x: their cost is now the reduction (`sqrt_dd`, `div_dd`
   with Dekker products in SSE2), not the kernel. Next design: `asin x = x + x³P(x²)` for `|x| < 1/2` and
   `π/2 − 2 asin(sqrt((1 − x)/2))` above (brief), needing a minimax `P` (no generator was at hand: no
   mpmath/Sollya on the workstation) with the first two terms in double-double; or feed `atan_quick` a
   cheaper ratio (`th = RN(a/s)` and its residual by one FMA).
3. atan2's `tl = rem/d` (second division) could be `rem·RN(1/d)` only with a bound check.

## Not to redo

The two dropped-term bugs above; `prod_short` needs `rh` at most 26 bits (the table cut) and `hh` 27.
