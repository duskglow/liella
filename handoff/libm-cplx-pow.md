# LIBM-CPLX-POW handoff: cpow/cpowf quick tier (design done, no code yet)

State: branch task/libm-cplx-pow, cut from origin/next d1b3a596c; this note is the only change (run 1 hit the
180k context checkpoint while reading: complex_body.rs / complex_fast.rs / trig.rs / fast64.rs are large). Role
shiki:~/w/sh2. Decisions in force: D-10.53 (design), D-10.74, D-10.99, D-10.25, D-10.17. No jobs run.

## Where things are (do not re-read whole files)

* Route: `complex.rs:253` `cpow` = ziv ladder over `complex_body.rs:1764` (special operands -> `cpow_naive`;
  `d = 0` on an axis -> `pow_axis`; `d = 0`, integer `|c| <= 64` -> `pow_int` (EXACT parts, no inexact);
  `d = 0, c = 0.5` -> csqrt; `d = 0`, `|x| = |y|` diagonal and the bases -1, ±i -> exact angle reduction;
  else `pow_general` (always inexact)). Add `cpow_route` in complex.rs (as `cexp_route`, complex.rs:182) and
  make `cpow` try the tier first, as `cexp`/`clog` do (complex.rs:172-206).
* clog internal form: `complex_fast.rs:568` `clog_parts(x, y) -> Option<[(Dd, abs eps); 2]>` = [(ln|z|, er),
  (arg z, ei)]; window `2^-400 <= |x|,|y| < 2^400` (zeros/axes excluded). ARG_EPS = 2^-65 rel (the bottleneck),
  log ~2^-70 rel, near |z| = 1 `2^-63` rel of ln|z|.
* exp of a double-double: `fast64.rs:415` `reduce_pow(z: Dd, kf)` (private: make `pub(crate)`), `kf =
  nearest_int(z.hi * f64::from_bits(t::SCALES[0]))`, then `fast64::exp_kernel(r, to_i32(kf)) -> (V, m, tail)`,
  bound `fast64::exp_eps(V, tail)` = |V|(2^-49|tail| + 2^-70); needs |z.hi| <= 746. Adding `z.lo` into `l`
  costs <= 2^-97 absolute (|z.lo| <= 2^-44, |l| <= 2^-61).
