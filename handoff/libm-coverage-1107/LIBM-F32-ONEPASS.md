# LIBM-F32-ONEPASS: binary32 exp/log/pow/trig evaluated once in double, proven exhaustively

Id: LIBM-F32-ONEPASS
Kind: code
Priority: 83
Size: L
Host: -
Model: opus
Effort: high
BUDGET: 100 tool calls / 200k

## The change

Decision row: DESIGN.md 3.1 A/B under D-10.51's two-phase principle (if a new row is needed: the coordinator reserves
the D-id). Rows (time IVB / N100): expf 2.11 / 2.60, exp2f 2.31 / 2.59, exp10f 2.60 / 3.06, expm1f 1.60 / 1.04, logf
2.60 / 3.50, log2f 2.35 / 2.99, log1pf 2.18 / 2.18, powf 4.47 / 4.86, sinf 2.05 / 3.28, cosf 2.28 / 3.12, expf_qnan
3.57 / 4.26; after a confirming re-time also log10f, tanf, coshf (instructions at or below glibc, time 1.2-1.3x).
Counts (keke:libm-coverage-1107-percall): expf 73 vs 42, exp2f 73 vs 40, logf 80 vs 46, log2f 81 vs 45, sinf/cosf 93
vs 45, powf 243 vs 78. Tier today: fast32 double kernel + a rounding check per call, MXCSR read on entry.

METHOD (a): one evaluation in double with relative bound ~2^-45..-50 (Tang table + short polynomial, Estrin), then one
`cvtsd2ss` in the caller's rounding mode: mode-independent hot path, no MXCSR read or switch, no software flags (the
conversion raises inexact/overflow/underflow itself; check underflow-after-rounding per D-10.14 on the subnormal
boundary inputs exhaustively). Correctness by exhaustion: all 2^32 inputs x 4 modes against the oracle (`oracle32.rs`;
the exhaustive sweep tooling exists: `crates/math/src/fast32/tests.rs`); inputs whose double result lies within the
bound of a float rounding boundary go to a small exception table (generated, `cargo xtask tables`) or an exact check.
powf: method (b), the same double evaluation (log2 x in double ~2^-50, y·log2 x exact in double since 24x24 bits fit,
exp2) plus an online rounding test with a fallback to today's powf, because 2^64 inputs cannot be exhausted. expf_qnan:
the NaN path must not pay the kernel or the envelope (test order). Literature: Tang 1989/1990; Muller et al., Handbook
of Floating-Point Arithmetic 2nd ed. 2018 ch. 10-11 (exhaustive testing of binary32); Lefèvre-Muller worst cases.
Profile first: `wl_trace.sh` one call of expf and sinf (where 31-48 extra instructions go: entry, check, kernel).
Split: kernels + exhaustive proof first (exp family, log family, trig), wiring per family if fast32.rs reads are large.

## Tests and plants
Existing: exhaustive f32 sweeps (`fast32/tests.rs`, `oracle32.rs`), `elem_products.rs`, `s10_math_symbols.rs`.
New: the exhaustive 4-mode job per function (0 mismatches, flags and errno compared), the exception-table generator's
check. Plants: exception table entry dropped, conversion done in round-to-nearest instead of the caller's mode, sinf
reduction constant 1 ulp off, powf online test always true: each CAUGHT with msg:.

## JOBS
clippy + static, direct gate, plants, the exhaustive proof jobs, counts before/after (expf exp2f logf log2f sinf cosf
powf), booked timing of the rows above on Ivy Bridge and N100. Targets: expf/logf/sinf <= glibc instructions,
time <= 1.0x; powf <= 1.3x.

Hand-back first line: `COMPLETE: f32 one-pass <functions>, IVB <x>x / N100 <y>x` or INCOMPLETE:/DO NOT LAND:.
