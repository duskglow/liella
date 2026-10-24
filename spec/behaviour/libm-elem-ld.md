# The elementary functions of `long double` (spec §10.3)

**Clauses:** `s10_3_c_0`, `s10_0_f_0`, `s10_0_g_0`.

Black-box observations of glibc 2.39 (Ubuntu 24.04, x86_64), made for Track M milestone 5a
(2026-10-05). Probe: `crates/math/tests/oracle/elem_ld_diff.c`, built with
`cc -std=gnu17 -O1 -fno-builtin elem_ld_diff.c -lm`; recorded output:
`libm-elem-ld.glibc.txt.gz` (25,176 lines: `expl`, `exp2l`, `exp10l`, `expm1l`, `logl`,
`log2l`, `log10l`, `log1pl`, `sinl`, `cosl`, `tanl`, `sincosl`, `asinl`, `acosl`, `atanl`,
`atan2l`, `sinhl`, `coshl`, `tanhl`, `asinhl`, `acoshl`, `atanhl`, `cbrtl`, `powl` on zeros,
the smallest subnormal and normal values, ±1 and its neighbours, ±∞, quiet and signalling NaNs,
domain edges, the overflow and underflow thresholds, huge trigonometric arguments up to the
largest finite value, exact cases and the Annex F tables of `powl` and `atan2l`, plus 150
random operands per function and mode, in every rounding direction, each with the 80-bit
encodings, the flags raised and `errno`). The same program is the differential test
`crates/math/tests/elem_ld_products.rs`, which checks every line where Liella differs against
the project's oracle (`crates/mathtools`). Interior accuracy is measured separately
(`cargo xtask mathcmp FUNC l`, `docs/libm-accuracy.tsv`). A separate probe settled the NaN
operands of `acosl` and the domain NaNs (2026-10-05, the results below).

Nothing here describes or derives from glibc's source code.

## Special values

* The Annex F special cases hold in every rounding direction as for `double`
  (`spec/behaviour/libm-elem.md`).
* Domain errors return a NaN with invalid and `EDOM`. Its sign: positive (`7fffc000...`) for
  `asinl`, `acosl`, `logl`, `log2l`, `log10l` (so `logl` and `log2l` differ from `log` and
  `log2`, whose NaN is the negative default one); negative (`ffffc000...`, the default NaN) for
  `log1pl`, `acoshl`, `atanhl` (including `atanhl(±∞)`), `powl`, `sinl`, `cosl`, `tanl`,
  `sincosl`. `logl(−∞)` and `log2l(−∞)` are positive too.
* NaN operands propagate quieted, sign and payload kept, invalid for a signalling one, except
  `acosl`, which returns the quieted NaN with its sign cleared (`acosl(−NaN(0x1234))` =
  `+NaN(0x1234)`). `powl` returns the first NaN operand, `atan2l(y, x)` `x`'s when both are NaNs.
* Poles return ±∞ with division by zero and `ERANGE`.

## Flags and errno

* Overflow to ±∞ and underflow to ±0 set `ERANGE`, except where a tiny argument takes a
  shortcut (`atanl` and `log1pl` of the smallest subnormals rounded to zero in a directed mode:
  flags only).
* Inexact is not raised where a function returns its operand or 1 for a tiny argument (`coshl`,
  `expm1l` and others) and is raised for exact results (`powl(1, 0.5)`, `powl` of exact powers,
  `cbrtl(27)`, `log2l(2^k)`, `log10l(10^k)`, `exp10l(k)`) and together with invalid or division
  by zero in `powl` and `log1pl`.
* Tiny arguments: `sinl`, `tanl`, `asinl`, `sinhl`, `asinhl`, `atanhl` of the smallest
  subnormal return the operand in every direction (the correctly rounded directed result is
  often a neighbour), `cosl` and `coshl` return 1; `tanhl` upward returns the next value above
  the operand (tanh x < x).

## What Liella does (D-10.19)

Every result correctly rounded in every mode, with the inexact, underflow and overflow flags of
that rounding; invalid, division by zero, NaN signs and payloads as glibc (including `acosl`'s
and the positive domain NaN of `logl` and `log2l`); `errno` by the §10.0 rule. The differences
from glibc are listed in `docs/differences-from-glibc.md`.
