# The elementary functions of `float` and `double` (spec §10.3)

**Clauses:** `s10_3_c_0`, `s10_0_f_0`, `s10_0_g_0`.

Black-box observations of glibc 2.39 (Ubuntu 24.04, x86_64), made for Track M milestone 2
(2026-10-05). Probe: `crates/math/tests/oracle/elem_diff.c`, built with
`cc -std=gnu17 -O1 -fno-builtin elem_diff.c -lm`; recorded output: `libm-elem.glibc.txt.gz`
(20,196 lines: `exp`, `exp2`, `exp10`, `expm1`, `log`, `log2`, `log10`, `log1p`, `sin`, `cos`,
`tan`, `sincos`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`,
`atanh`, `cbrt`, `pow`, `hypot` in `float` and `double`, on zeros, subnormals, ±1, ±∞, quiet and
signalling NaNs, domain edges, the overflow and underflow thresholds, huge trigonometric
arguments and the Annex F tables of `pow` and `atan2`, in every rounding direction, each with
the result's bits, the flags raised and `errno`). The same program is the differential test
`crates/math/tests/elem_products.rs`, which checks every line where Liella differs against the
project's oracle (`crates/mathtools`). Interior accuracy is measured separately
(`cargo xtask mathcmp`, `docs/libm-accuracy.tsv`).

Nothing here describes or derives from glibc's source code.

## Special values

* The Annex F special cases hold in every rounding direction: `pow(x, ±0)` = 1 for every `x`
  but a signalling NaN (which gives a quiet NaN with invalid), `pow(1, y)` = 1 for a quiet NaN
  `y` (a signalling one: a quiet NaN with invalid), `pow(−∞, y)` and `pow(±0, y)` by the parity
  of `y`, `atan2` on signed zeros and infinities by quadrant, `hypot(±∞, NaN)` = +∞ for a quiet
  NaN (a signalling one gives a quiet NaN with invalid), `exp(−∞)` = +0, `log(±0)` = −∞,
  `cosh(±∞)` = +∞, `tanh(±∞)` = ±1, `sin(±∞)` a domain error.
* Domain errors return a NaN with invalid and `EDOM`. Its sign: positive (`7ff8...`, `7fc0...`)
  for `asin`, `acos`, `log10`; negative (the default NaN, `fff8...`) for `log`, `log2`,
  `log1p`, `acosh`, `atanh` (including `atanh(±∞)`), `pow`, `sin`, `cos`, `tan`, `sincos`.
* Poles (`log(±0)`, `log1p(−1)`, `atanh(±1)`, `pow(±0, y<0)`) return ±∞ with division by zero
  and `ERANGE`.
* NaN operands: a quiet NaN propagates without flags, a signalling one is quieted with invalid.
  `pow` and `hypot` return the first NaN operand (`pow(qNaN, sNaN)` is the quiet `x`), `atan2(y, x)`
  returns `x`'s when both are NaNs.

## Flags and errno

* Overflow to ±∞ and underflow to ±0 set `ERANGE` in every function.
* A result rounded to the largest finite value (directed modes) or a nonzero subnormal: `asin`,
  `atan`, `atan2`, `sin`, `tan`, `sincos`, `sinh`, `tanh`, `asinh`, `atanh`, `expm1`, `log1p`,
  `hypot` raise the flags only, and so do `sinh`/`cosh` at the largest finite value. `exp`,
  `exp2`, `exp10`, `pow` (and their `float` forms) set `ERANGE` for some of these results and
  not for others of the same kind (for example `exp` of a subnormal result: 11 of 22 probed
  cases; `pow`: 54 of 67).
* Inexact: raised for most inexact results, but not where the function returns its operand or a
  limit for a tiny or huge argument: `sin`, `tan`, `asin`, `atan`, `sincos` of `|x|` below about
  2^-26 return `x` with no flag (even in directed modes, where the correctly rounded result is
  a neighbour of `x`); `cos(tiny)` = 1, `acos(tiny)` = π/2, `cosh(tiny)` = 1 and `atan2` limits
  likewise. Conversely inexact is raised for some exact results: `cbrt(27)`, `log2(2^k)`,
  `log10(10^k)`, `exp10(k)` for small `k`, `pow` of exact powers (`pow(3, 4)`, `pow(0.25, 0.5)`);
  `exp2(−1074)` and `pow` results that are exact subnormals raise underflow and inexact.
* Rounding: in directed modes several functions return the round-to-nearest result instead
  of the directed one (for example `sin`, `cos`, `tan`, `atan`, `atan2` at about half of all
  arguments, `exp10f(2)` upward = 100 + 1 ulp, `tanh(−huge)` upward = −1), measured in
  `docs/libm-accuracy.tsv`.

## What Liella does (D-10.13)

Every result correctly rounded in every mode, with the inexact, underflow and overflow flags of
that rounding (inexact exactly when the result is inexact; underflow detected after rounding,
as x86 does); invalid, division by zero, NaN signs and payloads as glibc; `errno` by the §10.0
rule (`ERANGE` for an overflow to ±∞ or an underflow to ±0 only). The differences from glibc
are listed in `docs/differences-from-glibc.md`.
