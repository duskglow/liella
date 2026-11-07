# LIBM-F64-EXP-QUICK-WIRE(-2): exp quick phase wired as tier 1, tier 2 on a miss (both builds)

State: branch `task/libm-f64-exp-quick-wire-2` (stacks on `task/libm-f64-exp-quick-wire`, LIBM-F64-EXP-QUICK-2);
code head 39b2271d0, handoff commits on top. Role mei:~/w/s3. Brief: `/home/claude/state/briefs/LIBM-F64-EXP-QUICK-WIRE.md`
("## The change"); continuation brief LIBM-F64-EXP-QUICK-WIRE-2. Decisions in force: D-10.51 (two-phase double
functions), D-10.54 (quick kernel constants and bounds), D-10.25/D-10.50 (two builds), D-10.3 (CR).

## What is in

* `crates/math/src/fast64/quick.rs`: `exp_tiered` in both builds: quick phase, on a miss `tier2(x)` (`#[cold]
  #[inline(never)]`; SSE2 `fast64::exp_nearest`, FMA `fast64::fused::exp_nearest`); outside `2^-60 <= |x| < 708`
  `None` at once. `crates/boundary/src/hot/exp_log.rs` `exp_nearest` calls it behind `dispatch::fma()`.
* `selftest::exp_tiered` (-2): the tiers as the entry calls them, in the build `set_math_fma` chose.
* Tests: `exp_quick.rs::exp_quick_misses_are_answered_by_tier_two` (-2; s10_0_f_0, D-10.51): 2·10^6 operands on
  [-10, 10]. SSE2: every quick answer is the tiers' answer, and the misses tier 2 answers are exactly the misses the
  tiers answer (11368 of 11386; the 18 others are declined by tier 2 too). FMA: 139 of 157 misses answered, the SSE2
  tier 2 also answers 139 of them (bound: >= 90%, since the two tier-2 builds may decline different operands next
  to a rounding boundary). `exp_entry_rounds_quick_misroundings_right`, `libm_fma_select.rs::libm_product_exp_quick_
  misses_take_tier_two` (count: misses 252, hits 106 instructions per call through the product, SSE2, mei; its doc
  now says it cannot tell tier 2 from the accurate path).
* Why the first run's tier2-skipped plant survived: the accurate path (`elem_fast_cold` -> `Real::exp` ->
  `fast("exp", .., fast64::exp)`) starts with the same double-double kernel, so a skipped tier 2 returns the same
  bits for little more than the cold envelope. The tier hook makes the tier observable; no count margin needed.

## Jobs (mei, final code head 39b2271d0)

lint0 `libm-f64-exp-quick-wire-2-lint0` (inherited head 88026568c: clippy + static rc 0), diag
`-diag` (rc 101: the first FMA bound, 99%, was wrong; evidence) and `-diag2` (rc 0, the numbers above), lint
`-lint` (full clippy, xtask clippy, static: rc 0), plants `-plants` (5 of 5 CAUGHT: tier1-on-miss,
tier2-skipped, tier2-skipped-fma, mxcsr-inverted, fma-without-tunable; file
`handoff/LIBM-F64-EXP-QUICK-WIRE-2-plants.txt`), gate `-gate`, counts `-counts` (cbench_libm `--compare --counters`,
plain then `-FMA`; tables in mei:~/work/LIBM-F64-EXP-QUICK-WIRE-2/counters-{fma,sse2}.tsv), stage `-stage`.

Results: lint rc 0; plants rc 0 (5/5 CAUGHT, no WARN); gate rc 0 (`affected: ok`, peak 12G, oom_kill 0); counts
rc 0. Counts (Skylake-SP, cbench_libm, instructions per call Liella / glibc): exp 94.2 / 67.1 (1.40), exp2 125.3 /
51.1, exp10 141.5 / 56.1 (exp2/exp10 unchanged by this story). The `-FMA` run printed the same exp count (94.2):
the tunable most likely does not reach the bench programs (xtask sets the bench environment), so the SSE2 build's
cbench count is NOT VERIFIED (its product count, through the test above, is 106 per hit). No cbench count of the
base exists (the first run's counts job never started), so "no row slower" rests on the timing below.

Timing (staged at 39b2271d0, tar sha256 14ac23c5..., `./xtask-bin sweep run --only cbench_libm`, bookings 122/123
cancelled after): Ivy Bridge (kotori, E3-1265L v2) exp time ratio 1.889 (cycles 1.649, instr 1.339; spread 0.675,
5 rounds; ns median Liella 31.76 vs glibc 13.59), against 2.90 in the 11-07 sweep. N100 (nozomi) exp time 2.174
(spread 0.006; ns 17.39 vs 8.00). Target (~1.4x on Ivy Bridge, DESIGN.md §4.1) not reached: the remaining gap is
the 21-instruction envelope plus the kernel; profile one call again before the next design.

## Envelope rulings (owner's call, #3938/#3939; not changed, no inline asm added)

From the base trace (Zen 1, one call: entry/envelope 18 + `liella_machine_mxcsr_get` 3 = 21 of 125 FMA / 179 SSE2),
estimated from the code, NOT measured (measuring needs the inline asm the coordinator held back):
* Inline `stmxcsr` in `fpu::peek`: removes `call` + `ret` (2 instructions per call, ~1.6% FMA / 1.1% SSE2); the
  memory round trip of `stmxcsr` stays (it has only a memory form).
* Register-only barrier for `black_box(x)`: removes one store and one load (2 instructions) and the
  store-to-load forwarding delay on `x`, which is on the critical path of every call (about 4-5 cycles on these
  cores, so likely more in time than in instructions).

## Next step

1. Timing verdict (above) and the decision on landing. 2. exp2/exp10 on the same table (same kernel shape, own
constants). 3. The two envelope rulings with the measured numbers once the owner allows a prototype.

## Not to redo

The kernel, its bound and tables (D-10.54); the base profile; the tier test (do not go back to a count margin for
"tier 2 skipped"). Any quick.rs edit needs `cargo xtask kernel-index`.
