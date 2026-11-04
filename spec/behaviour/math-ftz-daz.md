# libm under the caller's MXCSR FTZ / DAZ (software and operand-returning forms)

**Clauses:** `s10_0_f_0`.

Spec: §10.3, §10.6; decision D-10.22 (owner ruling 2026-10-23: the caller's FTZ/DAZ is honoured).
Probes: black-box C programs calling glibc 2.39 (Ubuntu 24.04, x86_64) after `ldmxcsr` with
0x1f80 (default), 0x9f80 (FTZ), 0x1fc0 (DAZ) and 0x9fc0 (both), printing result bits and the
raised underflow/inexact flags (MATH-M6-FPENV-HONOR-REST-2 on keke; the `faddl` family
MATH-M6-FPENV-HONOR-REST-3 on sumire). The probe sources are kept in the coordinator's
scratchpad (work/MATH-M6-FPENV-HONOR-REST-2, -3), not in the tree. Pinned by the table in
`crates/boundary/tests/s10_math_symbols.rs::math_software_paths_honour_the_callers_ftz_and_daz`.

| Family | Under DAZ | Under FTZ |
|---|---|---|
| fma, fdim, ldexp/scalbn, fadd/fsub/fmul/fdiv/fsqrt/ffma (from `double`) | a subnormal operand reads as a zero of its sign | a subnormal result is a zero of its sign, underflow + inexact |
| ceil/floor/trunc/round/roundeven/rint/nearbyint | the operand reads as DAZ does (`ceil(+sub)` = +0, `floor(-sub)` = -0) | no subnormal results |
| fmax/fmin/fmaxmag/fminmag | the operand as DAZ reads it is returned (a subnormal becomes 0) | never flushed |
| fmaximum/fminimum and the `_num`, `_mag`, `_mag_num` forms | compared as DAZ reads them, the RAW operand returned (`fminimum(+sub,+0)` = +sub; a same-sign tie gives x) | never flushed |
| nextafter | operands equal as DAZ reads them give `y` raw; else the raw `x` steps | never flushed |
| nextup/nextdown/nexttoward | the raw `x` steps | never flushed |
| next* flags | a step from an `x` DAZ reads as zero raises nothing; from a normal `x` as in the default word | as in the default word |
| fabs/copysign | unchanged | unchanged |
| daddl/dmull/dsqrtl, faddl/fsubl/fmull/fdivl/fsqrtl/ffmal (`long double` operands) | not applied (x87 operands) | never flushed; underflow + inexact as in the default word |

Recorded `faddl` family output (identical in all four modes, the MXCSR mode bits unchanged):

```
faddl(1e-40,0)         000116c2 UF=1 IX=1
fsubl(-1e-40,0)        800116c2 UF=1 IX=1
fmull(1e-20,1e-20)     000116c2 UF=1 IX=1
fdivl(1e-40,1)         000116c2 UF=1 IX=1
fsqrtl(1e-80)          000116c2 UF=1 IX=1
ffmal(1e-20,1e-20,0)   000116c2 UF=1 IX=1
faddl(1.5,0.5)         40000000 UF=0 IX=0
daddl(1e-310,0)        000012688b70e62b UF=1 IX=1
dmull(1e-310,1)        000012688b70e62b UF=1 IX=1
```

Not probed: the narrowing from `_Float128` (`f32addf128` ...), which Liella treats like the
narrowing from `double` (both rules apply).
