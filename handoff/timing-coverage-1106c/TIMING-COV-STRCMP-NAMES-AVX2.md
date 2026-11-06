# TIMING-COV-STRCMP-NAMES-AVX2: strcmp on short names and strncasecmp 64 on the AVX2 class

Id: TIMING-COV-STRCMP-NAMES-AVX2
Kind: code
Priority: 55
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 70 calls / 140k
Reserve: D-4

## The change

Bring cbench_opt/strcmp_names and cbench_locale/strncasecmp_c_64 to time parity on the AVX2 class. Both are at
instruction parity; TIMING-COV-STRCMP-SHORT-AVX2-2 and PARITY-STRCMP-AVX2 landed before the 1106 sweep and did not
close them. Decision rows in force: D-4.29, D-4.31, D-4.38 (strcmp head, exact page test, page-end step-back),
D-4.27 (casecmp kernel), D-4.33 (64-byte entries). Read handoff/TIMING-COV-STRCMP-SHORT-AVX2.md first.

## Rows (TIMING-SWEEP-1106, N100; instr / time)
strcmp_names 1.022 / 1.110 (1105 1.142); strncasecmp_c_64 1.029 / 1.050 (1105 1.099). Ivy Bridge: 0.773 and 0.947
(faster).

## Evidence (handoff/timing-coverage-1106c/README.md section 3, Haswell-EP build host, default level = AVX2)
strcmp_names 64.5 instr / 23.6 cycles vs glibc 63.1 / 20.8 (1.14x), branch misses 0.023 vs 0.011 per call.
strncasecmp_c_64 70.0 / 18.1 vs 68.0 / 17.1 (1.06); idq_uops_not_delivered 3x glibc's (aggregate); Liella samples:
casecmp_ord_avx2 71%, the strncasecmp export 17% (two functions per call). Both reproduce on the build host.

## Hypothesis (NOT VERIFIED)
strcmp_names: names differ in the first bytes; the extra mispredicts (+0.012/call) are too few for the 2.8-cycle gap,
so look at the D-4.38 head's page test and the order computation latency first (perf record of Liella's
liella_machine_strcmp by instruction on the build host; glibc aggregate counters only). strncasecmp_c_64: front-end
bound and a hop from the export into the kernel: try entering the kernel directly (no wrapper frame) as the strcmp
exports do, within D-4.27.

## Steps
1. Liella-only per-instruction samples of the two rows on the role host; name the cycle sink against D-4.38 / D-4.27.
2. Fix it; region counters (`CBENCH_ROWS=<row> CBENCH_COUNTERS=1`) before/after, plus every strcmp/strncmp/strcasecmp
   row of cbench_hot, cbench_opt, cbench_locale at default and hwcaps=-AVX2: no row slower.

## Tests / plants
Spec clauses §4.3 (strcmp order sign), §5.3 (case-insensitive compare in the C locale). Plants via `cargo xtask
plants`: wrong sign on the first differing byte; page-end case taken one byte early; strncasecmp limit off by one.

## JOBS
gate on the role; plants; counts (`bench --compare --counters` cbench_opt, cbench_locale, cbench_hot); booked timing
check of the two rows on the N100 timing host (`ops timing-book`), Ivy Bridge rows must stay faster.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
