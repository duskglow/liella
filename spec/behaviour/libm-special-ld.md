# The special functions of `long double` (spec §10.3)

**Clauses:** `s10_3_c_0`, `s10_0_f_0`, `s10_0_g_0`.

Black-box observations of glibc 2.39 (Ubuntu 24.04, x86_64), made for Track M milestone 5b
(2026-10-05). Probe: `crates/math/tests/oracle/special_ld_diff.c`, built with
`cc -std=gnu17 -O1 -fno-builtin special_ld_diff.c -lm`; recorded output:
`libm-special-ld.glibc.txt.gz` (10,068 lines: `erfl`, `erfcl`, `tgammal`, `lgammal`, `gammal`,
`lgammal_r`, `j0l`, `j1l`, `y0l`, `y1l`, `jnl`, `ynl` on zeros, the smallest subnormal and
normal values, ±1 and its neighbours, ±∞, quiet and signalling NaNs, poles, the overflow and
underflow thresholds of the 80-bit format (`erfcl` near 106.8, `tgammal` near 1755.5 and
−1766.5), zeros of `lgammal` and of the Bessel functions, huge arguments up to the largest
finite value, orders −3, −1, 0, 1, 2, 5, 30 and 200, plus 40 random operands per function and
mode (10 per order), in every rounding direction; each line with the 80-bit encodings, the sign
of Γ(x) stored for the gamma functions, the flags raised and `errno`). Left out on purpose:
`tgammal(±2^-k)` for k = 16382 to 16384 and the general orders at operands below 2^-1000,
where the oracle needs over 16,000 bits per line (the same code paths are checked at 2^-300
and through `j0l`, `j1l`, `y0l`, `y1l`). The same program is the differential test
`crates/math/tests/special_ld_products.rs`, which checks every line where Liella differs
against the project's oracle (`crates/mathtools`).

Nothing here describes or derives from glibc's source code.

## Special values

The conventions of the `float` and `double` forms (`libm-special.md`) hold unchanged:

* `erfl(±0)` = ±0, `erfl(±∞)` = ±1; `erfcl(±0)` = 1, `erfcl(+∞)` = +0, `erfcl(−∞)` = 2.
* `tgammal(±0)` = ±∞ (division by zero, `ERANGE`); at a negative integer or −∞ a positive NaN
  (`7fffc000...`), invalid, `EDOM`.
* `lgammal`, `gammal`, `lgammal_r`: +∞ with division by zero and `ERANGE` at 0 and the negative
  integers; the sign stored −1 for −0, 1 at the poles, ±∞ and NaNs, otherwise the sign of Γ(x).
* The Bessel functions: the zero limits carry the sign of the symmetries (`jnl(−3, +∞)` = −0,
  `jnl(−3, −∞)` = +0), `ynl(−1, +∞)` = −0; `y0l`, `y1l`, `ynl` of a negative argument give the
  positive NaN, invalid, `EDOM`.

One difference from the `double` form: `lgammal(2)` rounding downward is −0 (`lgamma(2)` is +0
in every mode, and `lgammal(1)` too).

## Flags and errno

* `ERANGE` for some subnormal results (`erfcl`, `jnl`) and for results rounded to the largest
  finite value (`ynl` in the directed modes), as for `double`.
* Inexact: raised for exact results of `tgammal` (`tgammal(2)` = 1, `tgammal(5)` = 24); not
  raised where a tiny argument takes a shortcut (`y1l` and `ynl` near 0; `jnl(2, 2^-190)` is
  2^-383 exactly in every mode, without flags).
* `lgammal_r` at very large negative integers (−LDBL_MAX and others above 2^64): overflow and
  inexact as well as division by zero.
* Rounding: of the 4,811 lines where Liella differs, 4,673 are results glibc does not round
  correctly (the directed modes above all); measured in `docs/libm-accuracy.tsv`.

## What Liella does (D-10.20)

Every result correctly rounded in every mode with the inexact, underflow and overflow flags of
that rounding; invalid, division by zero, NaN signs, the sign of Γ(x) stored and the signs of
zero limits as glibc; `lgammal(1)` = `lgammal(2)` = +0 in every mode (C F.10.5.3); `errno` by
the §10.0 rule. The differences are in `docs/differences-from-glibc.md`.
