# LIBM-INVALID-CONVERT-SCAN: handoff (INCOMPLETE, budget spent)

State: branch task/libm-invalid-convert-scan (from origin/next bb9130ed9), head = the last commit on the branch.
Decision id reserved: D-10.95 (record NOT written yet: `cargo xtask records new decision D-10.95` on a build host).

## Scan (step 1: done by type, not by text)
Inventory: `cargo clippy -p liella-math -p liella-boundary --lib -- -A clippy::all --force-warn clippy::cast_possible_truncation
--force-warn clippy::cast_sign_loss` (force-warn overrides the module-level allows that hide most casts). Result: 17 float-to-integer
casts in liella-math, none in liella-boundary (2 more in crates/core/src/num/parse.rs, out of scope, not examined).

| site | function | operand range | raises? |
|---|---|---|---|
| wide_int/arc.rs root_seed `(r*2^37) as u64` | asinl, acosl (root) | [2^63, 2^64] on EVERY call | yes, every call (the #4614 stall) |
| wide_int/arc.rs sqrt `vd as u64` | asinl, acosl | (2^63, 2^64] every call | yes, every call |
| wide_int/arc.rs div `(2^115/dh') as u64` | atan2l, asinl, acosl (quotient) | (2^62, 2^63] | yes when the divisor's top 53 bits are 2^52 (power-of-two divisors) |
| wide_int/arc.rs soft_sqrt `v as u64` | crate-internal root | [2^52, 2^54) | no |
| wide_int/arc.rs atan_le_in `as usize` | atan family | [0, 256] | no |
| fast32/gamma.rs eval_lgamma `x as i32` | lgammaf sign | any f32 if the compiler converts before `x < 0` | possible (not confirmed on disassembly) |
| dd.rs to_i32, fast32/bessel.rs x2, fast_bessel_n.rs x2, mp.rs x2, special.rs x4 | various | proved small by guards above each site | no |

Disassembly confirmation (cvttsd2si sites per function) and per-call flag counts: NOT DONE (budget).

## Changes (steps 2, 3)
- arc.rs: `big_u64(p)` reads `p as u64` from the double's bits for p >= 2^52 (same saturating result at 2^64; no conversion
  instruction, so nothing raises). Used at root_seed, sqrt, div, soft_sqrt.
- gamma.rs: x clamped to [-2^24, 0] with max/min (NaN -> -2^24) before the parity conversion; parity unchanged (floats <= -2^24 are even).
- RANGE: comments on the 12 in-range sites.
- New static check `float-casts` (xtask/src/float_casts.rs; in `cargo xtask static` and as `cargo xtask float-casts`): runs the clippy
  inventory above in target/float-casts and fails on a float-to-int cast with no `RANGE:` comment (its line, 5 above, 1 below) or `.clamp(`.
- Test crates/boundary/tests/math_kernel_no_invalid.rs: calls wide::{asin,acos,atan,atan2}::<F80> directly between feclearexcept and
  fetestexcept (the envelope clears stray flags, so a C-level fenv test cannot see them).
- Plants: handoff/LIBM-INVALID-CONVERT-SCAN-plants.txt (root seed back to `as u64`; flags test and static check).

## Job history
- gate2 / plants2 (rc 1 / 2) ran on the previous commit: clippy manual_clamp on gamma.rs, and the test's own f80() helper underflowed
  (u128 `- 1023` before `+ 16383`). Both fixed in the last commit; gate3 / plants3 started on it, results NOT read by me.

## Next step
1. Read `~/logs/libm-invalid-convert-scan-gate3.log` and `-plants3.log` on kanon (role kn1); fix what is red.
2. Disassemble the products (`objdump -d` of libm.so.6) and list cvttsd2si/cvttss2si per function; confirm lgammaf and fast_bessel_n:194.
3. Write D-10.95 (records new decision), run the 10^7 accuracy jobs for asinl/acosl/atan2l/atanl/lgammaf, liella::hot_text_products,
   counts before/after for the same functions, and booked Ivy Bridge cycles (say whether the ~240 fenced cycles shared by x87 functions
   came from this). The base products are built (job libm-invalid-convert-scan-so-base on origin/next).
