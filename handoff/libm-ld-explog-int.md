# handoff: libm-ld-explog-int (LIBM-LD-EXPLOG-INT, -2, -3)

**State.** Branch `task/libm-ld-explog-int-3` (continues `task/libm-ld-explog-int-2`; origin/next merged at
`ed1256e33`, ruling #4253). Code head `088977d4b`; later commits are handoff/report only. Decision record D-10.73
(`records/decisions/D-10/D-10.73.toml`). Jobs ran on shiki:~/w/sh2.

**Done in -3.**
* -2's red gate (`libm-ld-explog-int-2-gate`, keke): every suite passed (8385 tests, 126 conformance); the one red
  was `cargo xtask static`, the bare `|` in the D-10.73 body, which -2's last commit `2340fc97a` had already fixed.
  On the merged head clippy, static and codemap are clean (`-lint2`); `cargo xtask codemap` wrote no change.
* #4209: the tables moved into `cargo xtask tables`. New family `crates/mathtools/src/ld_explog_tables.rs` (400-bit
  mp) writes `@generated` `crates/math/src/coeffs/wide_int_el.rs` (`WI_*` names, re-exported in `coeffs::tables`;
  `wide_int_el.rs` imports them under the old short names). C1/C2 are exact integer formulas; the generator checks
  `|y''·c1' − 1| <= 2^-7` at both ends of every first-step interval. Values unchanged:
  `constants_are_the_rounded_values` re-derives every one from the 384-bit mp and passes. The Python script is retired.
* #4266, the logl 0.064% undecided: **a test artifact, not a product cost.** The long test's near-1 band built `x`
  below 1 as `u64::MAX − d` (d odd), so the distance `(d + 1)·2^-64` was even; distances with `2·v2 >= log2` put
  `ln x` within the cube term of a representable value or midpoint (genuine hard cases, correctly sent on to the dd
  tier). With `1 − d·2^-64`, d odd, the long run (`-acc`, 10^7 operands × 4 modes per function) gives **0 undecided
  for logl in 4·10^7 calls**, and all 8 functions pass: undecided exp 0, exp2 0, exp10 4 (band 1, ~10^-7), expm1 0,
  log/log2/log10/log1p 0; worst error 2^-112.1 (exp family), 2^-103.6 (expm1), 2^-103.9 (logs). The test now prints
  undecided calls per band. No product code was tightened.
* Plants `handoff/LIBM-LD-EXPLOG-INT-3-plants.txt`: -2's 5 kernel plants and 2 generator plants (L2 units -> stale
  `tables --check`; C1[127] = 1 -> the |u1| check fires). First run (`-plants`): 6 CAUGHT, 1 WARN-NO-MSG (L2 at
  units 2^-134 tripped the generator's own 127-bit fit check, not the stale check); the plant now uses 2^-132
  (`-plants2`, same code head, plants file copied to the host because the gate held the role): 7 of 7 CAUGHT with their messages, no WARN.
* Percall (`validation/percall/libm_wide_dd.c`, already covering the 8 rows and 4 `_full` rows; no new file needed),
  base = origin/next (dd tier only) vs this head vs glibc, instructions:u per call (`-percall2`):

  | row | glibc | base (dd tier) | new | new/glibc |
  |---|---|---|---|---|
  | expl | 87.0 | 1072.4 | 605.0 | 6.95 |
  | exp2l | 68.0 | 1225.9 | 766.5 | 11.27 |
  | exp10l | 87.0 | 1223.9 | 765.5 | 8.80 |
  | expm1l | 75.0 | 1231.9 | 770.5 | 10.27 |
  | logl | 52.2 | 1179.5 | 547.9 | 10.50 |
  | log2l | 49.2 | 1208.5 | 589.9 | 12.00 |
  | log10l | 49.2 | 1212.5 | 592.9 | 12.06 |
  | log1pl | 50.9 | 1469.0 | 818.3 | 16.09 |

  (`_full` rows within 2 of these.) Every row is 38-55% below base, and no row is slower. glibc's x87 code uses few
  instructions, each with long latency, so only timing can judge the ratio. The SELF-CHECK MISMATCH lines (log10l,
  expm1l) are in glibc's last bit: base and new agree with each other and with the 384-bit route (D-10.19).

**Gate.** `libm-ld-explog-int-3-gate` (`affected --run --base origin/next`, shiki, code head 088977d4b): rc 0, 16 suites ok, 12131 passed / 0 failed, conformance 127 / 0; peak 15G (page cache included).

**Not done (next story).**
1. Profile one call per phase (validation/wl_trace.sh or wl_percall.sh): logl spends ~548 instructions and where
   they go (entry, decoding, kernel, rounding) is not measured yet: split it before any tuning.
2. Booked timing (x87: instructions mislead): Ivy Bridge (nico or kotori, the quieter) and N100 (nozomi), the 8
   rows of cbench_libm; target <= 1.5x glibc time per row. Then a record that `amends` D-10.73 with the timing line,
   and a saturation entry (`records new saturation SR-NEW --section <n>`).

**Do not redo.** The design and proof (module doc of `wide_int_el.rs`); the unit fixes of -2; the generator; the
near-1 band analysis above.
**Read.** D-10.52, D-10.56, D-10.73; `crates/math/src/wide_int_el.rs` (module doc); `wide.rs` `tiers()`.
