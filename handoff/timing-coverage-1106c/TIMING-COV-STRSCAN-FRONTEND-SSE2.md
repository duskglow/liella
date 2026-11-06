# TIMING-COV-STRSCAN-FRONTEND-SSE2: strchr / strchrnul / strncmp short heads are front-end bound on the SSE2 class

Id: TIMING-COV-STRSCAN-FRONTEND-SSE2
Kind: code
Priority: 50
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Reserve: D-4

## The change

TIMING-IVB-STRSCAN-DISPATCH-2 (landed before the 1106 sweep) cut these rows 0.17-0.33 but left them slower at
instruction parity. Continue from its handoff "Open / next step" (handoff/TIMING-IVB-STRSCAN-DISPATCH.md): items 1 and
2. Decision rows in force: D-4.35 (one-pass kernels), D-4.39 (no per-call CPU-path test), D-4.45 (short heads before
any setup), D-4.33 (64-byte entries), D-4.21 (strncmp deciding-byte re-read). Read the red PERF-STRCHR-NONAVX2,
PERF-STRNCMP-ROWS and STR-CHR-CMP-SHORT-2 handoffs before trying a layout.

## Rows (TIMING-SWEEP-1106, Ivy Bridge; instr / time)
strchr_16_hit 1.000 / 1.116; strchr_64_hit 1.000 / 1.135; strchr_64_miss 1.000 / 1.288; strchrnul_16_hit 1.000 /
1.221; strchrnul_64_hit 1.000 / 1.267; strchrnul_64_miss 1.000 / 1.188; strncmp_eq16 1.015 / 1.158. N100: faster.

## Evidence (handoff/timing-coverage-1106c/README.md section 3, Haswell-EP build host, hwcaps=-AVX2)
Equal instructions; cycles strchr_16_hit 9.05 vs 8.55, strchr_64 16.2 vs 14.6, strchrnul_16_hit 9.05 vs 8.05,
strchrnul_64 14.1 vs 14.1, strncmp_eq16 18.1 vs 17.1. idq_uops_not_delivered 1.9x glibc's (aggregate), resource
stalls negligible: front-end bound. Smaller than on Ivy Bridge but the same sign, so iterate on the build host and
confirm on Ivy Bridge.

## Hypothesis (NOT VERIFIED)
The SSE2 heads' blocks cross 16-byte fetch windows / exceed the decoded-uop cache's per-32-byte-window limit, so the
legacy decoder feeds them (the handoff's candidate 1); the strncmp eq row adds the D-4.21 re-read's load-use latency
(candidate 2). Check with Liella-only samples by instruction and `idq.mite_uops` vs `idq.dsb_uops` per row (Liella;
glibc aggregate only).

## Steps
1. Per-row Liella counters (`CBENCH_ROWS=<row> CBENCH_COUNTERS=1 GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2`), DSB vs MITE
   uops; name the window/instruction that falls out of the uop cache.
2. Re-lay the head (alignment, block order, macro-fusion pairs) in csys/src/machine; anchor .S plants to the function
   label. AVX2 rows must not get slower.

## Tests / plants
Spec clauses §4.5 (strchr/strchrnul result, terminator match), §4.3 (strncmp bound). Tests named explicitly
(liella-boundary machine_scanners / small_copy: the affected tier selects nothing for .S files). Plants: hit one block
late; terminator not matched for c == 0; strncmp limit off by one.

## JOBS
gate on the role; plants; counts (`bench --compare --counters cbench_hot`, default and hwcaps=-AVX2); booked timing
check of the seven rows on an Ivy Bridge timing host (`ops timing-book`, the less busy of the two) and the N100.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
