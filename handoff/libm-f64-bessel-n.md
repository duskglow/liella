# LIBM-F64-BESSEL-N handoff (run 1, checkpoint on context: INCOMPLETE)

State: branch `task/libm-f64-bessel-n` (cut from origin/next 0d0d82e5d), head: see `git log -1`; role keke:~/w/n1.
All commits are WIP. NOTHING IS VERIFIED: no test of the new code has run.

## What exists
* `crates/math/src/fast_bessel_n.rs` (submodule `fast_bessel::n` via `#[path]`, lib.rs untouched): quick tier
  for `jn`/`yn`/`jnf`/`ynf`, orders |n| >= 2; |n| <= 1 route to the existing j0/j1/y0/y1 fast paths.
  Regimes (x > 0): yn forward from Y0, Y1; jn series when x^2 <= n+1; jn forward when n <= x; jn otherwise
  J_floor(x) forward times v_n/v_floor(x) from Miller's backward recurrence. Rounding tests `near64` (fast64::settle
  with exponent, underflow shortcut) and `near32` (fast32::round, made pub(crate)).
* `special.rs`: `jn`/`yn` try the quick tier (stats names jn, yn, jnf, ynf), then `jn_accurate`/`yn_accurate`
  (the old bodies); `j0_accurate`.. call the `_accurate` forms.
* Clippy: runs 1-3 failed (ceil not in core; nan-comparison style; docs), fixed; `libm-f64-bessel-n-clippy4`
  (on 8430e1bae) was started and not read.

## Error bounds (to be written up fully; summary in the module doc)
* Forward: error δ injected at f_{k+1} reaches f_m as δ(πx/2)(Y_k J_m − J_k Y_m) (A&S 9.1.16 Casoratian); bounded with
  |J_k| <= min(1,(x/2)^k/k!) (A&S 9.1.60/9.1.62), |Y_k| <= M_k <= M_m <= |Y_m|+|J_m| (Nicholson, DLMF 10.9.30);
  step error <= 2^-100(|c f_k|+|f_{k+1}|) (Joldes-Muller-Popescu 2017 double-word bounds). Y's own bound solves
  E <= a + βE (β <= 2^-20 required).
* Backward: for k > x, r_k = J_k/J_{k-1} in (0, q_k], q_k = x/(2k−x); ε_k <= 2^-99 + q_k q_{k+1} ε_{k+1}, ε_{N+1}=1;
  N chosen so (n−m)Π q_j q_{j+1} <= 2^-110 (Gautschi 1967). Known looseness: Jb = 1 between x and ~1.36x makes yn
  decline for large x just above the turning point (follow-up: Kapteyn-type bound DLMF 10.14.7).
* Series: alternating, ratio <= 1/4, sum in [3/4,1]; prefactor by scaled products.

## Next step (exact)
1. Read clippy4 (`~/logs/libm-f64-bessel-n-clippy4.log` on keke); run `cargo xtask static`.
2. Write `crates/math/tests/fast_bessel_n.rs`: quick vs `special::jn_accurate`/`yn_accurate` (bits + status) in 4
   modes over n in 2..300 and -n, x log/uniform, operands next to zeros of J_n/Y_n; fallback-rate per regime; synthetic
   near64/near32 straddle cases (must be None); an #[ignore] multithreaded 10^7-operand sweep per function.
3. Plants (handoff/LIBM-F64-BESSEL-N-plants.txt): direction swapped (jn `up` forced), Miller normalisation dropped
   (`wj.f.mul(q)` -> `q`), near always true (`fast64::settle(ya.scale(s), eps * s` -> `0.0`).
4. Decision record D-10.76 (`cargo xtask records new decision D-10.76`), gate `cargo xtask affected --base origin/next`,
   counts `cargo xtask bench --compare --counters` for jn/yn rows, booked timing (IVB nico/kotori, N100).

## Not to redo
The method choice above (Casoratian runtime bound instead of interval/absolute recurrences, which blow up by
(1+√2)^n in the oscillatory region).
