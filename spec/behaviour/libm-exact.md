# The exact math functions and the floating-point environment (spec §10.1, §10.2, §10.4, §10.6)

**Clauses:** `s10_1_c_1`, `s10_2_t_r16`, `s10_4_t_r1`, `s10_6_c_0`, `s10_0_f_0`.

Black-box observations of glibc 2.39 (Ubuntu 24.04, x86_64), made for Track M milestone 1
(2026-10-04). Probe: `crates/math/tests/oracle/exact_diff.c`, built with
`cc -std=gnu17 -O1 -fno-builtin exact_diff.c -lm`; recorded output:
`libm-exact.glibc.txt.gz` (about 114 000 lines: every classification, rounding, remainder,
min/max, payload, exponent, `fma`, `sqrt` and narrowing function in `float`, `double`,
`long double` and `_Float128`, on edge operands, in every rounding direction where the result
depends on it, each with the result's bits, the flags raised and `errno`). The same program is
the differential test `crates/math/tests/exact_products.rs`. Smaller probes settled
`remquo`'s quotient width, `fma`'s NaN order and where `feraiseexcept` sets flags.

Nothing here describes or derives from glibc's source code.

## The environment

* `math_errhandling` is 3 (`MATH_ERRNO | MATH_ERREXCEPT`).
* `fesetround` returns 0 for the four modes and 1 for anything else, which changes nothing.
* `feraiseexcept(FE_OVERFLOW)` raises only overflow (no inexact); each flag alone raises only
  itself. Invalid and division by zero are set in MXCSR; overflow, underflow and inexact in the
  x87 status word (seen through `fegetenv`). `fesetexcept` and `fesetexceptflag` set MXCSR.
  `fetestexcept` sees flags of either unit (an x87 `0/0` shows as invalid); `feclearexcept`
  clears both.
* `fesetexceptflag(&f, e)` sets each flag of `e` to its state in `f`: set ones are set, others
  cleared. Unknown bits of `excepts` (0x40, 0x1000) are ignored.
* `feenableexcept`/`fedisableexcept` return the previously enabled set; `fegetexcept` the
  enabled set. `FE_NOMASK_ENV` enables all five; `FE_DFL_ENV` is control word 0x37f, MXCSR
  0x1f80, no flags.
* `feholdexcept` stores the environment, then clears the flags and masks every trap;
  `feupdateenv` installs a saved environment and raises the flags that were set.
* `fesetmode(FE_DFL_MODE)` restores round-to-nearest and keeps the flags.
* With a trap enabled, `feraiseexcept` delivers `SIGFPE` for each of the five exceptions;
  `fesetexcept` delivers nothing.

## errno and flags

* Domain errors set `EDOM` with invalid: `sqrt(<0)`, `fmod(x, 0)`, `fmod(∞, y)`,
  `remainder(x, 0)`, `ilogb(0)`/`ilogb(NaN)` (`INT_MIN`), `ilogb(±∞)` (`INT_MAX`), `iseqsig` on any
  NaN, `fromfp` family out of range, `scalb` with a non-integral exponent (plus inexact).
* Flags only, no `errno`: `lrint`/`llrint`/`lround`/`llround` out of range (invalid), `fma` and
  the narrowing `*fma` (any case), `remquo(x, 0)` (invalid; the quotient is not stored),
  `logb(0)` (division by zero, result −∞), `nextafter`/`nexttoward` from ±0 (underflow and inexact).
* Range errors set `ERANGE` only for an overflow to ±∞ or an underflow to ±0. A result rounded
  to the largest finite value (a directed mode) or to a nonzero subnormal raises the flags only:
  `ldexp`, `scalbn`, `scalbln`, `scalb`, `fdim`, the narrowing operations. `nextafter` and
  `nexttoward` set `ERANGE` for any subnormal or zero result reached from a nonzero operand.
* Underflow is detected after rounding (x86): `ldexp(1.5, -1074)` is the subnormal 2·2^-1074 with
  underflow and inexact.
* Classification functions raise nothing, even for signalling NaNs.

## Results

* The NaN an invalid operation produces has the sign bit set (`fff8...` for `double`) in every
  format, including `_Float128`.
* Which NaN operand's payload a two-operand function returns: `float`/`double`: the first,
  except `fma` (y, then x, then z, signalling or not), `nextafter` (y first) and `double`
  `remainder`/`drem` (y first; `float`: x). `long double` and `_Float128`: a quiet NaN over a
  signalling one, then the larger significand (ties: the first). `fmax`/`fmin`/`fmaxmag`/
  `fminmag` in `_Float128`, and `fmaxmag`/`fminmag` in `long double`: a quiet NaN over a
  signalling one, then the first. `fma` in those two formats: the product's NaN, then that
  against the addend.
* `fma(∞, 0, qNaN)`: the NaN and no invalid for `float`/`double`; invalid for `long double`
  (result: the addend's NaN) and `_Float128` (result: the default NaN).
* `fmin`/`fmax` of `+0` and `−0` return `y` for `float`/`double`, `x` for `_Float128`, and for
  `long double`: `fmin` returns `x`, `fmax` `y`. The C23 `fminimum`/`fmaximum` family orders
  −0 below +0. A signalling NaN operand of `fmax`/`fmin` gives a quiet NaN with invalid; the
  `_num` forms return the number (with invalid).
* `lrint`/`lround` out of range return `LONG_MIN` for `float`, `double`, `long double`; for
  `_Float128`, `LONG_MAX` for positive (and +NaN) and `LONG_MIN` for negative.
* `fromfp` family (2.39: returns `intmax_t`/`uintmax_t`): out of range gives the end of the
  range nearest the operand's sign (NaN by its sign bit), width 0 gives 0, widths above 64 act
  as 64; all with invalid and `EDOM`.
* `remquo` stores the low three bits of the quotient with its sign.
* `significand(0)`: `float`/`double` invalid and `EDOM`; `long double` division by zero, no
  `errno`. `significandf(±∞)` also raises inexact; `significandl(±∞)` raises nothing.
* `nan("tag")`: the tag read by `strtoull` rules (base prefix, saturating), masked to the
  payload; a tag that is not entirely `[A-Za-z0-9_]` digits of one number gives payload 0.
* `getpayload` of a non-NaN is −1; `setpayload` of a non-integral, negative or too large payload
  stores +0 and returns 1; `setpayloadsig(0)` likewise.
* x87 encodings: a pseudo-denormal classifies as normal; unnormals, pseudo-infinities and
  pseudo-NaNs classify as signalling NaNs (`isnan` true, `fpclassify` `FP_NAN`) but `finitel`
  says unnormals are finite; arithmetic on them gives the default NaN with invalid.
  `canonicalizel` returns 1 without storing for all of these, pseudo-denormals included.

## Return values of the predicates

`__signbit*` return the sign bit where the implementation finds it (8 for `float` and
`_Float128`, 128 for `double`, 512 for `long double`); `__isnanl` returns 65535. The other
predicates return 0 or 1 (`__isinf*` ±1).
