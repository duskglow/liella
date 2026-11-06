# COLDSTART-PLAN-1106: plan for the cold-start rows that are slower in time

State: branch task/coldstart-plan-1106 (from origin/next 730f416b). Planning only, no code change. Result: INCOMPLETE: plan written (6 code briefs); the N100 re-time of the rows on next HEAD (step 1) was still running at hand-back.

## Method
- Probe: a scratch copy of the bench's cold child (bench/c/cbench_opt.c included unchanged, a new main): one letter per call in the bench's order (S setlocale, C strcoll, I iconv, L localtime_r, F strftime, M mktime, R regex, N fnmatch, B bindtextdomain+dgettext, G dgettext absent, P fork+_exit+waitpid; wordexp left out). Built from next 730f416b, Liella static (the bench's link line) and glibc dynamic.
- Row cost = mean of 30 fresh processes with the call minus without it (perf stat aggregate: instructions:u, instructions:k, page-faults), Broadwell-EP build host. Liella: perf record by symbol over 200 runs (instructions:u, page-faults). glibc: aggregate counters only.
- Re-time: cargo xtask bench --stage (cbench_opt, cbench_locale, cbench_process) on the build host, --staged --counters on the N100 timing host through its timing queue (label CSP1106-n100), plus the probe with 50 runs. Not collected yet.

## Findings (Broadwell-EP counts; Liella vs glibc per call)
| Row (N100 time in sweep 1105) | Liella user instr / kernel instr / faults | glibc | Cost named | Brief |
|---|---|---|---|---|
| first_strcoll (19.5x) | 11.8k / 36k / 3 | 0.3k / ~0 / 0 | full ASCII-slot fill at first general compare (Collate::fill) | COLDSTART-STRCOLL-FIRST |
| first_setlocale_en_US (2.22x) | 63.6k / 362k / 38 | 23k / 134k / 12 | first heap use faults, eager ctype derivation (CtypeFile::parse, ascii_case_runs), archive lookup | COLDSTART-SETLOCALE-FIRST |
| first_localtime_r_new_york (2.12x) | 37k / 108k / 16 | 12k / 38k / 4 | TZif read + parse, layout/allocation faults (to split) | COLDSTART-TZ-FIRST |
| second_bindtextdomain_gettext (7.7x) | 968 / +25k / 2 | 694 / ~0 / 0 | rebind to the same dir re-reads / reallocates (heap refill + memset faults) | COLDSTART-BINDTEXTDOMAIN-REBIND |
| fork_exit_wait (1.294x, instr 1.081) | 6.8k / 295k / 27 per fork | 0.3k / 255k / 12 | child_reset (58% of user samples) runs every subsystem's after-fork hook unconditionally; COW faults | COLDSTART-FORK-CHILD-RESET |
| second_setlocale_en_US (1.43x), second_mktime_new_york (1.28x) | 177 / - / 0; 1064 / - / 0 | 174; 1266 | instructions equal or fewer: latency or noise; re-time decides | COLDSTART-SECOND-LATENCY |

Rule check: none of these is start-up work; all are first-use (lazy) paths whose cost the failure-path-cost rule still counts. strcoll / setlocale / TZ tables derive from host data (archive, TZif), so build-time tables do not apply; the briefs ask for per-entry on-demand derivation or less work.

## Not briefed
- strcasecmp_c_16 (N100 1.136, instr 1.080; Ivy Bridge parity): steady-state, not cold; not measured here (the probe's K letter was not in the counted set). Decide from the N100 re-time; if still above 1.05x, brief it with PERF-STRNCASECMP-64-AVX2 as the nearest story.
- Deferred (OWNER-DEFERRED): first_wordexp, second_wordexp, wordexp_* rows.
- nscache first_/second_ rows: NSCACHE-SECOND-LOOKUP-2.

## Next step
Collect the N100 job (timing host, label CSP1106-n100: ~/work/COLDSTART-PLAN-1106/bench.log and counts-n100.txt; DONE file when finished). Drop any row at or below 1.05x and its brief; then dispatch the briefs (STRCOLL-FIRST and BINDTEXTDOMAIN-REBIND first: largest ratio, smallest change).

## Do not redo
The Broadwell-EP counts above (probe source and scripts: coordinator scratch work/COLDSTART-PLAN-1106/, not in the repo). Each brief's first step re-measures its row undiluted.
