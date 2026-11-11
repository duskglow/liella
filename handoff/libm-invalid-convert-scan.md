# LIBM-INVALID-CONVERT-SCAN (-2): handoff

State: branch task/libm-invalid-convert-scan-2 (stacked on task/libm-invalid-convert-scan, base origin/next bb9130ed9), head =
last commit on the branch. Decision in force: D-10.95 (records/decisions/D-10/D-10.95.toml, written in -2).
LIBM-LD-ARC-DIRECT had NOT landed on next when -2 ran (11-11 06:20): merge origin/next once it does (allowed) and keep one fix
per arc.rs site (its own path removed the root's as-u64 stall; big_u64 may then be dead at that site).

## Scan (done by type, not by text)
Inventory: `cargo clippy -p liella-math -p liella --lib -- -A clippy::all --force-warn clippy::cast_possible_truncation
--force-warn clippy::cast_sign_loss`; the `float-casts` static check runs it and now reports 15 casts in scope, each with a range.

| site | function | operand range | raises? |
|---|---|---|---|
| wide_int/arc.rs root_seed `(r*2^37) as u64` | asinl, acosl | [2^63, 2^64] every call | yes, every call: fixed (big_u64) |
| wide_int/arc.rs sqrt `vd as u64` | asinl, acosl | (2^63, 2^64] every call | yes: fixed (big_u64) |
| wide_int/arc.rs div `(2^115/dh') as u64` | atan2l, asinl, acosl | (2^62, 2^63] | at power-of-two divisors: fixed (big_u64) |
| wide_int/arc.rs soft_sqrt, atan_le_in | internal, atan | small | no (RANGE:) |
| fast32/gamma.rs eval_lgamma `as i32` | lgammaf sign | any f32 unclamped | clamped to [-2^24, 0] (NaN -> -2^24) |
| dd.rs, fast32/bessel.rs, fast_bessel_n.rs, mp.rs, special.rs | various | guarded | no (RANGE:) |
| core/num/parse.rs finish max10/min10 `as i64` (#4627) | strto* long path | abs < 5000 | no invalid (RANGE:); see strto* flags below |

## strto* flags (probe a7fa43af9749, settled; #4627 answer)
The two parse.rs casts cannot raise invalid. But the float product and truncation around them are the ONLY source of the
inexact flag in Liella's strto*: with them replaced by integer arithmetic (tried in -2, reverted), strtof("1") stops raising
inexact (matches the reference) and strtof("0.1") stops raising it too (the reference raises it). Today Liella raises inexact
on every strtof/strtold call and strtod calls off the short path, exact or not, and never overflow or underflow; the reference
raises inexact iff inexact, overflow on overflow, underflow on tiny inexact (IEEE 754 §7). `float::Rounded` already carries
inexact/overflow/underflow: the fix is to raise them from the boundary's strto*/wcsto* wrappers (one feraiseexcept-like op
after rounding) and then drop the float arithmetic in `finish` (`trunc(e*30103/10^5)` equals the float result at every
format's limits). Its own story: RULING NEEDED / brief recommendation STRTO-FENV-FLAGS.

## Done in -2
- gate3 red cause: clippy in math_kernel_no_invalid.rs (drop of Copy, approx FRAC_1_SQRT_2): closures return through black_box.
- static red: gamma.rs cast sat 6 lines below its RANGE: comment (window 5): cast moved into the clamp line.
- float-casts scope extended to crates/core/src/num/parse.rs (SCOPE list in xtask/src/float_casts.rs).
- D-10.95 written. Plants file: handoff/LIBM-INVALID-CONVERT-SCAN-plants.txt (4 plants: seed as-u64 x2, gamma and parse RANGE
  comments dropped).
- Jobs on keke (role n1): libm-invalid-convert-scan-2-gate (records check + affected --base origin/next),
  libm-invalid-convert-scan-2-plants. lint1..lint5 were development runs (lint5 green: clippy, static, flags test).

## Not done (next steps, in order)
1. Disassembly: `objdump -d` libm.so.6, list cvttsd2si/cvttss2si per function; confirm lgammaf and fast_bessel_n.
2. 10^7 accuracy jobs for asinl/acosl/atan2l/atanl/lgammaf; liella::hot_text_products on the merged head.
3. Counts before/after (base products: job libm-invalid-convert-scan-so-base on kanon) and booked Ivy Bridge cycles; say
   whether the ~240 fenced cycles shared by x87 functions came from this.
