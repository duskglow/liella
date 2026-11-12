# LIBM-F64-BESSEL-BANDS: j0/j1/y0/y1 tiny x, large x and near zeros

Kind: code. Model: opus. Effort: max. Stacks-on: LIBM-RETHINK-BESSEL (D-10.106). Size: L.

LIBM-TAIL-PROBE (f64): y0 for x < 9.3e-302 1.38e5 / 89 instructions (the p99.99 1.4e5), y1 for x < 5.6e-17 4466 /
41, j0/j1 tiny 469-504 / 37-50; |x| >= 32: 2705 / 830 with isolated 1.4e5-1.8e5 refusals; the probe's hard points
around zeros 5e4-2.2e5 / 416-525. Step 1: wl_trace one call per band with validation/percall/libm_bessel.c
(bes_y0_tiny --skip 62, bes_y1_tiny, bes_j0_large, bes_j0_zero, bes_y1_zero). Method: tiny x, D-10.106's series in
double-double (log_dd), J1 subnormal results by bits; large x, the modulus-phase form with an exact fixed-point
reduction (a 53-bit mantissa times a 192-bit window of 2/π), terms of θ and M to 2^-70; near zeros below 32, a
double-double second tier (Taylor expansions about each zero, Harrison, "Fast and accurate Bessel function
computation", ARITH 2009) so only isolated hard inputs reach the last Ziv tier. Done: each band within a small
factor of glibc, the f64 sweeps and the probe's hard points green.
