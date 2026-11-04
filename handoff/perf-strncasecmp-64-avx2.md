# PERF-STRNCASECMP-64-AVX2 (handoff)

State: branch task/perf-strncasecmp-64-avx2, code commit 0fb43a89 (csys/src/machine/scan.S), plus docs/report commits.

Change: in liella_machine_casecmp_ord_avx2, after a stop-free 32-byte head, a bound of 64 or less does the second
32-byte block inline (head's memory-operand constants, one OR-of-offsets page test at <= 4032, else the old loop);
a bound above 64 enters the 64-byte step directly (.Lcord_long), skipping the bound tests.

Counts (keke, Zen 1, bench --compare --counters cbench_locale, instructions Liella / glibc):
strncasecmp_c_64 94.1 -> 76.1 / 68.1 (1.38 -> 1.12; time 1.11 -> 1.03); strcasecmp_c_256 253.2 -> 251.3 / 234.3;
strcasecmp_c_16 54.1 -> 54.1 / 50.1. Jobs: count0 (before), count1 (after). SSE2 entry untouched (not recounted).

Plants (job perf-strncasecmp-64-avx2-plants2, test casecmp_avx2): page, bound, longskip, casesens(a-side fold), noeq CAUGHT;
nofold (b unfolded in the new block) EQUIVALENT by design: a false stop is re-read, found equal, and the rest entry answers.

Open: remaining ~8 instructions: export's locale-view/ascii tests, two separate fold sequences. Not closed; register row 07 updated.
Not to redo: second-block inline path; the loop-entry constant set-up cost was the waste.
Next step: if more is wanted, try one shared fold sequence for both blocks when the bound is in 33..64 (load all four, fold together).

## Continuation PERF-STRNCASECMP-64-AVX2-2 (2026-11-04)
CI PRECHECK RED cause: lab-names. The saturation row 07 named the lab host (keke); now "Zen 1 host" only (commit 28881e41).
No code change: scan.S is as in 0fb43a89. static and clippy (workspace, all targets, all features) clean on the keke role n2.
Gate (affected --base origin/next --run) rc 0: job perf-strncasecmp-64-avx2-2-gate.
Plants (job perf-strncasecmp-64-avx2-2-plants2, test casecmp_avx2, RAM copy): page, bound, longskip (second-block start index 64),
noeq, idx (stop index 64) CAUGHT; casesens (a-side fold dropped in the new block) EQUIVALENT by design (a false stop is re-read
and found equal). WARN: the page and longskip lines name a test whose name contains "error" (my message grep), caught by rc.
Superseded: perf-strncasecmp-64-avx2-2-plants (longskip/idx mutations were result-equivalent: they only moved the start of a re-read).
Next step from the earlier note (one shared fold for both blocks when the bound is 33..64) NOT taken: the fold is 4 vector ops per
input vector either way, so sharing saves only the second mask reduction (about 3 of 76 instructions) and needs both blocks
loaded before the head's stop test, which would cost the 16-byte row. Estimated not worth it; the remaining gap is the export's tests.
