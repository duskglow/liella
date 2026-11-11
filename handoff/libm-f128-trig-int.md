# handoff: libm-f128-trig-int (LIBM-F128-TRIG-INT)

**State.** Branch `task/libm-f128-trig-int` from origin/next 7d31b0621. Role shiki:~/w/sh2. Code: d48f8d531 (kernel and
wiring), e06fd174f (plants file, percall spec); then the decision record D-10.85 and this handoff (no code change).

**What it is.** `crates/math/src/wide_int/quad.rs`: the x87 integer sine/cosine method (D-10.52) on three 64-bit limbs
for binary128. `wide_int::sin_cos` sends the 113-bit format there; undecided operands fall through to the 384-bit
route unchanged. Shared with the x87 kernel: `TWO_OVER_PI` (448-bit window), the 128-bit `SC` table (quad adds the
signed third limb `SC_LO`), `mulhi`/`mul_wide`/`mulhi64`, and the test helpers (`Rng`, `Tally`, `MODES`, now
`pub(super)`). The kernels themselves are separate: two limbs against three (a three-limb x87 path would cost it).
Proof in the module doc: within 2^-156 relative; rounding test assumes 2^-152; `|d| < 2^-160` refused.

**Results (all on e06fd174f, shiki, Skylake-SP role).**
* Unit tests (`cargo test -p liella-math --lib wide_int`, debug): worst 2^-159.4, 0 undecided of 9600; near multiples
  every 61st exponent: closest |f| 2^-122, worst 2^-162.7.
* `libm-f128-trig-int-sweep` (release, `tier_agrees_with_the_wide_route_long`): 10^7 operands, 8x10^7 calls in four
  modes, 0 undecided, 0 refused, every result equal to the 384-bit route, worst error 2^-158.25 (bound 2^-156); near
  multiples at all 16 384 exponents (Muller ch. 11 continued fractions, denominators below 2^113): 131 072 calls,
  65 536 undecided by design (one of sin/cos per operand), worst 2^-187.78, closest |f| 2^-124, 0 refused.
* `libm-f128-trig-int-plants`: 3 CAUGHT (lowest reduction limb dropped, rounding test skipped, SC third limb dropped),
  0 WARN.
* `libm-f128-trig-int-gate` (direct tier, --base origin/next): 187 passed, 0 failed, peak 354M.
* `libm-f128-trig-int-percall` (`validation/percall/libm_f128_trig.c`, instructions:u, sd 0), glibc / base / new:
  sinf128 7250.6 / 82434.5 / 1257.3 (0.17x); cosf128 7247.6 / 82436.7 / 1257.2; *_1e10 ~7464 / ~73820 / ~1256;
  *_1e300 ~7144 / ~70494 / ~1252 (0.18x).
* `libm-f128-trig-int-static`: rc 0. Clippy (`cargo xtask clippy`) clean in the unit job.

**Decisions in force.** D-10.85 (this method), D-10.52 (the tier), D-10.56 (x87 kernel), D-10.19 (correct rounding).

**Next step: LIBM-F128-TRIG-INT-2.** (1) Booked timing of sinf128/cosf128 on Ivy Bridge and N100 (instructions say
0.17x; the 192-bit products are `mul`-heavy, so time it). (2) The other f128 functions on this kernel: tanf128
(quotient of the two), sincosf128 (one reduction for both), then the inverse functions (x87 `arc` module's method on
three limbs). (3) A saturation section for the row. Profile first: no per-phase split was taken here.

**Do not redo.** The constants (generator: Python decimal, 200 digits; the tests pin every one), the proof, the
continued-fraction sweep (`near_multiple` in quad's tests, 320-bit integers).
