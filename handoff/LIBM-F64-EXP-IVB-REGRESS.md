# LIBM-F64-EXP-IVB-REGRESS handoff: Ivy Bridge `exp` 1.86x (11-05) -> 2.90x (66b8d3ba)

**Verdict: no regression. The 11-05 figure of 1.86x was an outlier.** a6fc34c2 (the 11-05 tree) times 2.93x glibc on
Ivy Bridge today, and 66b8d3ba times 2.95x, measured in one interleaved run on the same host. No code change was made
and nothing was handed to LIBM-F64-EXP-QUICK-WIRE, because no cause exists in the entry, dispatch or envelope.

Branch `task/libm-f64-exp-ivb-regress`, cut from origin/next 5a12901d4. This branch has handoff files only. Role mei:~/w/s4.
Decisions in force: D-10.50 (load-time FMA decision). D-10.59 was reserved for this story and is not used, since nothing was decided.

## Step 1: per-call instruction counts (mei, Zen 1 host; validation/wl_percall.sh, spec `libm-f64-exp-ivb-regress/exp_ivb.c`)
The spec has three cases over 256 operands: exp_wide [-10, 10], exp_small [-0.5, 0.5], and exp_big [-10, 10] with |x| < 0.5 removed.
All three cases gave the same counts, sd 0 (instructions:u per call):

| build | glibc | a6fc34c2 | 66b8d3ba | next 5a12901d4 | jobs |
|---|---|---|---|---|---|
| default (FMA host) | 69 | 187 | 134 | 134 | cnt-a6, cnt-66b |
| `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA` (what Ivy Bridge runs) | 79 | 187 | 188 | 188 | cnt-a6-sse2, cnt-66b-sse2 |

The SSE2 path changed by +1 instruction (0.5%). The FMA path gained 53 instructions per call, but only on FMA hosts. At
a6fc34c2 the FMA build never ran (default = SSE2 count), which matches what LIBM-F64-FAST-KERNELS run 2 found. The two
ranges cost the same, so no path taken only for |x| > 0.5 exists to have regressed.

## Step 3: Ivy Bridge timing (nico, Xeon E3-1220L v2; load 0.04 at start, 0.12 at end; job nico:libm-f64-exp-ivb-regress-ivb)
- Programs: one cbench_libm.c, linked against each revision's cached products (~/wl/products/<sha>/{libm.a,libc.a,liella-crt1.o})
  with xtask's flags. The glibc programs are host-dynamic (the sweep's reference) and host-static.
- bench/c/cbench_libm.c has the same blob (e2d134a3) at a6fc34c2, 66b8d3ba and HEAD, so this harness matches the 11-05 one.
- Run: 2 passes x 15 rounds, interleaved. Each program run gives a best-of-7 ns per call.
- Median ns per call (spread = (max - min) / median):

| row | a6fc34c2 | 66b8d3ba | glibc dyn | a6 ratio | 66b ratio |
|---|---|---|---|---|---|
| exp | 82.81 (sp 0.02) | 83.43 (sp 0.22) | 28.26 | 2.93 | 2.95 |
| exp_small | 82.79 | 83.56 | 28.31 | 2.92 | 2.95 |

Each pass on its own gives the same figures (`libm-f64-exp-ivb-regress/ivb-summary.txt`). This also re-times a6fc34c2
twice, as the brief asked: 2.93 and 2.93. 66b8d3ba is 0.7-0.9% slower than a6fc34c2. That is inside the
+1-instruction change and the 66b runs' spread, and it is not a regression.

Placement check (objdump of our own binaries): `exp` has the same entry structure at both revisions (MXCSR read and
check, kernel call, tail jump). The entry and the kernel are 64-byte aligned in both.

## What the 1.86 could have been (NOT VERIFIED)
nico's ~/work/TIMING-SWEEP-1105/out/rounds.tsv has no cbench_libm rows, so the 11-05 libm numbers came from a run whose
raw rounds were not kept, and the outlier cannot be explained from data. exp_small in the same 11-05 table was 2.92x,
which matches today's figure, while exp has the same instruction count. A single disturbed round (a slow glibc round or
a fast Liella round) would produce this.

## Next step
- LIBM-F64-EXP-QUICK-WIRE: the baselines to beat on Ivy Bridge are 2.93-2.95x in time and 188 vs 79 instructions (SSE2).
  Nothing in the entry or envelope regressed.
- docs/benchmarks.md still shows exp's 11-05 column as 1.86. The next sweep's 11-05 -> now comparison should not flag the row again.

Reproduction: `handoff/libm-f64-exp-ivb-regress/` (exp_ivb.c spec, build.sh, run.sh, summ.py). Logs are on mei
~/logs/libm-f64-exp-ivb-regress-*.log and nico ~/logs/libm-f64-exp-ivb-regress-ivb.log.
Refusals: none. Booking 121 (nico) was cancelled. Nothing is left running.
