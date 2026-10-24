# MATH-M6-F64-GAP-ASSEMBLY (story A of 5, register 13.5 #1 and #6): CHECKPOINT, NOT COMPLETE

State: branch task/math-m6-f64-gap-assembly (origin/next merged; stacks on task/math-m6-f64-gap-breakdown, not landed).
Role shiki:~/w/sh2, synced with --base origin/task/math-m6-f64-gap-breakdown.

Done (commits on the branch):
- cc4219a3 + as-cast fix: crates/math/src/fast64.rs. exp_kernel assembles t + t*p from one exact product and two
  Fast2Sums; new log_assemble (42-bit LN2H 0x3fe62e42fefa3800, LN2L 0x3d2ef35793c76730, |ln2-LN2H-LN2L| < 2^-101)
  serves log_kernel and log_pow; log_pow's rh^3/3 = two_prod(s.hi, rh), two_prod(c.hi, THIRD.hi) and one correction.
  DD99 replaced by DD94 (2^-94 per |e|) + DD100. Every bound restated in the doc comments, term by term.
- New unit tests fast64::bounds (no spec clause): each kernel's (y, eps/el) against mp::ln / mp::exp. They pass.
- D-10.13 (plan/07-decisions/s10-mathematics.md) records the assembly. Register 13.5 NOT updated yet (no counts).
- clippy --workspace --all-targets --all-features -D warnings: rc 0 on the head before this handoff commit. xtask static: rc 0
  (on cc4219a3; the later change is one test line).

Jobs on shiki (labels math-m6-f64-gap-assembly-*):
- gate: started on the code head, not yet read.  sweep (SLOT_SIZE=M SLOT_MAX_MIN=600, detached): running.
- plants: rc 1, SUPERSEDED: 4 CAUGHT; 3 "survivors" were NOT APPLIED to code (the patterns first matched the
  restated proofs in doc comments). plants-2 (patterns /g, fresh copy plants-2): running, not read.
- percall-before: rc 1, wrong bench arguments (names are not positional filters here; run cbench_libm with no
  names and grep the six rows). percall-base: rc 2, patch -R failed on the test hunk (fix: drop `|| exit 2`, keep
  the DD99 check). percall: rc 2, job.sh was overwritten while it ran.

Next step: read gate and plants-2 (all 7 must be CAUGHT or the info one); install a NEW script name (job2.sh) with the
percall-base fix; run percall-base and percall (copies base-2, after-2); fill register 13.5 rows 1 and 6 with the
measured savings at both levels (default, -AVX2) vs glibc; keep only rows not slower; report JSON jobs.
Not to redo: the proofs; the bound tests; the LN2 split constants (computed from tables LN2 by exact rationals).
