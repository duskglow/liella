# Handoff: timing-coverage-1106c (TIMING-COVERAGE-1106C)

State: branch task/timing-coverage-1106c, stacked on TIMING-SWEEP-1106-2 (c7f3acf9; sweep build next 4d454f06).
Planning only, no product code. Full report: handoff/timing-coverage-1106c/README.md.

## Result
- (c) 19 improved rows: one open covering story, TIMING-COV-SPAWN-HEAP256M-IVB (verifying) -> close. The other 18
  were moved by stories already landed in the sweep's build: nothing else to close.
- (b) 39 reopen rows: 18 covered by open stories (TIMING-IVB-MEMREF-ENTRY-2 9, TIMING-N100-MEMCMP 2,
  TIMING-N100-SCAN-64K-3 2, NULLFOLD-COPY-MID-4 2, TIMING-N100-MEMMOVE-4K-REGRESS 1, TIMING-COV-STRSTR-NONAVX2 1,
  TIMING-COV-REGEX-FIRSTBYTE-IVB 1); 19 NONE in four groups (malloc 8, strscan 7, strcmp 2, locale 2); 2 noise.
- Briefs (top 3 groups): handoff/timing-coverage-1106c/TIMING-COV-MALLOC-PAIR-CHAIN.md (Reserve: D-3),
  TIMING-COV-STRCMP-NAMES-AVX2.md (Reserve: D-4), TIMING-COV-STRSCAN-FRONTEND-SSE2.md (Reserve: D-4). Group D
  (nl_langinfo/towupper_l IVB): no brief, one row reproduces, lowest weight.
- Noise / marginal: first_bindtextdomain_gettext, openpty_close, mkstemp_unlink_close: what settles each is in README
  section 4; no brief.

## Evidence
Job shiki:tc1106c-prof (rc 0, 9.2 min): Liella vs glibc region counters and aggregate perf stat, Liella samples by
symbol, on the Haswell-EP role host (shiki:~/w/sh2, tree 4741f943). The malloc, strcmp and strscan gaps reproduce
there, so those stories can iterate on a build host and book timing hosts only for the final check.

## Not verified / limits
- Profiles are Haswell-EP, not Ivy Bridge or N100: they show the sign and the stall class, not the timing hosts' size.
- The whole-process sample list for cbench_malloc is dominated by xthread rows that CBENCH_ROWS did not filter.
- The 1106B coverage table (on next after 4d454f06) was not opened; its notes are carried in the sweep table.
- Causes in the briefs are hypotheses, checked against D-4.21/27/29/31/33/35/38/39/45 and heap.rs, not measured.

## Next step
Coordinator: close TIMING-COV-SPAWN-HEAP256M-IVB when its own verify agrees; submit the three briefs with
`ops submit --reserve`; re-time first_bindtextdomain_gettext on a build containing COLDSTART-BINDTEXTDOMAIN-REBIND.
