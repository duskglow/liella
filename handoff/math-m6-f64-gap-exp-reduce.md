# MATH-M6-F64-GAP-EXP-REDUCE(-2) (register 13.5 #2): code done and planted; counts PENDING

State: branch task/math-m6-f64-gap-exp-reduce-2, code tree 33c75b77 (run 1's 80b39d15 + its handoff e7c5b16c + merge of
origin/next d5b7003d, clean, crates/math untouched by next). Later commits are docs/handoff only. Role keke:~/w/n3, synced
at 33c75b77 with --base origin/next (assembly-2 is in next, so the gate selects the Direct tier, not FULL).

## Decisions in force
- D-10.13 now states the reduction: `reduce_ln2` (C0 36 bits, C1 35, C2 34; k*Ci exact for |k| < 2^17), `reduce_exp`
  (x - k*C0 exact by Sterbenz, Fast2Sum for -k*C1, low part s.lo - k*C2, renormalizing Fast2Sum, no x.lo add),
  `reduce_pow` (z.lo joins the low part), `ln2_times` for exp2 (Dekker, ln2 pre-split LN2A/LN2B). Bounds: exp 2^-106.5,
  pow 2^-96.9, exp2 2^-104*|r|, all inside the kernel's 2^-90; tests check 2^-106 / 2^-96 / 2^-104*|r| against mp.

## Results (keke)
- Workspace clippy -D warnings rc 0, `cargo xtask static` rc 0 on n3 at 33c75b77 (foreground).
- Plants: job math-m6-f64-gap-exp-reduce-2-plants rc 0, RAM copy /dev/shm/claude/math-m6-f64-gap-exp-reduce-2/plants-1:
  baseline ok, 10/10 CAUGHT with the expected message (drop-kc2, drop-slo, c0-one-ulp, exp-r-2m105, pow-drop-zlo,
  pow-r-2m95, exp2-drop-bb, exp2-drop-l1, exp2-l1-2m103, ln2b-one-ulp); each bound one bit beyond the test's tolerance.
  No WARN lines.
- Gate: math-m6-f64-gap-exp-reduce-2-gate (Direct tier, 20 changed files), RUNNING at hand-back.
- Sweep: math-m6-f64-gap-exp-reduce-2-sweep (SLOT_SIZE=M, SLOT_MAX_MIN=300, math_double_fast_entry_sweep on a RAM copy of
  n3), RUNNING. Run 1's sweep (old role n4, older head) had 1.0e9 operands, 0 mismatches, when I stopped it (rc 124).
- Counts: math-m6-f64-gap-exp-reduce-2-percall2 (~/work/math-m6-f64-gap-exp-reduce-2/counts.sh: full cbench_libm,
  --compare --counters, default and --tunables glibc.cpu.hwcaps=-AVX2, base = run 1's saved pre-story fast64.rs/dd.rs vs
  after), QUEUED at hand-back. percall (run 1) and -2-percall failed on the invocation: bench names are bench PROGRAMS
  (positional) and `--rows` does not work with --compare; only whole programs or `--only`.

## Not done / next step
1. Read percall2: rows exp, exp2, log, log2, log10, pow, Liella base/after and glibc, both levels. Write register 13.5
   row #2 "DONE" with the measured saving and add the numbers to D-10.13 ("Measured ..."). If any row is slower, report
   the trade-off.
2. Confirm gate and sweep rc 0.
INFORMATIONAL: docs/saturation-register.md has several sections numbered "## 13." (merged from parallel stories);
renumbering is the coordinator's call. Not to redo: proofs, split constants, tests, plants script.
Plan rows: D-10.13 (plan/07-decisions/s10-mathematics.md); register 13.5.
