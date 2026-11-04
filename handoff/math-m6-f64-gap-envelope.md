# MATH-M6-F64-GAP-ENVELOPE (register 16.5 #3 + #4), run 3 (ENVELOPE-3): survivor explained, specials pre-checked

State: branch task/math-m6-f64-gap-envelope-3 (stacks on -2, -1), code head 03fb0b49, cut from origin/next 432f1ef7.
Role mei:~/w/s2. Scratch on mei: ~/work/math-m6-f64-gap-envelope-3/ (plants.sh, counts.sh, run3.patch, *.out, logs);
RAM copies /mnt/zram/claude/MATH-M6-F64-GAP-ENVELOPE-3/{plants-1,base,after,sweep-1}. Run 2's scratch is on shiki
~/work/math-m6-f64-gap-envelope-2/.

## Done (runs 1-2)
- `#[inline(always)]` kernels and entries (#3), `elem_fast_cold` out of line (#4), the five fn-item entries as closures
  in s10_math.tsv + math-gen (`clippy::redundant_closure` allowed in xtask/src/math.rs). Six fused entries.
- Counts (run 2, shiki, base 432f1ef7): exp -22, exp2 -28, log -29, log2/log10 -13, pow -40 instructions per call;
  exp_qnan/overflow_errno/tiny and expf_qnan +9..+14 (ruling #2378: keep the trade-off).

## Done (run 3)
- Plants-2 survivor (skip-mode-check, fast path in every word) EXPLAINED: values cannot show which path ran. The fast
  kernels' rounding tests (`near`/`finish`) also hold in the caller's directed hardware mode, so the in-place result is
  the directed one; and the every-mode test's "full envelope" reference used the DAZ word, itself non-default, so under
  the plant both sides took the fast path. Exact cases are refused before any arithmetic, so no flag shows either.
  An unmasked inexact trap does not help: the envelope raises inexact on exit in the caller's word as it must.
- New test `math_double_fast_entry_stays_out_of_place_in_a_non_default_mode` (s10_math_symbols, s10_0_f_0): the six
  entries under every direction x {-, FTZ, DAZ, FTZ+DAZ}, compared with liella-math's directed value AND MXCSR after
  == caller's word | reference flags exactly (no DE mask). A subnormal argument (1e-310, 5e-320) makes the plant
  visible: the fast domain comparison raises DE in hardware, and in a non-default word `elem` then saves and restores
  it, while the entry never leaves DE. Also covers qNaN, sNaN, +-inf in every word.
- Pre-check (ruling #2378): `fast64::special` on the refusal branch of exp/exp2/log/log2/log10 `_nearest`: quiet NaN
  -> itself, +inf -> +inf, -inf -> +0 (exp, exp2) / refused (logs, invalid). Exact, raise nothing, no errno. sNaN and
  all other refusals still go cold. pow not pre-checked (pow(NaN,0)=pow(1,NaN)=1). elem32_fast doc contract amended.
- expf_qnan +9 EXPLAINED: `elem32_fast` is an `#[inline(always)]` wrapper of `elem_fast`, so the f32 entries also got
  run 1's out-of-line `elem_fast_cold` (#4); their refused inputs pay the cold call like the f64 ones. fast32 has no
  special pre-check yet.
- clippy (--workspace --all-targets --all-features -D warnings) rc 0 and `cargo xtask static` rc 0 on c7a3b124;
  s10_math_symbols `math_` tests green on 03fb0b49 (8 passed).

## Jobs (mei ~/logs/jobs.tsv) on 03fb0b49
- math-m6-f64-gap-envelope-3-plants-1: skip-mode-check, wrong-rounding-dispatch, special-snan, special-neg-inf,
  special-exp-sign expected CAUGHT; drop-black-box EQUIVALENT (order only visible under an unmasked trap).
- math-m6-f64-gap-envelope-3-gate: `cargo xtask affected --base origin/next --run`, ESCALATES TO FULL by itself
  (s10_math.tsv generator input, generated s10_math_gen.rs, xtask/src/math.rs from run 2): long, detached.
- math-m6-f64-gap-envelope-3-counts: cbench_libm default and -AVX2, base e275745d (run-2 head = 03fb0b49 minus
  run3.patch) vs after 03fb0b49 -> counts-{base,after}-{default,noavx2}.txt.
- math-m6-f64-gap-envelope-3-sweep: the full fast-entry sweep (200 chunks x 2e6 pairs, release) on the final head.
  It replaces the 10-chunk base-vs-after digest: run 3 changes no kernel arithmetic, only the refused specials.

## Run-3 results
- plants-1 rc 0: ALL SIX CAUGHT. skip-mode-check and drop-black-box by the new test (drop-black-box was expected
  EQUIVALENT: without the barrier the domain compare is hoisted ahead of the mode branch and DE leaks), wrong-rounding-
  dispatch by math_envelope_keeps_the_callers_mxcsr, special-snan by the new test, special-neg-inf and special-exp-sign
  by both fast-entry tests. Verdict lines went to jobrun's log, not plants-1.out: read plant-*.log.
- counts rc 0 (instructions:u per call, base e275745d -> 03fb0b49, default; -AVX2 the same within 0.1):
  exp_qnan 111.1 -> 35.1 (pre-fusion base 98.2); exp/exp2/log/log2/log10/pow and the _upward rows unchanged (+-0.3);
  expf_qnan 114.1 unchanged (no fast32 pre-check). SLOWER: exp_overflow_errno 223.2 -> 240.2 and exp_tiny
  457.5 -> 474.5 (+17 each): the `.or_else(special)` runs on every finite refusal too.
- clippy rc 0 on 03fb0b49 (RAM copy plants-1); gate and sweep still running at hand-back.
- NOT READY on the +17: fix next run by testing specials only where the domain test failed on a non-finite x (e.g.
  inside `if !(abs(x) <= f64::MAX)` in exp_near/exp2_near/log_domain refusal), recount, keep only if no row is slower.

## Open
- Register rows 13.5/16.5 and the D-10.13 note with the measured saving: NOT DONE (budget); next run.
- fast32 special pre-check (expf_qnan back to base): same helper shape in fast32.rs; not done.
- Remaining `push rbx` (run 1's step 2): not done.

## Exact next step
1. Read the gate and sweep rows on mei. Fix the +17 on exp_overflow_errno/exp_tiny (Run-3 results), recount, plants.
2. Register rows 13.5/16.5 + D-10.13 note from counts-after-*.txt (run 2's numbers above for the fusion; run 3's delta).
3. Optionally the fast32 pre-check and the `push rbx`.

## Not to redo
- Base build of 432f1ef7 and run 2's counts (shiki). The fused form is confirmed by disassembly (run 2).
- The Edit tool trims a trailing space in a replace_all new_string; there is no local cargo (run cargo on the role).
- A remote `a && nohup job &` list keeps the ssh channel open: redirect the whole list or start jobs one per ssh.
