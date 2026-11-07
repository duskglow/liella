# TIMING-SWEEP-1106E (table written)

State: branch task/timing-sweep-1106e, base next 34c98f61 (products built from it). No code change. Result:
results/timing-sweep-1106e-table.md (summary, spread, lists a/b/c, new rows, full table, wordexp apart).

## Runs (products of next 34c98f61, staged once on shiki sh1)
- Stage: shiki:timing-sweep-1106e-stage, rc 0, ~/work/TIMING-SWEEP-1106E/stage.tar (sha256 6eba4f71...), copied
  whole to both timing hosts; 76 products matched the manifest.
- Ivy Bridge: kotori:timing-sweep-1106e-kotori, 18:40-20:23Z, rc 1 = the bench's per-host budget check (4 rows
  flagged, 11 in 1106; listed in the results). Run complete, all tsv written. Same rc-1 cause as 1106 and 1105.
- N100: nozomi:timing-sweep-1106e-nozomi, 18:40-19:48Z, rc 0 (433 budgets met).
- Bookings 95 (kotori) and 96 (nozomi) cancelled by me when each run ended. kotori was picked over nico: both idle,
  kotori ran 1106 (same host, comparable).
- Command: handoff/timing-sweep-1106e/run.sh (1106's command plus `--staged`, which checks the tree is a stage).
  Raw data on the hosts: ~/work/TIMING-SWEEP-1106E/out/{counters,rounds}.tsv, bench.log.
- `sudo: a password is required` in bench.log on N100 also appears in the 1106 log (the bench's own optional step);
  not run by me.

## Findings
- Ivy Bridge 252 faster / 99 parity / 56 slower (1106: 238/97/69); N100 313/74/20 (1106: 307/74/23).
  Spread median 0.009 / 0.012; drift now/1106 median 1.000 on both.
- (a) REAL REGRESSION: cbench_opt/first_mktime_new_york 0.91 -> 4.89 (Ivy Bridge), 0.99 -> 4.80 (N100), spread
  <= 0.06; likely coldstart-second-latency-4 (a86c9677, crates/legacy/src/time_compat.rs + recheck.rs, the only
  landing on that path). cbench_opt/first_snprintf 1.16 -> 1.45 (Ivy Bridge), 0.99 -> 1.26 (N100); likely
  coldstart-fork-child-reset-2 (bf079683, stdio_compat.rs) or a86c9677. gettext_language_steady Ivy Bridge 1.051:
  marginal. strncasecmp_c_64 N100: identical value, rounding only.
- (b) 36 (1106: 39). (c) 21 improved: the 11-06 memcmp/__memcmpeq/bcmp, memchr_16_mix, memcpy mid (N100),
  memmove_4k N100 (the 1106 regression is fixed), regex first-byte, fork_exit_wait.
- New memmove_overlap8_4k / overlap64_4k: parity or faster on both; their instruction ratios (10-26) mislead.

## Next step
Coordinator: open a story for first_mktime_new_york (+first_snprintf) first-call regression, both classes, starting
from a86c9677 / bf079683; close stories whose rows are in list (c).

## Not to redo
The stage, both timing runs, the analyzer run. Analyzer: python3 -I handoff/timing-sweep-1106e/analyze.py <dir>
results/timing-sweep-1106-table.md with <dir>/{ivb,n100}/{counters,rounds}.tsv. Landings list in
handoff/timing-sweep-1106e/landings-4d454f06..34c98f61.txt.
