# LIBM-CPLX-LOG-SQRT handoff: clog/csqrt (and clogf, csqrtf) quick tiers

State: branch task/libm-cplx-log-sqrt-2 (stacked on task/libm-cplx-log-sqrt, cut from origin/next 2bc03ede6), HEAD in
handoff/reports/LIBM-CPLX-LOG-SQRT-2.json. Role shiki:~/w/sh2. Decisions in force: D-10.53 (design), D-10.74 (this
story's record, records/decisions/D-10/D-10.74.toml: windows, bounds, Kahan 1987, Dekker 1971, de Dinechin-Lauter-Muller
2007), D-10.55, D-10.25.

## Done

* Run 1 (LIBM-CPLX-LOG-SQRT): code, proofs and tests for the four tiers (`crates/math/src/complex_fast.rs`,
  `fast32/clog_csqrt.rs`, `complex_fast_ls_tests.rs`, boundary `hot::csqrt_nearest`/`csqrtf_nearest`, s10_math rows).
* Run 2 (LIBM-CPLX-LOG-SQRT-2): clippy + static on the inherited head (green); D-10.74 record; plants; gate; counts;
  the 10^7 sweep job; booked timing (cexp/cexpf step 0 and the four new rows).

## Results (run 2, head 1b7b2706b code; later commits are handoff/probe only)

* Gate `libm-cplx-log-sqrt-2-gate2` (shiki): direct tier, 10 suites, 3690 passed, 0 failed; conformance 19/19. Run
  with `--files` naming every changed file except `records/decisions/D-10/D-10.74.toml`: `affected` does not know
  `records/` paths and escalates them to FULL (first attempt `-gate`, killed at the escalation). The record is
  checked by `cargo xtask records check` + `static` (`libm-cplx-log-sqrt-2-static`, green).
* Plants `libm-cplx-log-sqrt-2-plants`: 6 of 6 CAUGHT, no WARN (handoff/LIBM-CPLX-LOG-SQRT-2-plants.txt has
  which test caught each).
* Counts `libm-cplx-log-sqrt-2-percall2` (instructions:u per call, Zen 1 host, glibc dynamic; probe
  validation/percall/cplx_log_sqrt.c, cbench_libm's own 256 operands):

  | case | glibc | base (route) | new | new/glibc |
  |---|---|---|---|---|
  | clog_bench | 417.7 | 74248.0 | 519.9 | 1.24 |
  | clogf_bench | 274.4 | 73244.6 | 276.1 | 1.01 |
  | csqrt_bench | 146.2 | 41205.5 | 307.2 | 2.10 |
  | csqrtf_bench | 127.5 | 41306.7 | 106.0 | 0.83 |
  | clog_axis (y = +0) | 199.6 | 663.0 | 745.0 | 3.73 |
  | csqrt_axis (y = +0) | 58.0 | 3669.7 | 3759.7 | 64.82 |

* #4206 (clog's atan2 window `min/max >= 2^-60`): the bench rows never hit it. Their operands are uniform in
  [-3, 3]^2 from a 53-bit generator (resolution 6·2^-53 ≈ 2^-50.4), so no part is below 2^-50 unless it is exactly 0,
  which this seed does not produce; every bench operand is in the tier's window. The bench is fair for the
  tier, but it does not cover the axes: an exact-zero part (a real operand passed as complex, common in practice) is
  outside the window and goes to the route, now paying the declined tier (+82 clog, +90 csqrt instructions, see
  `_axis`). csqrt on the real axis is 65x glibc by instructions.
* Timing (booked; staged head 1b7b2706b; `bench --no-build --compare cbench_libm --rows ...`, ns per op, Liella /
  glibc dynamic; jobs `nozomi:libm-cplx-log-sqrt-2-n100b`, `nico:libm-cplx-log-sqrt-2-ivb`). Step 0 (cexp/cexpf) is
  the first booked timing of LIBM-CPLX-EXP-TIER-2's tier:

  | row | Ivy Bridge | N100 | instructions (Zen 1) |
  |---|---|---|---|
  | cexp | 539.2 / 100.8 = 5.35 | 186.6 / 32.7 = 5.70 | 397 / 251 = 1.6 (EXP-TIER-2) |
  | cexpf | 128.2 / 114.5 = 1.12 | 57.7 / 57.9 = 1.00 | 182 / 173 |
  | clog | 274.1 / 210.3 = 1.30 | 137.0 / 69.3 = 1.98 | 1.24 |
  | clogf | 174.3 / 182.3 = 0.96 | 78.1 / 80.1 = 0.97 | 1.01 |
  | csqrt | 294.4 / 75.0 = 3.93 | 112.1 / 28.6 = 3.92 | 2.10 |
  | csqrtf | 129.2 / 103.3 = 1.25 | 60.9 / 57.7 = 1.06 | 0.83 |

  Before the tiers these rows were 115-240x by time. Brief target <= 2x: clog met on both hosts, csqrt not (3.9x).
  cexp and csqrt are 2-3.5x worse by time than by instructions. HYPOTHESIS, NOT VERIFIED: the row's average is
  dominated by the few bench operands the tier declines (one 384-bit route call is ~41k-116k instructions at low IPC;
  for cexp, run 1 of EXP-TIER-2 measured one such operand of 256 adding ~450 instructions to the average). If so,
  the bench row is fair (random operands, the decline rate is what users meet), and the fix is a cheaper tier between
  the quick tier and the 384-bit route (a double-double/triple-double accurate phase), not a faster quick tier.
  First check: per-operand counts of the 256 bench operands (which ones decline, and why). For csqrt the other
  candidate is latency: its tier is one dependent chain sqrt -> div -> sqrt -> div (`sqrt_dd` twice, `div_b`), which
  instruction counts do not show; one decline in 256 (41k/256 = 160) would also explain 307 vs a ~150 tier.

## Next steps (in order)

1. csqrt/cexp time gap (LIBM-CPLX-DECLINES): count per bench operand which ones the tiers decline and what each
   costs; if the declines carry the time, design an intermediate accurate tier (one shared design for the group).
2. Axis fast paths (a new story, LIBM-CPLX-AXES): exact-zero parts answered in the tier, not the route:
   `csqrt(x ± 0i)` = `(sqrt x, ±0)` for `x > 0`, `(+0, ±sqrt(-x))` for `x < 0`; `clog(x ± 0i)` = `(log|x|, ±0 or ±π)`;
   the `x = ±0` axis likewise. Annex G signs; flags from the real function. Counts: `_axis` cases of the probe.
3. csqrt (2.10x by instructions) is above the brief's 2x target: profile `csqrt_bench` per phase (two `sqrt_dd`,
   `div_b`, two `round_root`, the hot entry's decline) before any change.
4. Continuations: LIBM-CPLX-TRIG (csin/ccos), LIBM-CPLX-POW, then the long double variants.

Do not redo: the bound derivations in complex_fast.rs, the atan2_abs split, the float margins, the plants.
