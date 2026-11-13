# LIBM-RETHINK-GAMMA-2: finish the gamma band forms (continuation of LIBM-RETHINK-GAMMA)

Kind: code. Model: opus. Effort: max. Stacks-on: task/libm-rethink-gamma (or next once it lands).
Read handoff/libm-rethink-gamma.md first (traced bands, design, what is done).
1. Verify run 1's f64 bands: direct-tier gate, plants through `cargo xtask plants`, after-probe vs
   ~/work/libm-rethink-gamma/before (keke), D-10.105 record (reserved by run 1's brief).
2. Remaining bands, same method (closed form + proved bound + settle / fast32 rounding): f64 x <= -16
   (reflection), f64 lgamma [2^1012, 2^1015) and tgamma subnormal x (settle at m = 1023 with overflow), tgammaf
   tiny, tgammaf x <= -34 / x >= 35, lgammaf x <= -42 (subnormal/overflow float results). Exhaustive sweeps for
   every changed f32 function.
Done means: each band's mean within 2x of glibc's by the probe tool; sweeps green.
