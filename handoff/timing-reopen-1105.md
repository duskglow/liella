# TIMING-REOPEN-1105 (COMPLETE, planning; no code change)

State: branch task/timing-reopen-1105 from origin/next 5ac80194; briefs commit 2517aaf4 plus this handoff/report.
Profiled binaries: the TIMING-SWEEP-1105 stage on the Ivy Bridge timing host (next f2157504), so the sweep table and
the profiles are the same code. Booking 34 (nico, Ivy Bridge E3-1220L v2) taken and cancelled.

## Output
handoff/timing-reopen-1105/README.md (summary table) and five briefs: TIMING-IVB-STRSCAN-DISPATCH,
TIMING-IVB-MALLOC-CHAIN, TIMING-IVB-MEMREF-ENTRY (stacks on the first), TIMING-ICONV-TRANSLIT-CPI,
TIMING-NSCACHE-FIRSTCALL. Each: rows with instr / cycles / time ratios, evidence, hypothesis, The change, JOBS with
gate, plants via cargo xtask plants, instruction counts before/after and a booked Ivy Bridge timing check.

## Jobs (nico, under ~/bin/timing)
timing-reopen-1105-prof: perf stat (7 named-event sets x 3 runs, user mode) + cycle samples, 17 rows, both sides
(nscache Liella only). timing-reopen-1105-prof2 and -prof3: memref region counters (CBENCH_COUNTERS=1) good; the
sampled-event half wrote empty files both times (prof2: perf script without ip gives no dso/sym; prof3 still empty,
cause not found) - memref stall attribution NOT VERIFIED, the memref brief's step 1 redoes it.
Raw data and scripts: /home/claude/w/.scratch/TIMING-REOPEN-1105/ (prof.sh, prof2.sh, cmp.py, out/, out2/, cmp1.txt,
strchr.dis) and nico ~/work/TIMING-REOPEN-1105/.

## Findings (short; detail in the briefs)
strchr family: front-end (undelivered slots 22-27x glibc, ~90-97% of the gap); the non-AVX2 entry takes two dispatch
branches and two mask loads per call. strncmp_eq16: load latency more than front-end. malloc: rs-full on the carried
bitmap chain (PERF-MALLOC-IVB-CYCLES; whole-process counts with CBENCH_ROWS are diluted by the mix set-up).
memref: constant +2.5-5 cycles per call. iconv: MITE delivery, flags-merge uops, store-forward blocks, rs-full.
nscache: cold-start dominated; rows not separable by CBENCH_ROWS.

## Not done / open
N100 rows not profiled (listed in README for a later round). glibc annotated with symbol shares only (no disassembly:
clean-room rule overrides the brief's "annotate on glibc"). RULING: TIMING-IVB-STRSCAN-DISPATCH option (c) and
TIMING-IVB-MALLOC-CHAIN step 2 need D-12 rulings (recommend: try the cheap layout fix / measure behind a branch first).
Next step: coordinator submits the five briefs (strscan first; memref stacks on it).
