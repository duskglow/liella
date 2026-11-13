# LIBM-F64-EXP2-SSE2-DEKKER-AB: measure D-10.64's SSE2 trade (dropped low part vs Dekker product)

Kind: code. Model: sonnet. Effort: high. Stacks-on: PERF-SSE2-COUNT-AUDIT (D-10.64 follow-up). Size: S.

D-10.64 drops the low part of the reduction product `r = s·ln2` (exp2) and `(sh + sl)·ln10` (exp10) in the SSE2 build
instead of computing it with a Dekker product. This doubles the bound (`RHO_FAM_SSE2`, expected fallback 1.14%
instead of 0.57%). The record argues this costs about 1 instruction per call against about 6 for the product. That
was never measured. The SSE2 counts on next are exp2 100.4 / 51.1 and exp10 120.8 / 56.1 (Liella / glibc
instructions per call, `--tunables glibc.cpu.hwcaps=-FMA`; handoff/reports/PERF-SSE2-COUNT-AUDIT.md).

1. In a scratch build, add the Dekker product (Dekker 1971) to the SSE2 path of exp2 and exp10 only, with the FMA
   bound `RHO_FAM_F`.
2. Run an A/B count with `cbench_libm --compare --counters --runs 3 --tunables glibc.cpu.hwcaps=-FMA` on exp2 and
   exp10. Also measure the fallback rate on both sides with the existing test's counter.
3. Keep the variant with fewer instructions per call. If the Dekker variant wins, time it on Ivy Bridge (the
   instruction mix changes), amend D-10.64 with a new record, and add a plant (the low part dropped while the
   tight bound stays). If it loses, record the measured figures as an amending record and add a NEGATIVE row to
   the saturation register.