* sin/cos of a double-double Φ (|Φ.hi| < 2^20): `trig::kernel::{reduce, poly, assemble}` + `trig::eps(sh, y)`
  (see complex_fast.rs:100-111 for the call shape). Fold the low part: `(n, r) = reduce(Φ.hi)`,
  `s = two_sum(r.hi, Φ.lo)`, `r' = two_sum(s.hi, s.lo + r.lo)` (two_sum, NOT fast_two_sum: r.hi may cancel
  with Φ.lo). Adds <= 2^-110 to the reduction error (2^-89.9 -> still < A_RED = 2^-89) and |r'| <= π/128 +
  2^-34 < 2^-5.34 (the kernel proof's limit), so `trig::eps` holds unchanged.
* Rounding/scaling helpers to reuse (private in complex_fast.rs; put the new code in a child module
  `#[path = "complex_fast_pow.rs"] pub mod pow;` so `super::` reaches them): `round(p, eps, m, mode)` (near +
  settle + `scale`), `round_root` pattern (decline when |P.lo| <= eps), `PROD` = 2^-99 (Dd::mul 2^-102.6).
* Boundary: `crates/boundary/src/mathret.rs:1219` `cplx2`; add `cplx2_fast` + cold fallback mirroring
  `cplx_fast` (mathret.rs:1191-1216). Table: `crates/boundary/src/posix/s10_math.tsv:196`
  (`cpow fdlq mret::cplx2(...)`): split into `f`, `d` rows with `mret::cplx2_fast({x}, {y}, {E}, <nearest fn>,
  math::complex::cpow)` and `lq` unchanged; regenerate with `cargo xtask math-gen` (never edit the _gen.rs).
* Tests: `complex_fast_tests.rs` has the xorshift `uniform`, regimes, route compare and the ignored 10^7
  sweep pattern (lines 12-171); copy that shape into `complex_fast_pow_tests.rs`.

## Design (method of the brief; error bound proved here, to go in the module comment and a D-NEW record)

`cpow(z, w) = e^E cis Φ`, `E = c·Lr − d·Li`, `Φ = d·Lr + c·Li`, `L = clog_parts(z)`.
1. Ranges (in-place safety: no overflow/underflow in hardware): z in clog's window; `|c|, |d|` each 0 or in
   `[2^-60, 2^20]`; `|ℓr.hi|, |ℓi.hi| >= 2^-60` (else decline; keeps every product and its Dekker error term
   normal); `w = 0` -> decline early (route gives the exact 1). `|Ê.hi| <= 708` for exp, `|Φ̂.hi| < 2^20`.
   **Large-|w| / large-|L| band:** `M_E = |c·ℓr| + |d·ℓi|`, `M_Φ = |d·ℓr| + |c·ℓi|`; above 2^10 decline
   at once (the amplified error, about M·2^-63, cannot decide a 53-bit part): that band keeps the route.
2. Products: `A = ℓr.mul_f64(c)` etc. (2^-104.4 rel each), `Ê = A − B`, `Φ̂ = C + D` by `Dd::add` (Joldes,
   Muller, Popescu, ACM TOMS 44(2) 2017, Alg. 6: 2^-104.4 of the result). So
   `eE = |c|·er + |d|·ei + 2^-100·M_E + 2^-96`, `eΦ = |d|·er + |c|·ei + 2^-100·M_Φ`.
3. Part bound (A = |V.hi|, B = |T.hi|, T = cos or sin Dd, eT = trig::eps): `|part·2^-m − V⊗T| <=
   A·((eE + exp_rel)·B + eΦ + eT)·(1 + 2^-30) + PROD·A·B` with `exp_rel = 2^-49|tail| + 2^-70`, valid while
   eE, eΦ + eT <= 2^-30 (second-order terms and eps's own rounding inside the 2^-30). Real part error scales
   with |result| (relative E error); the angle error eΦ is absolute, as the brief says.
4. **Strict test (proved, no transcendence assumption):** per part decline if `|P.lo| <= eps`, then `round`.
   The interval then holds no double and no midpoint, so the value is correctly rounded AND inexact is right.
   This covers every exact case (d = 0 with dyadic c: (3+4i)^1.5 = 2 + 11i; integer c), zero parts (decline),
   and d != 0, where Gelfond–Schneider makes z^w transcendental but NOT its parts (rationality of Re z^w is
   open), so the cexp-style "never a double" argument is unavailable. Cost: decline rate ~2^-10 per part.
5. cpowf: same parts from the f32 operands (exact in double); f32 strict test, nearest only (directed -> None
   -> route): `v = scale(P.hi, m)`, require `2^-126 <= |v| <= f32::MAX`, `r = v as f32`,
   `δ = (v − r) + lo·2^m` (Sterbenz-exact first step), `h` = half ulp32 of r (h/2 below a power of two when
   δ points to zero), `thr = eps·2^m·(1 + 2^-50) + h·2^-20`; answer r iff `thr < |δ| < h_side − thr`
   (h·2^-20 also forces `v != r`, so the conversion itself raises inexact in place).
6. Optional cheap exact paths (brief: only proved and measured): `d = 0, c = 0.5` -> `csqrt` tier; integer
   c = 2 by exact two_prod parts. Not designed further.

Expected decline rate ~2^-9 per call (atan2's 2^-65 dominates); each decline costs the route (~10^5 instr):
a later middle tier (~2^-100 clog + `wide_dd::exp_dd` + `sin_or_cos`, as D-10.99) would remove that.

## Next step (continuation, fresh context; suggested budget 120 calls / 200k)

1. Write `complex_fast_pow.rs` (parts + `cpow_nearest` + `cpow` + `cpowf_nearest` + `cpow_any`), make
   `reduce_pow` pub(crate), wire complex.rs and the boundary, math-gen.
2. Tests: tier vs `cpow_route` bit-for-bit (values + flags) in all four modes, regimes: typical
   `[-3,3]^2 x [-3,3]^2`, |z| within 2^-20 of 1, x < 0 with |y| < 2^-20 (branch cut), |w| in [2^6, 2^12];
   exact cases decline ((3+4i)^1.5, (1+i)^2.5 diagonal, c = 2); ignored 10^7 sweep; plants (strict test,
   eΦ term, r' fold, range checks) with `msg:` regexes using [0-9]+.
3. Jobs: clippy, static, gate (`affected --run --base origin/next`), plants, sweep, percall (typical + hard
   rows, with and without `--tunables glibc.cpu.hwcaps=-FMA`) vs next and glibc.
