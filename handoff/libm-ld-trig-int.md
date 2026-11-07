# handoff: libm-ld-trig-int (LIBM-LD-TRIG-INT, checkpoint hand-back)

**State.** Branch `task/libm-ld-trig-int` (cut from origin/next bb7f6ae1c). Role kanon:~/w/kn1. Code commits
07202bd37 (kernel + wiring) and 628a2ce84 (test fix); then this handoff. Context checkpoint (250k, Effort: max) reached.
* `kanon:libm-ld-trig-int-unit4` rc 0 on 628a2ce84's code: clippy -p liella-math clean; `cargo test -p liella-math
  --lib wide_int`: 4 passed, 1 ignored. Random operands 1200 (4 bands) x sin/cos x 4 modes = 9600 calls: 0 undecided,
  every result equal to the 384-bit route, worst raw kernel error 2^-96.1 (proof: < 2^-94). Near multiples of pi/2
  (continued fractions, every 61st exponent) + edge operands: 2216 calls, 1092 undecided BY DESIGN (one of sin/cos is
  +-(1 - r^2/2) within 2^-92 of a representable value), worst error 2^-102.4, closest |f| 2^-70 (the 2^-92 refusal
  is never reached). Superseded: unit (clippy expect_used), unit2/unit3 (undecided-rate assertion counted the
  by-design cases).
* `kanon:libm-ld-trig-int-gate` (xtask clippy + static + affected --run --base origin/next) started detached on
  628a2ce84's code; read it first.
* No plants, percall, long sweep or timing yet.

**What is there.**
* `crates/math/src/wide_int.rs`: the integer fixed-point tier for x87 sinl/cosl. Proof of the bound in the module doc
  (relative error < 2^-94; rounding test with 2^-92, err = 2^36 + 4 units of the 128-bit significand).
  - Reduction: Payne-Hanek for every |x| >= 2^-34 (one path, no separate Cody-Waite): 256-bit window of the existing
    generated table `t::TWO_OVER_PI` (coeffs/trig.rs, 312 words, already covers x87 and binary128; no new table),
    3 full + 1 low 64x64 products, fraction in units 2^-256, truncation < 2^-190.
  - Tang step on the quadrant fraction: j = round(128|f|), table `SC` of sin/cos(j*pi/256), j = 1..64 (2 KiB);
    d normalised (relative precision for j = 0); refuses |d| < 2^-92 (fallback).
  - Polynomials in u = d^2 with pi/2 folded into the coefficients (saves the r = f*pi/2 product): sin 5 terms, cos 6;
    inner steps in u64, last two in u128.
  - `round`: integer test + four integer roundings, packs the x87 bits directly (no `float::round`).
  - Hooked in `wide::sin` / `wide::cos` after the tiny-argument shortcut (sincosl gets it through sin + cos).
* Tests in the module: constants and the table pinned against mp; `two_over_pi_table_times_pi_is_two` (pi by Machin in
  320-word fixed point times the 2/pi table = 2, pins table bits 1..19905, independent of the generator);
  `rounding_test_refuses_near_a_boundary` (pins the bound value; compares with `float::round`);
  `tier_agrees_with_the_wide_route` (4 bands x 300 operands + edge operands + continued-fraction near multiples of
  pi/2 every 61st exponent, four modes, and the kernel's raw error against 2^-92); the ignored `_long` (10^7 operands +
  near multiples at every exponent; release, threaded).
* Constants: Python `decimal` script (not in the repo; scratch) - pi by Machin, sin/cos by Taylor, 160 digits.

**Next steps (in order).**
1. Read the result of `kanon:libm-ld-trig-int-unit` (`~/logs/libm-ld-trig-int-unit.log`); fix compile/clippy/test
   failures (fresh code, never compiled before this job).
2. `cargo xtask clippy`, `cargo xtask static`, gate `cargo xtask affected --run --base origin/next` (detached jobrun).
3. Long sweep as a job: `cargo test --release -p liella-math --lib wide_int -- --ignored` (prints worst error and
   closest |f|; put both figures in the module doc / D-row).
4. Plants file `handoff/LIBM-LD-TRIG-INT-plants.txt` (`cargo xtask plants`): (a) flip a bit in an early word of
   `TWO_OVER_PI` in crates/math/src/coeffs/trig.rs (caught by `two_over_pi_table_times_pi_is_two`; tier-vs-mp agreement
   alone cannot catch it, both use the table); (b) quadrant `let q = (l3 >> 62) as u32;` -> `+ 1` (agreement test);
   (c) `const ERR: u64 = (1 << (128 - B)) + 4;` -> `(1 << (127 - B)) + 4` (`rounding_test_refuses_near_a_boundary`;
   the agreement tests cannot see a halved bound, the measured error is far below it); (d) drop the carry
   `(s2 >> 64) as u64` into l3 in `reduce` (agreement). Carries in the low limbs move f by ~2^-126 only: equivalent
   for results, do not use them as plants.
5. Percall spec `validation/percall/libm_ld_trig.c` (sinl/cosl over [-10, 10], 64-bit significands, 1e10 and 1e300
   operands) via `validation/wl_percall.sh`; target <= 300 instructions (glibc 1119). Estimated ~200.
6. Timing (x87 rows judged in cycles): book an Ivy Bridge host (nico or kotori, less busy) and the N100, cycles per
   call of the percall binaries as in docs/saturation/21; then a saturation section + D-row text.
7. Decision row: write the method details (uniform Payne-Hanek, existing table reused, f-unit polynomials, bound 2^-94)
   as `D-NEW` in plan/07-decisions/s10-mathematics.md (no id reserved in the brief) or ask the coordinator for an id.

**Decisions in force.** D-10.52 (this tier), D-10.19 (correct rounding of the wide functions), D-10.25 (dispatch
pattern; no mulx/adx variant written: plain `mul` only).

**Do not redo.** The design and constants above; the closest-to-pi/2 search is in the test code (continued fractions of
frac(2^e*2/pi) from the table, convergent denominators < 2^62).
