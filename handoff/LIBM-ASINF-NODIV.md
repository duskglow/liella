# LIBM-ASINF-NODIV: asinf/acosf/atanf without the per-call division (#4924)

State: branch task/libm-asinf-nodiv-2 (run 2, continues task/libm-asinf-nodiv), code head 9b28981f1 (origin/next
d1b3a596c merged in 3b06cb20a), handoff/report commit on top. Decision in force: D-10.103 (amends D-10.82; D-10.68's
partial-register check). Report: handoff/reports/LIBM-ASINF-NODIV-2.json; plants: handoff/LIBM-ASINF-NODIV-2-plants.txt.

## What the change is (run 1, unchanged in run 2)
- asinf/acosf: |x| < 1/2 polynomial x + x·z·R(z) (`fits/fast32_asin.fit`, odd 1..23, 2^-51.9, Estrin up to z^4);
  |x| >= 1/2 half-angle pi/2 - 2 asin sqrt((1-|x|)/2) (Cody & Waite ch. 10): one sqrtsd, no division (was sqrt + 2 div).
- atanf |x| > 1: one division (was two): c = j/32 from the 160-byte `ATAN_RECIP_J` (exponent + top 5 fraction bits),
  u = (1 - x c)/(x + c), fit `fast32_atan32.fit` (odd 1..9 on [-1/32, 1/32], 2^-62.1). |x| <= 1 unchanged.
- Bounds: asinf 19.3u, acosf <= 15u, atanf above 1 5.4u (u = 2^-53 relative), shared INV_ONEPASS_BOUND 24 / DELTA 64.

## Run 2
- Run 1's jobs (shiki): sweeps rc 0 (asinf, acosf, atanf exhaustive, 4 directions, 0 mismatches); plants2 rc 1:
  `atan-table-short` SURVIVED. Cause: the kernel test held atan above 1 only to the shared 24u; a table cut at 128
  (j = 0 from |x| = 16) leaves |u| up to 1/16 and an error of 16.2u, inside 24u. Fix (561a977e1, 9b28981f1): the
  test holds atan above 1 to its proof's 5.4u relative. Now CAUGHT (`atan(0x41800016): -16.15 u above 1`).
- Gate red of run 1 (first localtime_r 21 > 20 faults) is gone after merging origin/next (LAYOUT-STARTUP-STATE-LOCALE
  landed): gate on 561a977e1 green, 2581 passed / 0 failed, conformance 18/0, peak 3.9G.
- Final head 9b28981f1 (mei, role s1): libm-asinf-nodiv-2-gate2 green (2581 passed / 0 failed, conformance 18/0,
  peak 1.8G); libm-asinf-nodiv-2-plants2 5 CAUGHT / 0 SURVIVED / 0 WARN. Counts (mei) and timing (kotori, nozomi)
  ran on 561a977e1's staged products; 9b28981f1 changes only a test comment, so the products are the same.
- Superseded: libm-asinf-nodiv-2-gate and -plants (green, on 561a977e1 before the comment fix).

## Measurements (staged trees: branch 561a977e1 vs origin/next d1b3a596c, each `--compare` against glibc 2.39)
Instructions per call (Skylake-SP build host, `--counters`, deterministic): branch / next / glibc
asinf 69.3 / 72.4 / 53.5; acosf 70.1 / 72.2 / 61.2; atanf 65.4 / 68.8 / 66.6 (atanf 0.98x glibc).
Booked timing, ns per call, median of 5, two interleaved passes (identical to 0.1 ns):

| row | Ivy Bridge (kotori) branch / next / glibc | N100 (nozomi) branch / next / glibc |
|---|---|---|
| asinf | 19.0 / 48.5 / 14.4 (1.31x, was 3.37x) | 13.05 / 22.1 / 7.8 (1.66x, was 2.82x) |
| acosf | 19.05 / 47.9 / 18.9 (1.01x, was 2.52x) | 13.1 / 23.1 / 9.95 (1.31x, was 2.31x) |
| atanf | 22.6 / 33.6 / 22.9 (0.98x, was 1.47x) | 12.65 / 16.2 / 12.15 (1.04x, was 1.34x) |

Skylake-SP (build host, unbooked, indicative only): asinf 8.8 / 17.6 / 6.2, acosf 7.7 / 15.7 / 7.6, atanf 9.2 / 11.5 / 8.6.
Zen 1 not timed: keke cannot be booked; ruling #4932 makes Ivy Bridge and N100 the verdict.
Verdict: every function faster than next on every host measured; target (glibc parity) not reached for asinf
(1.31x / 1.66x) and acosf on N100 (1.31x).

## Not to redo
The sqrt operand is formed twice on purpose (D-10.68 partial-register trap, run 1). The exhaustive sweeps passed on
run 1's code (math unchanged since). Staging next needs a git tree: run 2 put a depth-1 clone of origin/next under
the role's target/bench-base/ and ran `cargo xtask bench --stage` there.

## Next step (a follow-up story, optional)
asinf's remaining gap is latency in the |x| < 1/2 polynomial (degree 23, 12 terms) and the sqrt chain above 1/2.
Candidates: a shorter fit per half (split at 1/4 with a second table point), or a pre-scaled Estrin tree to cut the
dependent multiply depth; measure on Ivy Bridge and N100 with the recipe above.

Plan rows: D-10.82, D-10.68, D-10.103 (records/decisions/D-10/).
