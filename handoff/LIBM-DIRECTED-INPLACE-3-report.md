# LIBM-DIRECTED-INPLACE-3 report

Branch task/libm-directed-inplace-3, role keke:~/w/r5 (Zen 1).

## Commits
- 25b11ee02 Merge origin/next (the coordinator's note: held until the data-cluster fix landed, then merge
  next and re-gate; LAYOUT-DATA-CLUSTERS-4C landed and the story was unheld).
- a8e17945f Choose the directed exp's direction by instance, not by data (quick.rs `exp_directed::<UP>`,
  hot/exp_log.rs branch on the mode, D-10.94 amended with the finding, plants file).
- then this report, the handoff and handoff/reports/LIBM-DIRECTED-INPLACE-3.json.

## Step 0 (inherited head merged with next)
lint1: `cargo xtask math-gen` (s10_math_gen.rs unchanged, md5 equal to the worktree), clippy, static: rc 0.

## Profile before design (the coordinator's #4648 request)
- perf stat, cbench_libm rows on the unbooked Zen 1 host (meas1): Liella exp_upward IPC 0.90, 92% of
  cycles backend-stalled; Liella exp (nearest) IPC 1.99; glibc 2.5-2.8. perf record of exp_upward: the
  samples spread evenly over the kernel's FP instructions (each ~5%), and half of the entry's own
  samples on the load right after `stmxcsr`: a latency-bound chain, not one slow instruction.
- Hardware probe rcprobe.c (work dir; no libm): add/mul/FMA throughput and latency are equal in all four
  rounding directions on Zen 1 (0.515 ticks per add, 3.09 add latency, 0.646 per FMA): no directed-mode
  penalty. `stmxcsr` plus a use of its value: ~20.5 TSC ticks per iteration in every mode.
- Disassembly of the directed path: one stmxcsr, then `sete %cl; movsd (table,%rcx,8),%xmm1` (`h`
  selected from the MXCSR value as data) feeding the reduction; no other MXCSR access, no float-to-int
  conversion. Named cause: **the data dependency of the whole evaluation on the MXCSR read**, which waits
  for the caller's earlier FP work, so successive calls could not overlap.
- Fix: the direction is a const generic picked by a branch. After (prof3): IPC 2.32, backend stall 64.5%
  (nearest 2.21 / 61.8%). The proof does not change (`h` takes the same values).

## Results (final code)
- t4: timing 3 alternations (unbooked, load ~20; ratios only): exp_upward Liella 22.12-22.59 ns vs glibc
  18.05-18.53 (before: 68.30 vs 14.28, 4.8x); exp 19.26-19.44 vs 18.06-18.32. exp_quick 10 passed,
  s10_math_symbols 179 passed, directed_bounds 1 passed.
- plants: 3 of 3 CAUGHT with msg, no WARN lines (handoff/LIBM-DIRECTED-INPLACE-3-plants.txt).
- gate2: `affected --base origin/next --run` on the final head (see the hand-back for its verdict).
- Instruction counts: not re-measured; the change replaces a table load by a branch and duplicates the
  cold directed kernel (one instance per direction class), so instructions per call stay as run -2 measured
  (exp_upward 107.5 FMA / 125.1 SSE2).
- NOT DONE: booked timing on Ivy Bridge and N100; log/sin/pow (next story).

## INFORMATIONAL
- Every Liella libm entry reads MXCSR once per call (`stmxcsr`), ~20 TSC ticks reciprocal throughput on
  Zen 1 per the probe; Liella's nearest exp is 19.3 ns vs glibc 18.2 here under load (10.4 unloaded in run
  -2's numbers vs 19.0). A cheaper environment check is a separate design question (recommend a
  measurement story on Ivy Bridge and Zen 1 before any change).
- gate1 (merged head before the fix) was stopped by me when the fix superseded it.

## IMPROVEMENT
Profiling first paid off: a 30-line hardware probe plus one disassembly named the stall in ~15 calls.
Budget note: `cargo xtask bench --build-only` requires `--compare` (one job lost to it).
