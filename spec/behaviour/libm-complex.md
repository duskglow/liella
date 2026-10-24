# The complex functions of `float` and `double` (spec §10.5)

**Clauses:** `s10_5_c_0`, `s10_0_f_0`, `s10_0_g_0`.

Black-box observations of glibc 2.39 (Ubuntu 24.04, x86_64), made for Track M milestone 4a
(2026-10-05). Probe: `crates/math/tests/oracle/complex_diff.c`, built with
`cc -std=gnu17 -O1 -fno-builtin complex_diff.c -lm`; recorded output:
`libm-complex.glibc.txt.gz` (the grid part: `cexp`, `clog`, `clog10`, `csqrt`, `csin`, `ccos`,
`ctan`, `csinh`, `ccosh`, `ctanh`, `casin`, `cacos`, `catan`, `casinh`, `cacosh`, `catanh`,
`cproj`, `conj`, `cabs`, `carg`, `creal`, `cimag` in `float` and `double` on every pair of 27
components (zeros, subnormals, ±1, values next to π/2 and π, the exponential's overflow and
underflow thresholds, huge values, ±∞, quiet, negative, signalling and payload NaNs), and
`cpow` on 16 bases and 14 exponents, in every rounding direction; each line with both parts'
bits, the flags raised and `errno`). The probe's random section (2,000 operands per function
and format, round-to-nearest, a fixed generator) is not recorded; it is the measurement of
`docs/libm-accuracy.tsv`. The same program is the differential test
`crates/math/tests/complex_products.rs`, which checks every line where Liella differs against
the project's oracle (`crates/mathtools/src/cplx.rs`).

Nothing here describes or derives from glibc's source code.

## Special values (C Annex G)

glibc follows Annex G's tables, with these choices where Annex G leaves them open:

* NaN results are the positive quiet NaN without payload (`7ff8...`) in most cases; the
  default NaN of an invalid operation (`fff8...`) where an infinity meets a zero or an infinite
  angle (`csin(∞ + i0)` = `fff8... + i0`, `ccosh(0 + i∞)`, `cexp(∞ + i∞)` = `∞ + i fff8...`);
  the operand itself (payload and sign kept, sometimes even a signalling NaN unquieted:
  `ctanh(sNaN + i0)`, `ctan(0 + i·sNaN)`, `casinh(sNaN + i0)`, `cproj`) in the rows where Annex G
  passes a NaN part through (`cexp(+∞ + iNaN)`, `csinh(0 + iNaN)`, `ccosh(0 + iNaN)`, `ccos`,
  `casin(0 + iNaN)`); `cacos` returns the imaginary NaN negated, and `csin(NaN + i0)` the real
  NaN with its sign cleared.
* Invalid is raised for a quiet NaN where Annex G makes it optional (`cexp(x + iNaN)` and
  `cexp(NaN + iy)` for finite nonzero `y`, `csinh(x + iNaN)`, `csin(NaN + iy)`, `ctan(∞ + iNaN)`,
  `ctanh(NaN + i∞)`), and for a signalling NaN except where the result does not depend on it
  (`catan(±∞ + i·sNaN)`, `cproj(±∞ + i·sNaN)`).
* The signs Annex G leaves unspecified: `cexp(−∞ + i∞)` = `+0 + i0` (the sign of the
  imaginary part follows y), `cexp(+∞ + i∞)` = `+∞ + i fff8...`, `csinh(0 + i∞)` = `±0 + iNaN`
  (the sign of x), `ccosh(0 + i∞)` = `NaN + i0`, `ctanh(+∞ + iy)` = `1 + i0·sin 2y`, the
  zero parts of finite results as the products of the parts' signs.
* `cpow` with an infinite, NaN or zero operand is `cexp(w · clog z)` with Annex G's
  multiplication (G.5.1): `cpow(0, 0)` = NaN + iNaN with invalid and division by zero,
  `cpow(z, 0)` = 1 + i0 for finite nonzero z, `cpow(NaN + i0, 0)` = NaN + iNaN.

## Flags, errno and rounding

* `errno`: set only incidentally — `ERANGE` for some underflows of `cexp`, `cpow`, the poles
  of `catan`/`catanh`, and as `hypot` and `atan2` set it for `cabs` and `carg`; never for an
  overflow of `cexp`, `csin`, `csinh` and the like, never for `clog(0)`, never `EDOM`.
* Inexact: raised for exact results (`cexp(0)` = 1, `ccosh(0)`, infinite parts of
  `cexp(+∞ + iy)` and `csinh`), not raised for the inexact constants of some special values
  (`clog(−∞ + iy)` = `∞ + iπ`, `cacos(0 + i∞)` = `π/2 − i∞`).
* Rounding: no function is correctly rounded throughout. On the random sample in
  round-to-nearest, glibc's results are correctly rounded for 47% (`clog10`) to 96% (`clog`)
  of the operands (11% for `cpow`, whose errors reach 2.7·10^5 ulp), with errors of up to 4.9
  ulp (`catanh`); in the directed modes the constants of the special values and many
  finite results are the round-to-nearest values.

## What Liella does (D-10.17, D-10.18)

Every part correctly rounded in every mode, exact parts exact; the special values,
NaN choices and invalid flags as glibc; inexact by IEEE 754; `errno` by the §10.0 rule. The
differences are in `docs/differences-from-glibc.md`.
