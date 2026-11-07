# LIBM-F64-POW-QUICK(-2): binary64 pow quick phase (tier 1), wired, plants/counts/timing done

State: branch `task/libm-f64-pow-quick-2` (= `task/libm-f64-pow-quick` + this run; stacks on LIBM-F64-LOG-QUICK-2 at
615d6a4f9). Role keke:~/w/r1. Briefs `/home/claude/state/briefs/LIBM-F64-POW-QUICK.md` (spec) and `-2.md`.
Decisions in force: D-10.51, D-10.54, D-10.60, D-10.62 (this kernel; measurements added this run), D-10.3,
D-10.25/D-10.50.

## What is in (unchanged from run 1)
* `crates/math/src/fast64/quick_pow.rs`: quick log with a degree-10 term -> `(lh, ll, r3)`; exact `y·lh`
  (2MultFMA / Dekker+Veltkamp); quick exp on `th` with `tl` folded into Cody-Waite; one-sided Ziv test with the
  per-call multiplier `c = RN(|y·r3|·89/16 + C0)`. Domain `pow_domain` first, `2^-60 <= |th| < 708`,
  `|y·r3| <= 2^-12`; else tier 2 = `fast64::pow_nearest`. Both builds. Wired in `hot/exp_log.rs::pow_nearest`.
* Tests: `quick_pow/tests.rs` (5), `crates/boundary/tests/pow_quick.rs` (2, clause s10_0_f_0).

## This run (-2)
* Inherited head lint + static: `libm-f64-pow-quick-2-lint` rc 0 (nothing red left over).
* Entry test gained three subnormal-`x` operands vs the correctly rounded pow: without them a bypassed
  `pow_domain` in `pow_tiered` survived by value (the quick log reduces only normal `x`). Commit 0ec1981a1.
* Plants `handoff/LIBM-F64-POW-QUICK-plants.txt`, job `libm-f64-pow-quick-2-plants`: 6 of 6 CAUGHT, 0 WARN
  (log-low-dropped, product-inexact, near-true-quick, near-true-tiered, domain-bypass-quick,
  domain-bypass-tiered; the last caught by the new subnormal check, `SSE2 pow(5e-324, 3e-1) subnormal x misrounded`).
* Counts (keke, Zen 1, cbench_libm `--compare --counters`, instructions per call Liella / glibc):
  FMA 177.5 / 141.3 (ratio 1.256; before 319 / 143), job `libm-f64-pow-quick-2-stage`
  (keke:~/work/LIBM-F64-POW-QUICK-2/counters-fma.tsv). SSE2 247.7 / 191.5 (1.294; before 494 / 193), job
  `libm-f64-pow-quick-2-counts-sse2` (staged tree, `--tunables glibc.cpu.hwcaps=-FMA`; counters-sse2-tunables.tsv).
  The plain `GLIBC_TUNABLES=... cargo xtask bench` run does NOT pass the tunable (xtask clears the bench env; it
  printed the FMA numbers again): use `--tunables`. Brief target <= 170 FMA: not reached (177.5).
* Timing (stage sha256 6301875f..., commit 0ec1981a1, `./xtask-bin sweep run --only cbench_libm`, 5 rounds):
  Ivy Bridge (nico, E3-1220L v2) pow instr / cycles / time 1.293 / 1.408 / 1.411 (spread 0.009; before 3.35);
  N100 (nozomi) 1.256 / 1.478 / 1.478 (spread 0.006; before 3.59). Target <= 1.5x met on both. Controls: exp
  1.70 / 2.17, log 1.95 / 2.12 (IVB / N100 time). Bookings 136 (moved), 137, 138 cancelled.
* Gate `libm-f64-pow-quick-2-gate` (`affected --base origin/next --run`, keke) and the 10^8-operand sweep
  `libm-f64-pow-quick-2-sweep` (both pow_quick tests, release, `LIELLA_POW_SWEEP=12500000` = 5·10^7 operands per
  build over the 4 sets, RAM copy /dev/shm/claude/LIBM-F64-POW-QUICK-2/sweep-1): see the hand-back for their state.

## Open / next
* Instruction target 170 (FMA) missed by 7.5; profile one call with wl_trace (not done in either run) before any
  tweak: the entry envelope (~21 instructions, rulings #3938/#3939) is the likely remainder, as for exp/log.
* Directed rounding modes do not reach the fast path (elem_fast needs the default MXCSR); pow_upward is
  LIBM-DIRECTED-INPLACE's.
* Saturation register row for pow (docs/saturation/16-...) not updated.

## Not to redo
The composition and its bound (D-10.62), the plants above, the counts and timing above.
