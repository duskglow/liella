# VALIDATION-ROWS-STALE: done (validation/README.md refreshed)
Branch task/validation-rows-stale-2. Job validation-rows-stale-so ran on shiki:~/w/sh2 (rc 0, 79 s).
Evidence (nm -A --defined-only target/liella/libc.a / libm.a): libc.a has all T23 names (frexp ldexp modf scalbn finite isinf isnan,
__ forms, f/l forms, __signbit/f/l as W; __fpu_control as V); libc.a has no fmod/fmodf/fmodl; libm.a has T fmod/fmodf/fmodl.
Edited: T23 and T34 status cells (fixed, cited check), T24 "Rechecked 2026-11-04: unchanged", next-steps item 0.
Not done / not redone: suites (findutils, gawk as is) were not re-run; item 1 of the next-steps list still stands.
