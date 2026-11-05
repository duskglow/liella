## 20. The 384-bit mp core and the double-double rounder (LIBM-MP-CORE-SPEED, 2026-11-05, counted)

Zen 1 host (AMD EPYC 7551), `cargo xtask bench --compare --counters --runs 3 cbench_libm`, instructions per call
(Liella). Before: base b6353c7b. After: the LIBM-MP-CORE-SPEED code (5d68e879, 6e4aa8cc). Results are bit-identical
(value and flags) in every mode; see the tests named below.

**What was tried, and what it gave.**

| Change | Where the time was (LIBM-SLOW-PATH-CAUSE) | Result |
|---|---|---|
| `Mp::div_u64`: 32-bit half-limb native division for `k < 2^32`, else Moller-Granlund 2-by-1 with one reciprocal per call | erf 48% and expl 30% in `__udivti3` | kept; exact floor quotient as before (`div_u64_is_the_exact_floor_quotient`) |
| `pack`/`shr`: one shift per limb, a copy when already normalised | erf 18%, expl 28%, atanl 32% | kept; digest of every 384-bit kernel unchanged (`narrow_kernels_unchanged`) |
| `ln10` and lgamma's `ln π` as generated constants (`MP_LN10`, `MP_LN_PI`) in place of the per-call `ln_atanh` series | log10l ran two log series per call | kept; both constants equal the 384-bit truncation (also the nearest value) of an independent 250-digit reference (`ln10_and_ln_pi_constants_are_correctly_rounded`); the digest moved only by the old series' low bits |
| `elem::direct`: round a normalised double-double to `double`/`float` in each mode, the 256-bit route kept for zero, non-finite, near-limit values and exact ties | 9-20% of every double-double path | kept (`direct_rounding_matches_the_wide_route`, value and Status bit for bit) |

**Counts (instructions per call, Liella before -> after, glibc).** 111 rows on the mp and double-double paths (erf*,
lgamma*, tgamma*, j*/y*, `*l`, `*f128`, complex) got 15% to 75% cheaper (median 19%); none of the 169 rows got slower
than 0.5%.

| Row | Before | After | Change | glibc |
|---|---:|---:|---:|---:|
| `exp10l` | 136496.6 | 34157.6 | -75.0% | 82.5 |
| `log10l` | 191335.4 | 77885.4 | -59.3% | 44.4 |
| `lgamma` | 585889.9 | 428290.7 | -26.9% | 370.7 |
| `tgamma` | 624494.1 | 460414.5 | -26.3% | 610.5 |
| `atanl` | 201768.6 | 159346.7 | -21.0% | 21.8 |
| `cpow` | 441461.8 | 354799.2 | -19.6% | 685.1 |
| `erf` | 212074.5 | 174248.0 | -17.8% | 153.9 |
| `expl` | 40226.3 | 33322.6 | -17.2% | 82.3 |
| `j0` | 142453.4 | 121243.5 | -14.9% | 415.1 |
| `cbrt` | 1069.8 | 636.8 | -40.5% | 148.4 |

**Not reached.** The 2-3x the brief expected on the mp rows: the overheads it named are gone, and what is left is
the arithmetic of a 384-bit evaluation on every call. Those rows stay 100x to 7000x over glibc, which is the case for
the fast Ziv tiers (LIBM-F64-FAST-KERNELS, -INVTRIG-HYPOT-TIER, -SPECIAL-FAST-TIER, -WIDE-DD-TIER), not for more
work on the core. One open item in the core: `sqr` still runs the full 36-product `mul` (a 21-product square
would be bit-identical, since both products are exact); not measured.
