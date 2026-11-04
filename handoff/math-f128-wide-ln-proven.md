# MATH-F128-WIDE-LN-PROVEN handoff (NOT READY: direct gate red)

State: branch task/math-f128-wide-ln-proven (from origin/next f3b3584a), commits: (1) kernel import of mp.rs, mpw.rs,
special.rs from origin/task/math-m5-cplx-f128-ziv-b2-3 per ZIV-CHAIN-PLAN 1'; (2) 90c18ac6 switch of all ten mp::ln
callers (wide.rs log/log2/log10/pow; special.rs Stirling, shifted product, reflection x2, jn/yn x2) to mp::ln_atanh,
wide.rs module doc, new row D-10.22 (proven bounds per caller). mp::ln kept: root complex.rs (539, 1565) still calls it.

Callers and claims (D-10.22): log/log2/log10/pow of long double, _Float64x, _Float128 rest on D-10.19 correct rounding;
with ln_atanh: log* within 2^-257 relative (x87 2^-308), pow's y*ln|x| within 2^-244 absolute; special.rs sites within
2^-371 relative / 2^-372 absolute, inside D-10.20 budgets. Correct rounding still rests on the 2*113+10 hardest-case
estimate (named as unproven in wide.rs and D-10.22).

Jobs on shiki (role sh2): math-f128-wide-ln-proven-gate rc=1: 335 passed, 1 failed:
liella-math mpw::tests::wide_kernels_within_their_proven_bounds (mpw.rs, imported unchanged from B2-3; its line 118
compares mpw::ln with the Halley mp::ln). My change does not touch mp::ln or mpw.rs, so it most likely fails on commit 1
already: the B2-3 kernels on next's base. ZIV-CHAIN-PLAN 1' says stop and hand back when the kernels fail on next.
math-f128-wide-ln-proven-plants: baseline ok; series-one-bit-loose EQUIVALENT (inside the 4-bit stated margin, no
result can move: no test can catch a one-bit loosening of a bound with margin); series-third-precision CAUGHT
(float128_elementary_functions_against_the_reference); log-back-to-halley still running at hand-back (expected
EQUIVALENT: both ln forms are far inside 236 bits; only the proof differs).
math-f128-wide-ln-proven-cmp (still running at hand-back): after-state counts instructions:u for 200 calls (minus
none=106629): logf128 17.09M, log2f128 18.82M, log10f128 38.53M, powf128 25.63M, lgammaf128 73.29M, logl 17.12M,
powl 25.61M. The before state and the oracle-output diff (elem/special f128 and ld) are in ~/logs/math-f128-wide-ln-proven-cmp.log.
log10 costs about 2x log: mp::ln10() recomputes ln_atanh(10) every call (B2-3's mp.rs); a constant table would remove it.

Superseded: math-f128-wide-ln-proven-gate0 (rc 143: killed by me, ran on commit 1 while I synced commit 2).

Next step: run `cargo test --manifest-path crates/math/Cargo.toml --lib mpw::tests::wide_kernels_within_their_proven_bounds`
on commit 1 to confirm the failure comes from the import; if so this story waits for ZIV-C to land (plan 1'), then
re-applies commit 2 on next. Read ~/logs/math-f128-wide-ln-proven-cmp.log for the before/after counts and any changed lines.
Do not redo: the caller list and bound derivations (D-10.22, wide.rs doc).
