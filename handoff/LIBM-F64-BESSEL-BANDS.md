# LIBM-F64-BESSEL-BANDS handoff (run 1)

State: branch `task/libm-f64-bessel-bands` on next fe4c4893d; role keke:~/w/r5. Decision in force: D-10.110
(records/decisions/D-10/D-10.110.toml). Report: handoff/reports/LIBM-F64-BESSEL-BANDS.json. Plants:
handoff/LIBM-F64-BESSEL-BANDS-plants.txt.

## Done (band 1 of 3: tiny x)
`crates/math/src/fast_bessel.rs`: below 2^-27 the fast tier now answers every operand.
- `j0`, `j1`: read off the bits (`below`: the value or its neighbour toward 0, inexact), no `Mp` built;
  `j1` below 2^-1021 (subnormal results, before: 384-bit path) through `float::round` from
  `(2m − 1)·2^-1076` + sticky (underflow; `ERANGE` only for 0, `float::range_errno`).
- `y0`, `y1`: first-order series in double-double with `fast64::log_dd` (`ln_tiny`: subnormal `x` scaled by
  2^128, `128·ln 2` off), `y1` carried as `Y1·2^-128` and settled with `m = 128` (`round` takes `m` now);
  `Y1_OVERFLOW` (a subnormal): below it `elem::overflow`. Before: both declined below 2^-1000.
- Test: crates/math/tests/fast_bessel.rs `double_bessel_tiny_band_settles_and_matches_the_accurate_path`
  (subnormals, both boundaries 2^-1022/2^-1021, the `y1` overflow threshold, 2^-27; all modes, both signs for j).

## Profile (step 1) - NOT DONE
`validation/wl_trace.sh --spec validation/percall/libm_bessel.c --skip 62 . bes_y0_tiny y0` printed nothing in
40 min (job libm-f64-bessel-bands-trace1, stopped by me: the gdb single-step of a 1.4e5-instruction call plus
the build). The counts below come from wl_percall instead. Next run: trace the 2.7e3-instruction large-x call
(`bes_j0_large`, small) first, not the 1.4e5 one.

## Counts
wl_percall, instructions per call, Zen 1 (keke), base next fe4c4893d (percall1 on the first code head; percall3
re-ran it on the final head 198dd501e, whose later fixes touch only boundary operands outside the probe):
| case | glibc | base | new | new/glibc |
|---|---|---|---|---|
| bes_j0_tiny | 49.0 | 481.0 | 128.0 | 2.61 |
| bes_j1_tiny | 62.0 | 530.0 | 151.9 | 2.45 |
| bes_y0_tiny | 100.3 | 7325.7 | 470.5 | 4.69 |
| bes_y1_tiny | 57.2 | 7747.7 | 550.3 | 9.61 |
| bes_j0_large | 821.5 | 2712.9 | 2688.9 | 3.27 (band 2, untouched) |
| bes_j0_zero / bes_y1_zero | 433 / 436 | 146788 / 464227 | same | band 3, untouched |
Self-check MISMATCH on bes_y1_zero (y1(2.0), base and new alike) and bes_y0_tiny (y0(1e-10) vs the reference's
last place; the new test checks y0(1e-10) against the 384-bit path in all modes).
Fixes found by the new test: j1(2^-1021) Down/Zero needs underflow (420cbca69); y1 near its overflow threshold
(settle raises no overflow: 198dd501e, window to the 384-bit path). Jobs gate1/2, plants1/2 ran on older heads.

## Next step (bands 2 and 3, designed, not started)
1. Large x >= 32 (2705 / 830 instructions): `hankel_kernel` spends on `trig::sin_cos` (Payne-Hanek above 1500) and
   dd `div`/`sqrt`/two dd Horners. Design: modulus-phase `J = M cos θ`, `Y = M sin θ` with
   `M = a·sqrt(P² + Q²)`, `θ = χ + atan(Q/P)` (exact identities from the Hankel form). Generate in
   crates/mathtools/src/bessel_tables.rs (next to `emit_hankel`) the formal power series in `w = 1/x²` of
   `S(w) = sqrt(p(w)² + w·q(w)²)` and `D(w)` with `atan(Q/P) = D(w)/x` (series division, odd powers of
   `R = q/p`), first omitted terms at 32 below 2^-74. Reduction: x·(64/π) mod 128 exactly in fixed point
   (53-bit significand times a 192-bit window of `coeffs::trig::TWO_OVER_PI`; window start
   max(1, E − 6) for x = m·2^E; truncation below 2^-130), add `δ·64/π` and subtract `(2n+1)·16` in the same
   fixed point, then `trig::poly` + `trig::assemble(k)` / `(k + 32)` (FMA and baseline kernels exist).
   Error budget: θ to 2^-70 needs `D_0·t` and `D_1·w·t` in dd (`t = 1/x` dd), `S_1·w` in dd.
2. Near zeros below 32 (5e4-2.2e5 / 416-525): second tier after a refused `table_kernel`: Taylor rows about
   each zero (39 zeros), zero as triple-double, coefficients from the ODE recurrence already in
   `bessel_tables.rs::taylor4` (needs a Big centre), relative bound; Harrison, ARITH 2009.
Not to redo: the tiny band; the D-10.106 errno rule (probes settle it).
