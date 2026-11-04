# MATH-F128-WIDE-LN-PROVEN-2 handoff

State: branch task/math-f128-wide-ln-proven-2, one code commit on top of next (ZIV-C landed there): the ten
`mp::ln` callers (wide.rs log/log2/log10/pow; special.rs Stirling, shifted product, reflection x2, jn/yn x2) now call
`mp::ln_atanh`. Kernel files (mp.rs, mpw.rs) untouched: next's versions are used. `mp::ln` stays for complex.rs.

Decision in force: D-10.24 (plan/07-decisions/s10-mathematics.md; bounds per caller, as run 1's D-10.22 text; the
number was assigned by the coordinator because next's D-10.22 is the sqrt row). wide.rs module doc cites D-10.24.

Open: correct rounding of log/pow of _Float128 still rests on the 2*113+10 estimate (named unproven in doc and row).
Not to redo: caller list and bound derivation. Possible follow-up: log10 costs ~2x log because mp::ln10() recomputes
ln_atanh(10) each call; a constant table would remove it (run 1 count: log10f128 38.5M vs logf128 17.1M instr/200 calls).

Results: clippy --workspace --all-targets --all-features -D warnings rc 0; cargo xtask static rc 0 (shiki sh1).
Gate and plants: jobs math-f128-wide-ln-proven-2-gate / -plants on shiki (see handoff/reports json).
