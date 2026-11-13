# LIBM-RETHINK-HYPOT-2: f64 hypot, one correctly rounded path for every finite input (implement D-10.104)

Kind: code. Model: opus. Effort: max. Stacks-on: next (or task/libm-rethink-hypot, which adds only
`validation/percall/hypot.c` and this note). Decision id: D-10.104 (reserved for LIBM-RETHINK-HYPOT; reuse it).

## Evidence (LIBM-RETHINK-HYPOT, Zen 1 build host, `wl_percall.sh --spec validation/percall/hypot.c`, instructions per call incl. the loop)

| case | glibc | Liella (c32adc349) | ratio |
|---|---:|---:|---:|
| hypot_typ (x, y uniform in [-100, 100]) | 65.9 | 110.0 | 1.67 |
| hypot_near (same binade +-2, every magnitude) | 68.8 | 2306.9 | 33.5 |
| hypot_bits (uniform finite bit patterns) | 41.5 | 3703.4 | 89.2 |

`wl_trace.sh` (hypot_bits, call 11): 3804 instructions = `pow::hypot::<f64>` 3320 + `arith::sum` 175 + `float::round`
162 + `nan_operands_by` 65 + `elem_fast_cold` 34 + entry 22. The fast tier `invtrig::hypot_nearest` refuses and the
256-bit exact path (D-10.13) runs. Why it refuses: it accepts only `2^-450 <= |x|, |y| < 2^450` with exponents within
60 of each other (about 19% of bit patterns), round-to-nearest only (every directed mode takes the 256-bit path), and
also refuses every root within `2^-98` of a `double` (all exact roots such as `hypot(3, 4)`, all huge exponent gaps).

## Design (first principles; both alternatives estimated in the handoff)

All in the scaled domain, no window, no 256-bit path for any finite input:

1. `a = max(|x|, |y|)`, `b = min` by bits. NaN/inf -> `pow::hypot` (unchanged). `b == 0` -> `a` exactly (no flags).
2. **Gap exit:** with biased exponent fields `Ea - Eb >= 27` the root lies strictly inside `(a, a + ulp(a)/2)`
   (proof: `r - a < b^2/(2a) < 2^(2eb+1-ea) <= 2^(ea-53)`; holds for subnormal `b` with `Eb = 0`). Nearest/down/zero:
   `a`; up: `nextup(a)`; always inexact. Fast entry: return `a * (1 + 2^-60)` (rounds to `a`, raises inexact only;
   `a` is normal here). This is 97% of bit patterns: target ~15 instructions.
3. **Scale** both by `2^(1023-Ea)` (constant from bits; clamp the biased value to >= 1 for `Ea = 2046`): exact, `a`
   lands in [1, 4), `b` in [2^-77, 4) (the field gap is < 27; a subnormal `b` adds at most 52). Both subnormal
   (`Ea == 0`): the result in units of 2^-1074 is the integer `round(sqrt(A^2 + B^2))` with `N = A^2 + B^2` in u128
   (`A, B < 2^52`): `q` from `sqrtsd(N as f64)` fixed by +-1 with exact `q^2` compares; nearest: `N > q^2 + q` -> q+1
   (no ties possible); exact iff `N == q^2`; `f64::from_bits(q)` is `q * 2^-1074` for every `q < 2^53`.
4. **Root:** the existing `sqrt_dd(fast_two_sum(s.hi, s.lo + (pa.lo + pb.lo)), sqrt)` on `two_sum(a^2, b^2)`
   (Borges, "Algorithm 1014: An improved algorithm for hypot(x,y)", ACM TOMS 47(1), 2021: one FMA-residual correction
   step, here on the double-double sum; relative error < 2^-104 as already proved for `EPS_HYPOT`).
   (yh, yl) normalized by `fast_two_sum`; `eta = 2^-98 * yh`; `h` = half ulp of yh (`h/2` when yh is a power of two
   and `yl < 0`).
