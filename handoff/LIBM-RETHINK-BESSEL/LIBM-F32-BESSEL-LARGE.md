# LIBM-F32-BESSEL-LARGE: j0f/j1f/y0f/y1f for |x| >= 2^19 without the multi-precision path

Kind: code. Model: opus. Effort: max. Stacks-on: LIBM-RETHINK-BESSEL (D-10.106). Size: L.

Probe rank 4 (LIBM-TAIL-PROBE): 21-43% of float bit patterns cost 1.67e5-2.87e5 instructions against 269-356.
Method (handoff/LIBM-RETHINK-BESSEL.md, approach A): J_n = M_n cos θ_n, Y_n = M_n sin θ_n (DLMF 10.18.17-18),
θ_n = x - (2n+1)π/4 + (μ-1)/(8x) + (μ-1)(μ-25)/(384x³), M_n = sqrt(2/(πx))·(1 + (μ-1)/(16x²)), μ = 4n². Reduce
x·2/π exactly in fixed point (the 24-bit mantissa times a 128-bit window of 2/π's bits, u128 wrapping product),
subtract (2n+1)/2 there, take the quadrant from the top bits and t = f·π/2 + phase correction, then one sin or cos
polynomial. Per-call relative bound into `round`; refused operands go to an exception table found by an exhaustive
enumeration (expected 30-60 per function), with no runtime MP. Also extend `oracle32::bessel_est` above 2^19 with
the same reduction in double-double so the exhaustive sweeps can check the band. Plants: a table entry removed, the
margin halved, the x^-3 phase term dropped. Done: band mean within 2x of glibc, exhaustive sweeps green.
