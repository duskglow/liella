# MATH-M6-F64-GAP-ASSEMBLY (story A of 5, register 13.5 #1 and #6): COMPLETE (FULL gate running, run 2)

State: branch task/math-m6-f64-gap-assembly-2 (contains run 1's task/math-m6-f64-gap-assembly; origin/next merged at
0a616056, the register conflict kept both 12.12 and 13). Stacks on task/math-m6-f64-gap-breakdown (not landed).
Role shiki:~/w/sh2, synced at 0a616056 with --base origin/next. Commits after 0a616056 are docs/handoff only.

Decisions in force: D-10.13 (the assembly paragraph, moved to the end of the row: run 1 had inserted it inside
"`exp(y·log |x|)`"; plus the measured saving). Register 13.5 rows 1 and 6 marked DONE with the counts.

Code (run 1, unchanged in run 2): crates/math/src/fast64.rs. exp_kernel assembles t + t*p from one exact product and
two Fast2Sums; log_assemble (42-bit LN2H 0x3fe62e42fefa3800, LN2L 0x3d2ef35793c76730) serves log_kernel and log_pow;
log_pow's rh^3/3 = two_prod(s.hi, rh), two_prod(c.hi, THIRD.hi) and one correction. DD99 replaced by DD94·|e| + DD100.
Every bound restated term by term in the doc comments; unit tests fast64::bounds check them against mp.

Results (run 2, shiki, all on 0a616056):
- Counts (math-m6-f64-gap-assembly-2-percall-base / -percall, rc 0), instructions:u per call, Liella before -> after
  (glibc): exp 344.7 -> 320.6 (67.1), exp2 257.5 -> 233.4 (51.1), log 295.5 -> 235.4 (63.1), log2 332.7 -> 272.6
  (63.1), log10 345.9 -> 285.8 (102.9), pow 756.1 -> 633.8 (141.2). -AVX2: the same savings (glibc 77.1/51.1/78.1/
  83.1/118.3/191.3). No row slower. Base = the head with fast64.rs from 2cd4384d (pre-assembly).
- Plants (math-m6-f64-gap-assembly-2-plants, rc 0): 10/10 CAUGHT with "above the bound": 7 term drops plus, for each
  restated bound (exp_kernel eps, log_kernel eps, log_pow el), the result moved by 2x the bound (one bit outside).
- Sweep: code unchanged from run 1, so not re-run. Run 1's shiki:math-m6-f64-gap-assembly-sweep passed the test
  (every chunk 0 mismatches, `test result: ok`, 2515 s); its rc 2 came after the test: job.sh was rewritten while the
  sweep was still reading it (sh reads a script as it goes). Fixed in ~/work/math-m6-f64-gap-assembly-2/job2.sh (new
  name, explicit exit per mode, never edited while one of its jobs runs).
- clippy --workspace --all-targets --all-features -D warnings rc 0, xtask static rc 0 (role, 0a616056).
- Gate math-m6-f64-gap-assembly-2-gate: escalated to FULL by itself (the breakdown branch's s10_math.tsv /
  s10_math_gen.rs), running detached; run 1's FULL gate on the same code passed (rc 0).

Open questions: none. exp saved 24 against the estimated ~35; why is not measured.
Not to redo: the proofs, the bound tests, the LN2 split constants, the counts, the sweep.
Next step: story B (register 13.5 #2) from this branch after the gate is read.
Plan rows to read: D-10.13; docs/saturation-register.md 13.5.
