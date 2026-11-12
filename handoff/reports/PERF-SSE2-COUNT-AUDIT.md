# PERF-SSE2-COUNT-AUDIT: SSE2 counts that were really default-level counts

Status: complete after run 2 (PERF-SSE2-COUNT-AUDIT-2). No record's conclusion changes. The SR-25 recount is below.
The summary table is at the end. This is the deliverable the brief calls `reports/PERF-SSE2-COUNT-AUDIT.md`; the
repository keeps story reports under handoff/reports/.

Question: which records and reports give an SSE2 (non-FMA or non-AVX2) instruction count that came from a run where
`GLIBC_TUNABLES=...` was set in the environment of `cargo xtask bench`? BENCH-FIXED-ENV (landed 2026-10-21 20:58Z)
cleared the bench environment. Until D-X.39 (XTASK-BENCH-TUNABLES-AND-MATHTOOLS, landed 2026-11-07 16:08Z, #4099),
such a run silently measured the default level. `--tunables` (BENCH-TUNABLES, landed 2026-10-23 19:29Z) was the correct route throughout that window.

## Method

1. Bench logs. Since BENCH-TUNABLES, every bench run prints `tunables: GLIBC_TUNABLES=<value>|(unset) (both sides)`.
   I scanned every `~/logs/*.log` on keke, mei, shiki and kanon (869 logs carry that line). A run labelled SSE2,
   noavx2, -FMA or -AVX2 whose tunables line says `(unset)` measured the default level.
2. Records. I scanned all `records/decisions` and `records/saturation` bodies for SSE2, -FMA, -AVX2 and non-AVX2 figures
   (33 records), and all `handoff/reports/*.json` and handoff notes for `GLIBC_TUNABLES`. Each in-window figure was
   traced to its job log, or to its route: `wl_percall` (which carries the tunables key) or a direct-run harness
   both pass the environment. An Ivy Bridge timing is the SSE2 class by hardware.
3. Recount (job `perf-sse2-audit-counts2`, shiki, Xeon E5-2697 v3 Haswell-EP, tree = origin/next at this branch's
   base). Command: `cargo xtask bench --compare --counters --runs 3 cbench_libm --rows 'exp*,log*,asinh*,acosh*,atanh*'`,
   first with no tunables, then with `--tunables glibc.cpu.hwcaps=-FMA`. The log shows both tunables lines, `(unset)` and
   `glibc.cpu.hwcaps=-FMA`. Host caveat: the brief asks for the same host as each recorded figure. D-10.66 was counted
   on keke (Zen 1), but this story's role is on shiki. Instruction counts at a forced level follow the code path, not
   the core. glibc's -FMA figures here match D-10.66's keke figures within 0.2 per call, which supports comparing
   across the two hosts.

## Findings: records

No decision or saturation record carries an SSE2 figure that came from a dropped-environment run.

| record | SSE2 figure as recorded | how it was taken | verdict |
|---|---|---|---|
| D-10.64 (exp2/exp10, later expm1) | none: "SSE2 build counts NOT VERIFIED" (handoff) | env run, dropped | gap, filled below |
| D-10.65 (asinh/acosh/atanh) | none: "NOT VERIFIED" (report: log prints unset) | env run, dropped | gap, filled below |
| D-10.66 (log2/log10/log1p) | 144.4 / 143.2 / 145.5 (glibc 83.2 / 118.4 / 103.1) | `--tunables`: keke `libm-f64-log-family-wire-2-counts2.log` prints `glibc.cpu.hwcaps=-FMA (both sides)` | correct |
| D-10.54 (exp quick) | count loop, own harness | direct run, env reaches it | correct route |
| D-10.61 (trig quick) | SSE2 after sin 173.1 ... | trig-quick-2/-3 count logs carry a set tunables line | correct |
| D-10.62 (pow quick) | 247.7 / 191.5 | `--tunables` named in the record | correct |
| D-10.77, D-10.90 | SSE2 104.2 -> 102.2; erf rows | `wl_percall` | correct route |
| D-3.17 (realloc) | non-AVX2 288.8 -> 245.7 | levels differ from AVX2 figures; kanon `perf-realloc-usable-base3.log` prints `-AVX2 (both sides)` | correct |
| D-4.57 (memchr short, SSE2 level) | memchr_16 44.3 -> 29.3 ... | `timing-cov-memchr-short-nonavx2-4-counts.log`: 2 set lines | correct |
| D-4.61/62, SR-20/21/22/45/47/48/49, D-4.53/SR-43, D-3.18/SR-57 | timings at hwcaps=-AVX2 or Ivy Bridge | Ivy Bridge is SSE2 by hardware; Alder Lake-N -AVX2 ratios move where only the SSE2 body changed (memmove_144 0.851) | consistent |
| SR-25 (`tried`: SSE2-GAP, instr 43.4 to 45.4) | 43.4 -> 45.4 (a dropped candidate's delta) | measured 2026-10-11 (bench/perf-ledger.tsv rows SSE2-GAP-2 and the `rejected` SSE2-GAP row), before BENCH-FIXED-ENV, so a caller's environment still reached the bench; the "2026-10-23" in the row dates its `now` column | outside the window; consistent with the recount (run 2, below) |
| D-4.14, D-4.25 | -AVX2 figures, 2026-10-09/10 | before BENCH-FIXED-ENV | outside the window |

## Findings: reports and handoffs (dropped runs, none cited as a true SSE2 figure)

Each author flagged these as NOT VERIFIED or NOT MEASURED. The log evidence confirms that:
LIBM-F64-EXP-QUICK-WIRE-2 (mei `libm-f64-exp-quick-wire-2-counts.log`, 3x unset), LIBM-F64-LOG-QUICK-2 (mei
`...-counts4.log`, 2x unset), LIBM-F64-INVHYP-QUICK (mei `...-counts2.log`), LIBM-F64-EXP-FAMILY-QUICK(-2) (not run),
TIMING-COV-COMPARE-HEAD-SSE2 (mei, `noavx2` sections all unset; report says "-2 per call by code reading"),
TIMING-COV-STRSTR-PAIR(-2) (-AVX2 unchanged), CLASSED-SLOT-INFRA-3 ("+65k kernel every class", corrected by -4),
and NULLFOLD-MEMSET (a `liella.compat` tunable, page-1 counts not taken).
PERF-SSE2-REMEASURE-3's open item, the "15.1 realloc row tunables line NOT CHECKED", is now checked: kanon `perf-realloc-usable-base3.log` prints
`glibc.cpu.hwcaps=-AVX2 (both sides)`.

## Recount: SSE2 build (`--tunables glibc.cpu.hwcaps=-FMA`), next, instructions per call

| row | record | recorded SSE2 | true SSE2 Liella / glibc | ratio | default Liella / glibc | conclusion holds |
|---|---|---|---|---|---|---|
| exp2 | D-10.64 | none | 100.4 / 51.1 | 1.97 | 86.2 / 51.1 | yes |
| exp10 | D-10.64 | none | 120.8 / 56.1 | 2.15 | 101.9 / 56.1 | yes |
| expm1 | D-10.64 (-2) | none | 149.7 / 104.3 | 1.44 | 125.3 / 86.4 | yes |
| asinh | D-10.65 | none | 197.5 / 115.1 | 1.72 | 133.6 / 99.4 | yes |
| acosh | D-10.65 | none | 184.6 / 98.8 | 1.87 | 122.6 / 83.6 | yes |
| atanh | D-10.65 | none | 191.3 / 130.7 | 1.46 | 140.2 / 112.7 | yes |
| log2 | D-10.66 | 144.4 / 83.2 | 140.2 / 83.1 | 1.69 | 107.2 / 63.1 | yes (recorded figure was a real SSE2 count; -4.2 since, later rows) |
| log10 | D-10.66 | 143.2 / 118.4 | 139.0 / 118.4 | 1.17 | 107.0 / 103.0 | yes |
| log1p | D-10.66 | 145.5 / 103.1 | 141.2 / 102.9 | 1.37 | 109.2 / 83.5 | yes |
| exp | D-10.54 (target 95 SSE2) | (count loop) | 99.3 / 77.1 | 1.29 | 85.1 / 67.1 | target not met in the bench (99.3 > 95) |
| log | D-10.60 (LOG-QUICK-2) | NOT VERIFIED | 116.2 / 78.1 | 1.49 | 89.1 / 63.1 | n/a (no SSE2 claim) |

## Recount: SR-25 rows (run 2, PERF-SSE2-COUNT-AUDIT-2)

Job `perf-sse2-audit-2-sr25` on keke (AMD EPYC 7551, Zen 1, the class of SR-25's `now` figures), tree = origin/next at
this branch's base. The rows are in `cbench_memref`, not `cbench_opt`, which is why run 1's job failed. The command was
`cargo xtask bench --compare --counters --runs 3 cbench_memref --rows strlen_64,strnlen_64,rawmemchr_64`, run three
times: with no tunables, with `--tunables glibc.cpu.hwcaps=-AVX2` and with `--tunables glibc.cpu.hwcaps=-FMA`. The log
prints the matching `tunables:` line for each pass: `(unset)`, `-AVX2 (both sides)` and `-FMA (both sides)`.

| row | SR-25 `now` (default, Zen 1, 10-23) | default Liella / glibc | SSE2 (`-AVX2`) Liella / glibc | `-FMA` Liella / glibc |
|---|---|---|---|---|
| strlen_64 | 38.4 / 33.4 (above) | 32.0 / 33.6 (0.95) | 46.3 / 68.7 (0.67) | 32.0 / 33.7 |
| strnlen_64 | 55.4 / 48.4 (above) | 41.4 / 48.5 (0.85) | 64.9 / 74.8 (0.87) | 41.4 / 48.5 |
| rawmemchr_64 | 35.5 / 33.4 (above) | 31.4 / 34.9 (0.90) | 45.4 / 49.5 (0.92) | 31.4 / 33.5 |

* For the string kernels, `-FMA` gives the default path on both sides, so the SSE2 level of these rows is `-AVX2`.
  The coordinator's "-FMA" applies to the libm rows only.
* The SR-25 figure 43.4 -> 45.4 measured a candidate (a 48-bit memchr head mask and a split of rawmemchr's first
  group) that was dropped. That code is not in next, so the delta itself cannot be recounted. What can be checked
  is whether 43.4 could have been a default-level count. Probably not, but this is inferred, not proven: no recorded
  default-level Zen 1 figure for these rows or for memchr_64 from SHORT-2 (2026-10-09) on is near 43.4
  (perf-ledger: strlen_64 38.5, memchr_64 38.4; SR-25: 38.4, 55.4, 35.5), while the SSE2-level rawmemchr_64 is 45.4 today.
  Which row the 43.4 belongs to is not in the repository (the source note is on a historic branch this brief does not name). The figure is
  also outside the dropped-environment window (measured 10-11). Verdict: the conclusion holds ("net loss, dropped;
  do not redo that head").
* SR-25's `now` column is stale. At the default level all three rows are below glibc in instructions today
  (0.85-0.95), not "ABOVE glibc". At the SSE2 level they are also below glibc (0.67-0.92). This audit edits no
  record. Refreshing SR-25 is a records task for whoever next owns the string-scan rows.
* Timing on this build host, for information only (not a booked timing host): Liella's default path takes 1.46x
  glibc's time in all three rows at fewer instructions, and Liella's SSE2 strlen_64 (8.04 ns) is faster than its
  default AVX2 path (9.54 ns). This matches docs/saturation section 13's PERF-STRLEN-NONAVX2 note
  (fewer instructions, slower) and is not part of this audit.

## Proposals

No decision's conclusion changes, so the brief's step 3 has no redo to propose. Two follow-ups are worth a story:

* **D-10.65 SSE2 path.** asinh, acosh and atanh cost 51 to 64 instructions more in the SSE2 build than in the FMA build.
  exp, log and their families cost 14 to 33 more. The likely cause is the exact products in the pair-input log and the Newton-corrected root,
  which become Dekker products without FMA (NOT VERIFIED). Proposal: profile one SSE2 call with
  `wl_percall --tunables glibc.cpu.hwcaps=-FMA` and look for Dekker products the bound does not need. D-10.64 dropped one
  such product at a fallback cost of about 1 instruction. Timing on Ivy Bridge decides.
* **D-10.64 SSE2 trade.** The record weighs about 1 instruction per call of extra fallback against about 6 for a Dekker
  product. That was never measured. Proposal: one A/B count of exp2/exp10 at -FMA with the Dekker product, which needs a
  scratch build, so it belongs to a code story.
* **SR-25 SSE2-GAP.** Done in run 2 (section above): the figure stands, and only the `now` column needs a refresh.

Both code proposals are written as briefs for the coordinator:
handoff/PERF-SSE2-COUNT-AUDIT-2/LIBM-F64-INVHYP-SSE2-PROFILE.md (D-10.65, #5057) and
handoff/PERF-SSE2-COUNT-AUDIT-2/LIBM-F64-EXP2-SSE2-DEKKER-AB.md (D-10.64).

## Summary table (all audited SSE2 figures)

| row | record | recorded SSE2 | true SSE2 (Liella / glibc) | conclusion holds |
|---|---|---|---|---|
| exp2, exp10, expm1 | D-10.64 | none | 100.4 / 51.1, 120.8 / 56.1, 149.7 / 104.3 | yes |
| asinh, acosh, atanh | D-10.65 | none | 197.5 / 115.1, 184.6 / 98.8, 191.3 / 130.7 | yes (SSE2 gap: follow-up brief) |
| log2, log10, log1p | D-10.66 | 144.4, 143.2, 145.5 | 140.2, 139.0, 141.2 (later rows: -4.2) | yes |
| exp | D-10.54 | count loop | 99.3 / 77.1 | yes (bench above the 95 target) |
| strlen_64, strnlen_64, rawmemchr_64 | SR-25 | 43.4 -> 45.4 (candidate delta, 10-11) | 46.3 / 68.7, 64.9 / 74.8, 45.4 / 49.5 (`-AVX2`) | yes (outside the window; `now` column stale) |

Pending record: `records/pending/PERF-SSE2-COUNT-AUDIT-1.toml` (D-NEW, series D-10, amends D-10.64 and D-10.65) cites
the true SSE2 counts. No code was changed. No existing record was edited.
