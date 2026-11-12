# LIBM-F64-INVHYP-SSE2-PROFILE: find where the SSE2 build of asinh/acosh/atanh spends its extra 51-64 instructions

Kind: code. Model: opus. Effort: high. Stacks-on: PERF-SSE2-COUNT-AUDIT (D-10.65 follow-up, #5057). Size: M.

Evidence (handoff/reports/PERF-SSE2-COUNT-AUDIT.md, `cbench_libm --compare --counters --runs 3`, Haswell-EP host,
next at the audit's base; Liella / glibc instructions per call):

| row | SSE2 build (`--tunables glibc.cpu.hwcaps=-FMA`) | FMA build | SSE2 - FMA |
|---|---|---|---|
| asinh | 197.5 / 115.1 | 133.6 / 99.4 | +63.9 |
| acosh | 184.6 / 98.8 | 122.6 / 83.6 | +62.0 |
| atanh | 191.3 / 130.7 | 140.2 / 112.7 | +51.1 |

The exp and log families lose only 14 to 33 instructions in the SSE2 build, so the D-10.65 entries pay about twice
as much. The likely cause is NOT VERIFIED. D-10.65 uses exact products, 2MultFMA in the FMA build and Dekker's
product (Dekker 1971) in the SSE2 build, in three places: the pair argument of the logarithm, the Newton-corrected
root and atanh's division remainder. Each Dekker product costs about 13 more operations than 2MultFMA.

1. Profile first. Use `validation/wl_percall.sh` (or `wl_trace.sh`) with the base primed to split one SSE2 call per
   phase: argument pair, root and Newton step, `log_pair_parts`/`log1p_pair_parts`, and Ziv test. Record the split
   against glibc's total at the same level in your handoff.
2. For each exact product in the SSE2 path, work out (in the module docs) whether the stated bound `RHO_PAIR = 2^-63`
   needs its low part. D-10.64 dropped one such low part and doubled its bound, at about 1 instruction per call of
   extra fallback. Where the bound allows it, drop the low part or use a cheaper split (a Veltkamp split of a
   constant done once at build time). Re-derive the bound and the expected fallback rate.
3. Keep only changes that make no measured row slower. Count both builds (`--tunables glibc.cpu.hwcaps=-FMA` and
   default). The FMA build must not change. Time the SSE2 build on Ivy Bridge, because the change alters the
   instruction mix.

Tests: `crates/boundary/tests/invhyp_quick.rs` (both builds; correct rounding against the accurate path, fallback
rate). Plants: a dropped low part the bound does need, and a halved Ziv margin. Done: the SSE2 - FMA gap is at or
below the exp/log families' 33, or a decision record explains why it cannot be.
