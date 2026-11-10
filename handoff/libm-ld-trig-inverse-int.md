# handoff: libm-ld-trig-inverse-int (LIBM-LD-TRIG-INVERSE-INT)

**State.** Branch `task/libm-ld-trig-inverse-int` (cut from origin/next 9af6dff86), role mei:~/w/s2. One WIP commit:
the x87 integer tier for atanl/atan2l/asinl/acosl/tanl in `crates/math/src/wide_int/arc.rs` (child module of
`wide_int.rs`, so it reuses the parent's `reduce`, `sin_poly`/`cos_poly`, `SC`, `round` and the 2/π table without
widening any visibility), wired in `crates/math/src/wide.rs` before each 384-bit route, and the plants file
`handoff/LIBM-LD-TRIG-INVERSE-INT-plants.txt`. Stopped at the context checkpoint (~160k of 200k, 60 calls).

**Method (in the module doc, with the proof).** One kernel A(y, x) = atan(y/x), 0 < y <= x: Tang's table
(atan(j/128), 128 rows, 2 KiB), r = (y − cx)/(x + cy) formed exactly on 2^-120-truncated operands, atan r = r·P(r²)
(7 odd terms, Horner, 3 steps in 64-bit words, 3 in 128). Division: `divsd` reciprocal estimate + one exact
integer residual (2^-101.8). Root (asin/acos, s = √(1 − a²), 1 − a² exact for |a| >= 1/2): `u128::isqrt` seed +
one `divsd` correction (2^-113.8). The math crate cannot issue `sqrtsd` (forbid(unsafe); `core` has no f64::sqrt on
1.99.1; `_mm_sqrt_sd` needs unsafe here). atan = A(|x|,1) or π/2 − A(1,|x|); atan2 the same then π − θ for x < 0;
asin = atan2(|a|, s); acos = atan2(s, |a|), π − that for a < 0; tanl: one reduction, sin|a| / cos|a| (swapped in
odd quadrants) by the same division. Proved: arc results within 2^-100.5, tan within 2^-94.1; rounding test 2^-92.

**Results so far.** `mei:libm-ld-trig-inverse-int-unit3`: constants pinned (ok; table and π from Python decimal
agree with the 384-bit arithmetic), parent tests ok; two TEST bugs found and fixed after it (mp::atan takes |t|;
the root test's end case 2^128 − 1 lies outside the root's domain [2^126, 2^128 − 2^60]).
`mei:libm-ld-trig-inverse-int-unit4` (the fixed tests, --nocapture prints worst errors) was QUEUED at hand-back:
read `~/logs/libm-ld-trig-inverse-int-unit4.log` first. Superseded: -unit, -unit2 (did not compile: `_mm_sqrt_sd`,
`f64::sqrt`), -unit3 (the two test bugs).

**Not done (next steps, in order).**
1. Read unit4; fix what it finds (only the kernel bound asserts are new: 2^-99.9 arc, 2^-94.1 tan).
2. `cargo xtask clippy` + `cargo xtask static` on mei:~/w/s2 (not run yet; the new file may need a codemap row).
3. Decision record: `cargo xtask records new decision D-10.75` (reserved), body = the module doc's method and
   citations (Tang ARITH 10 1991; Brent & Zimmermann 2010 ch. 3-4; Muller 2016 ch. 2), measurements.
4. Plants: `cargo xtask plants handoff/LIBM-LD-TRIG-INVERSE-INT-plants.txt --dir /mnt/zram/claude/LIBM-LD-TRIG-INVERSE-INT/plants-1`
   (five plants: table index, quotient low part, tan reduction low word, `near` always true, atan2 quadrant).
5. Long sweep: `cargo test --release -p liella-math --lib wide_int::arc -- --ignored` (10^7 per function, four
   modes, half in each function's hard region) as a job.
6. Direct-tier gate `cargo xtask affected --run --base origin/next`; percall counts; booked timing on Ivy Bridge
   and N100 (x87 rows are judged in cycles; target <= 1.5x glibc). If `isqrt` shows in the asin/acos profile, pass
   `sqrtsd` in from the boundary as `invtrig.rs` does (its `sqrt: impl Fn(f64) -> f64` parameter).

**Do not redo.** The constants (gen script logic: decimal at 200 digits, atan by argument halving + series, π by
Machin) are pinned by `arc_constants_are_the_rounded_values`. Decisions in force: D-10.52, D-10.56, D-10.19, D-10.75
(reserved, not yet written).
