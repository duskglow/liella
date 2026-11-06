# TIMING-N100-COPY-MID: handoff (one page)

State: branch `task/timing-n100-copy-mid` from origin/next 730f416b. No code change. Commits: the saturation register row
(docs/saturation/03-memory-and-string-kernels.md, NEGATIVE), this handoff and handoff/reports/TIMING-N100-COPY-MID.json.
Verdict: COMPLETE (target not reached; no same-instruction-count shape helps on Gracemont; the gap needs a ruling).

## Step 0 (brief): re-timed on next HEAD
N100 (nozomi, Alder Lake-N), staged bench of next 730f416b (built on keke role n2, `cargo xtask bench --stage`), job
nozomi:TIMING-N100-COPY-MID-n100 rc 0. Ratios Liella/glibc (time / cycles / instr): memcpy_128 0.752 / 0.753 / 0.939;
memcpy_144 1.077 / 1.092 / 1.003 (14.4 vs 13.2 cycles); memcpy_256 1.077 / 1.069 / 0.999; memmove_144 0.859;
memmove_256 0.947. Still above 1.05: PARITY-COPY-AVX2-3 did not touch this class (it changed the >= 257 kernel).

## Profile (scratch microbench, not in the repo)
One binary (built on keke, run pinned under `~/bin/timing` on nozomi): the 129-256 entry of fold.S copied as scratch asm,
variants of it, and glibc's `memcpy` (dynamic) on the same buffers; per call = (cycles at 2M calls - at 0) / 2M, min of 5.
Sources: keke:~/work/TIMING-N100-COPY-MID/mb (kern.S, kern2.S, mb.c, run.sh, td.sh); logs nozomi:~/logs/TIMING-N100-COPY-MID-n100{,-diag,-r2,-r3}.log.
- Current: 13.00 cycles, glibc 12.26, at every src/dst placement tried (malloc layout, 0:256, 0:320, 16:336, 0:2048,
  0:4096, 16:4144, 32:4096); 0:4104 (4 KiB alias) 17.0 for both. Not a bench layout artifact.
- Same count, no gain: stores reversed, head/tail interleaved, tail stored first, tail loaded first, vmovups, 129 class
  as fall-through (e1, 13.1), flag test first, NULL tests after the flag test (13.1), vzeroall. All 13.00 or worse.
- Diagnostic (not shippable): body only 11.0; no vzeroupper 12.67; VEX.128 body 12.67-13.78 (+15 instr); no flag test
  13.00 (-2 instr); no NULL tests 12.00 (-4 instr). 65-128 class (n=128) 9.00 vs glibc 11.00 in every layout.
- Topdown (n=144, slots per call): retiring 56.3 vs 54.3 = instructions + 1 per 256-bit memory op (two 128-bit halves
  each, as the brief suspected); backend mem_scheduler 5.0 vs 1.5 in the microbench (the bench profile put the same ~1
  cycle in non_mem_scheduler). Allocation-width bound: only fewer uops helps, and the uops to remove are the NULL tests.

## RULING NEEDED
The two explicit NULL tests on the 129-256 path (option B, owner 2026-10-08) are the time gap on Gracemont: without them
12.00 vs glibc 12.26. Recommend keeping option B (EFAULT abort on NULL is the S6 contract; 1 cycle in 13 on one core
class) and closing the row as design-bound. Alternative: a 129-256 entry that tests NULL with fewer uops, if the owner
accepts +1 instruction on the 65-128 class (that class is 0.75 of glibc's time on the N100).

## Jobs
keke:TIMING-N100-COPY-MID-stage-base (rc 0), keke:TIMING-N100-COPY-MID-count-before (rc 0; Zen 1: memcpy_144/256 38.4 vs
36.4 instr), nozomi:TIMING-N100-COPY-MID-n100, -n100-diag, -n100-r2, -n100-r3 (all rc 0), keke:TIMING-N100-COPY-MID-gate
(docs-only head). No count-after, plants or Ivy Bridge timing: no code changed. Booking 50 cancelled (never needed: the
`timing` queue served the runs).

## Not to redo
Any store/load order of the 8 ymm blocks; the branch layouts e1/e2/e3; vmovups; vzeroall; 128-bit ops.

## Next step
The ruling above. If the owner wants the cycle: design a 129-256 NULL test with fewer uops and measure the 65-128 cost.
Slip: one python call in this run was driven by a heredoc (scratch microbench edit), against the no-heredoc rule.
