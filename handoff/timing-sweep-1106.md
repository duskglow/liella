# TIMING-SWEEP-1106 (table written; TIMING-SWEEP-1106-2)

State: branch task/timing-sweep-1106-2 (continues task/timing-sweep-1106), base origin/next 4d454f06. No code
change. Result: results/timing-sweep-1106-table.md (table, spread, lists a/b/c, 1106B re-time verdicts, wordexp apart).

## Runs (products of next 4d454f06, staged once on shiki sh1, job timing-sweep-1106-stage rc 0)
- Ivy Bridge: kotori:timing-sweep-1106-kotori, 07:31-09:18Z, rc 1. The run is complete (all tsv written). rc 1 =
  xtask bench's per-host budget check (calibration main 5236e77) flagged 11 rows (list in the results header). 7 are
  slower in time than glibc and unchanged since 1105 (memcmpeq/bcmp eq/tail 64/128, memchr_16_mix); 4 are faster
  than glibc and only drifted from the calibration. Not a run failure, and I did not rerun (no reruns to pass). The
  1105 Ivy Bridge run also ended rc 1 on that check.
- N100: nozomi:timing-sweep-1106-nozomi-2, 07:31-08:38Z, rc 0 (all 431 budgets met).
- Superseded: nozomi:timing-sweep-1106-nozomi rc 1 in 0.24 s (run.sh passed `--staged --compare`, which xtask bench
  rejects; the coordinator fixed run.sh to `--no-build --counters --only`); nico booking 75 cancelled by the coordinator.
- Bookings 76 (nozomi) and 77 (kotori): my `ops timing-cancel 76` / `ops timing-cancel 77` was REFUSED by the
  permission layer. The coordinator must cancel them if they are still active.

## Findings (details in the results file)
- Ivy Bridge 238 faster / 97 parity / 69 slower (1105: 230/95/79); N100 307/73/24 (1105: 297/78/29). Spread
  median 0.010 / 0.015, p90 ~0.10. Drift now/1105 median 1.000 on both.
- (a) Regressions: 4. Real: cbench_memref/memmove_4k on N100 (0.965 -> 1.080, spread 0.008), likely
  parity-copy-avx2-3 (ff125cce, AVX2 copy entry; instructions 1.034 -> 1.000). Marginal: first_bindtextdomain_gettext
  on Ivy Bridge (1.053), likely perf-gettext-language-steady-5. Noise (no landing on the path, instructions unchanged,
  high spread): openpty_close on Ivy Bridge, mkstemp_unlink_close on N100.
- (b) Reopen: 39 (mostly the 1105 Ivy Bridge malloc_free / memcmp / memmove / memset mid-size rows, unchanged; strchr
  family improved but still slower). (c) Improved: 19.

## Next step
Coordinator: open a story for memmove_4k on N100 (time-only regression after parity-copy-avx2-3); re-time
first_bindtextdomain_gettext on Ivy Bridge before opening one; close stories whose rows are in list (c). Process:
for future sweeps, have run.sh tell "bench completed, budget check flagged rows" (record it, exit 0) apart from a
real run failure, so a finished sweep does not read as red (RULING NEEDED in the report).

## Not to redo
The stage, both timing runs, the analyzer run. Analyzer: handoff/timing-sweep-1106/analyze.py <dir> <1105 table>
<1106B table> with <dir>/{ivb,n100}/{counters,rounds}.tsv.