5. **Decide** (Ziv, ACM TOMS 17(3) 1991, rounding test): nearest: `|yl| < h - eta` -> `yh`; directed: `|yl| > eta`
   -> `yh` or its neighbour by the sign of `yl`. Nearest with `|yl| <= eta`: result `yh`, exactness unknown.
6. **Hard cases exactly, in integers (new, no multi-precision):** the sign of `a^2 + b^2 - m^2` for `m = M * 2^e`
   (`M < 2^56`: the midpoint `2Y+1` / `2Y-1` / `4Y-1` at a power of two, or `Y` itself) as a WRAPPING u128 sum:
   `D = (A^2 << 2(ea-base)) + (B^2 << 2(eb-base)) - (M^2 << 2(e-base))`, `base = min(ea, eb, e)`, each shift by
   `checked_shl(..).unwrap_or(0)` (a shift >= 128 is 0 mod 2^128, which `wrapping_shl` gets wrong), read as i128. It is
   exact because the true `|D| < 2^127` whenever the test reaches it: near a midpoint the true gap is <= 27 (else step
   2 applies), so `|D| <= 2^-88 * M^2 * 2^58 < 2^82`; near `yh` with a large true gap, `yh = a` and `D = B^2`.
   Equal at a midpoint = tie (even); equal at `yh` = exact (no inexact flag).
7. **Unscale** by `2^(Ea-1023)` (exact for a normal result). Overflow: nearest/up inf, down/zero DBL_MAX, overflow +
   inexact + ERANGE. No underflow outside step 3's subnormal case (result >= a).

Wiring: fast entry (`Hypot for f64`, `elem_fast`): returns `Some` for every finite input except an exact root whose
products were inexact (hardware may have raised a spurious inexact: refuse, the cold path settles flags), an overflow
(errno) and the both-subnormal case (underflow flag). The SLOW closure becomes a new `hot::hypot_rounded(x, y, mode)`
-> `Option<(f64, Status)>` (same kernel, any direction, explicit flags/errno; `None` only for NaN/inf -> `pow::hypot`),
so directed modes and every refusal cost tens of instructions, not 3.7k. Put the kernel in `invtrig.rs`'s `kernels!`
macro (both builds: Dekker and `fused`, bit-identical), the integer compare as a plain fn. Inexact on the fast path
is the hardware's: an inexact root always has an inexact intermediate (sl != 0 needs an inexact square or sum, else
`sqrtsd` is inexact).

Estimated cost (fast entry): gap exit ~15-20, main path ~40-45 (2 scale mul, 2 two_prod, two_sum, sqrt, residual, 1
div, test) vs glibc 26 / ~45-60 (calibrated). Alternatives rejected: (B) all-integer 160-bit isqrt every call
(~50 instructions, branchy, no gain); (C) division-free residual threshold compare (needs +-1/+-2 ulp branches
taken ~30% of calls: mispredictions).

## Done means

hypot_bits and hypot_near within 2x glibc (instructions), hypot_typ not worse (110.0), directed modes measured;
f64 hypot sweeps and hard points green (add tests: Pythagorean triples with 53-bit legs incl. exact midpoints
`M` odd 54-bit, every direction, gap 26/27/28 boundary, DBL_MAX overflow per direction, both subnormal, `b`
subnormal with `a` normal); plants (`cargo xtask plants`): the gap threshold 27 -> 26, the midpoint `2Y+1` -> `2Y`,
`checked_shl` -> `wrapping_shl`, eta halved/doubled, the tie-to-even pick inverted; `msg:` regexes with [0-9]+.
D-10.104 record (`cargo xtask records new decision D-10.104`) citing Borges 2021 and Ziv 1991. hypotf: not in the
probe's bad list; its fast path refuses only subnormal/overflowing roots and the midpoint pattern (mathret.rs,
`hypotf_fast`); check its subnormal-result band in the same story only if cheap.
