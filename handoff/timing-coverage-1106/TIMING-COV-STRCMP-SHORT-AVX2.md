# TIMING-COV-STRCMP-SHORT-AVX2: short strcmp and case-insensitive compares on the AVX2 class

Id: TIMING-COV-STRCMP-SHORT-AVX2
Kind: code
Priority: 60
Size: M
Host: -
Stacks-on: TIMING-IVB-STRSCAN-DISPATCH-2
Model: opus
Effort: high
BUDGET: 80 calls / 160k

## The change
Cut the AVX2-build head of liella_machine_strcmp (strings 8-64 bytes, difference early) and of liella_machine_casecmp_avx2
(16 B equal-ignoring-case, strncasecmp n = 64), csys/src/machine/scan.S. PARITY-STRCMP-AVX2 (landed after the sweep)
changed only the >= 1 KiB blocks; PERF-STRCMP-MIX recorded a ~1.18-1.22x head bound: read its saturation row first and
do not repeat what it tried. PERF-STRNCASECMP-64-AVX2 (red, unlanded) took strncasecmp_c_64 94 -> 76: ask the coordinator
whether to start from its branch. Stack on TIMING-IVB-STRSCAN-DISPATCH-2 (queued_ci, same file, dispatch change).

## Rows (TIMING-SWEEP-1105, N100, instr / time)
strcmp_names 1.062 / 1.142, strcasecmp_c_16 1.080 / 1.136, strncasecmp_c_64 1.118 / 1.099. Ivy Bridge: parity or faster.

## Evidence (keke, AVX2 path, next 730f416b, job timing-coverage-1106-percall)
Liella / glibc instructions per call: strcmp_names 46.4 / 38.0, strcmp_16_mix 59.8 / 50.5 (validation/percall/strcmp.c),
strcasecmp_c_16 58 / 53, strncasecmp_c_64 80 / 71. Workloads: strcmp 12.8M calls in 25 of 30 workloads (the most
widespread compare); strcasecmp / strncasecmp are not called by this workload set (rank them below strcmp).

## Hypothesis
8-9 instructions per call of fixed head cost (entry, page-cross test, mask build) before the first 32-byte compare;
the N100 issue width makes it time. Fold the page test into the first load's address check and return from the first
mask without the second dispatch branch.

## Steps
1. wl_percall --trace strcmp:200:200 for strcmp_names (Liella only); list the head instructions per class.
2. Change the head; keep the >= 1 KiB path from PARITY-STRCMP-AVX2 as is.
3. Tests: strcmp / strcasecmp / strncasecmp every length 0-130 x difference position x page-end placement, both paths.
4. Plants: page-cross test removed, sign of the first-block result inverted, case fold skipped for 'Z'+1.

## JOBS
gate, plants, counts before / after (wl_percall --spec validation/percall/strcmp.c and handoff/timing-coverage-1106/tc1106.c
--cases strcasecmp_c_16,strncasecmp_c_64), timing nozomi:timing-cov-strcmp-short-time (N100 class; `ops timing-book nozomi
<min> --story TIMING-COV-STRCMP-SHORT-AVX2`; confirm nozomi is the N100 host first) and nico:timing-cov-strcmp-short-ivb
(no Ivy Bridge row slower).

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
