# TIMING-COVERAGE-1107 (planning; no code)

State: branch task/timing-coverage-1107 from origin/next 66b8d3ba; this commit adds handoff/timing-coverage-1107/
(table.md, README.md, six briefs, prof.sh, prof2.sh, counters-mei.txt) and this page. Full report: README.md there.

## Result
1106E slower entries (62 rows, IVB 56 + N100 20): covered 2 (first_localtime_r: PERF-ZONE-LOAD-FAULTS-3, verifying),
re-time 18 (first_mktime/first_snprintf: PERF-FIRSTCALL-MKTIME-SNPRINTF-2; IVB memcmp/bcmp/__memcmpeq rows:
TIMING-COV-MEMCMP-NONAVX2-4, landed after 34c98f61 and never timed), NONE 56. Briefs (rank order):
TIMING-COV-COLD-SETLOCALE (70), TIMING-COV-COLD-STRCOLL (68), TIMING-COV-WCTYPE-L (66), TIMING-COV-COMPARE-HEAD-SSE2
(64), TIMING-COV-STRSCAN-SHORT-IVB (62), TIMING-COV-STRSTR-PAIR (60). All Stacks-on: none.

## Decisions / rulings
- RULING NEEDED: the IVB malloc_free_*/realloc rows (1.11-1.30) are NONE and not briefed: the saturation register
  (docs/saturation/04-allocator.md) records a latency-chain limit, two owner refusals of the latest-free path
  (#2826, #3210) and a NEGATIVE shift variant. Recommendation: accept as saturated on Ivy Bridge (parity on the
  Skylake-SP build host: 46.3 vs 45.6 cycles) unless the owner wants another lever tried.
- COLD-SETLOCALE and COLD-STRCOLL touch the same path: dispatch them one after the other, not in parallel.

## Probe-settled / measured (mei, Skylake-SP class, build host, not timing verdicts)
Jobs timing-coverage-1107-prof (rc 0) and -prof2 (rc 0); numbers in counters-mei.txt. Cold: first_setlocale 110.0 vs
45.8 us, first_strcoll 11.7 vs 0.28 us. iswctype 76 vs 43 instr; __memcmpeq_head_128 25.4 vs 21.3 instr, 14.0 vs
8.4 cycles (-AVX2); strstr_hit_64 125-154 vs 99 instr. memchr_64/rawmemchr_4k IVB gaps do not reproduce off Ivy Bridge.

## Not to redo
The 62-row mapping (table.md), the profiles. Unbriefed groups ranked 7-15 in README.md (copy/fill 144 IVB is next).

## Next step
Coordinator: submit the six briefs; consider whether the memcmp re-time (step 1 of COMPARE-HEAD-SSE2) should be a
small timing-only story first.
