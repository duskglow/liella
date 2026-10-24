# The special functions of `float` and `double` (spec §10.3)

**Clauses:** `s10_3_c_0`, `s10_0_f_0`, `s10_0_g_0`.

Black-box observations of glibc 2.39 (Ubuntu 24.04, x86_64), made for Track M milestone 3
(2026-10-05). Probe: `crates/math/tests/oracle/special_diff.c`, built with
`cc -std=gnu17 -O1 -fno-builtin special_diff.c -lm`; recorded output:
`libm-special.glibc.txt.gz` (`erf`, `erfc`, `tgamma`, `lgamma`, `gamma`, `lgamma_r`, `j0`, `j1`,
`y0`, `y1`, `jn`, `yn` in `float` and `double`, on zeros, subnormals, ±∞, quiet and signalling
NaNs, poles, the overflow and underflow thresholds, zeros of the Bessel functions and of
`lgamma`, huge arguments, orders −3, −1, 0, 1, 2, 5, 30 and 200, in every rounding direction;
each line with the result's bits, the sign of Γ(x) stored for the gamma functions, the flags
raised and `errno`). The same program is the differential test
`crates/math/tests/special_products.rs`, which checks every line where Liella differs against
the project's oracle (`crates/mathtools`).

Nothing here describes or derives from glibc's source code.

## Special values

* `erf(±0)` = ±0, `erf(±∞)` = ±1; `erfc(±0)` = 1, `erfc(+∞)` = +0, `erfc(−∞)` = 2. No flags.
* `tgamma(±0)` = ±∞ (division by zero, `ERANGE`); `tgamma` of a negative integer or −∞: a
  positive NaN (`7ff8...`), invalid, `EDOM`; `tgamma(+∞)` = +∞.
* `lgamma` (also `gamma`, `lgamma_r`): +∞ with division by zero and `ERANGE` at 0 and the
  negative integers; +∞ without flags at ±∞. The sign stored: −1 for `−0`, 1 for `+0`, the
  negative integers, ±∞ and NaNs; otherwise the sign of Γ(x) (negative on (−1, 0), (−3, −2), ...).
  `lgamma` and `gamma` store it in `signgam`, `lgamma_r` only through its pointer.
* `j0(±0)` = 1, `j0(±∞)` = +0. `jn(n, ±0)` and `jn(n, ±∞)` for `n ≠ 0` are zeros with the
  sign of the symmetries `J_{−n} = (−1)^n J_n` and `J_n(−x) = (−1)^n J_n(x)` (`j1(−∞)` = −0,
  `jn(−3, +0)` = −0).
* `y0`, `y1`, `yn` at ±0: −∞ with division by zero and `ERANGE` (+∞ for odd negative orders);
  at a negative argument or −∞: a positive NaN, invalid, `EDOM`; at +∞: +0, but −0 for
  `yn(−1, +∞)`.

## Flags and errno

* As for the elementary functions (`libm-elem.md`): `ERANGE` with every overflow to ±∞ and
  underflow to ±0, and also for some subnormal results (`erfc`, `jn`, `yn` and their `float`
  forms) and not for others of the same kind.
* Inexact: raised for exact results of `tgamma` (`tgamma(2)` = 1, `tgamma(5)` = 24, ...); not
  raised where a tiny argument takes a shortcut (`y1` and `yn` near 0, `jn(2, 2^-60)` =
  2^-123).
* Rounding: many results are not correctly rounded, the Bessel functions above all (more
  than half of the probed `j0` and `jn` lines and a third of the `y0` and `yn` lines differ from
  the correctly rounded value, most at large arguments) and in the
  directed modes; measured in `docs/libm-accuracy.tsv`.

## What Liella does (D-10.15, D-10.16)

Every result correctly rounded in every mode (384-bit evaluation, one rounding) with the
inexact, underflow and overflow flags of that rounding; invalid, division by zero, NaN signs,
the sign of Γ(x) stored and the signs of zero limits as glibc; `errno` by the §10.0 rule. The
differences are in `docs/differences-from-glibc.md`.
