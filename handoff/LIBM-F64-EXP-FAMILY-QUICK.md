# LIBM-F64-EXP-FAMILY-QUICK: exp2 and exp10 on the exp quick kernel (expm1 not done)

State: branch `task/libm-f64-exp-family-quick` (stacks on LIBM-F64-EXP-QUICK-WIRE-2, base 799313e1e); code commit
531d0df92, plants file and handoff commits on top. Role keke:~/w/n2. Brief:
`/home/claude/state/briefs/LIBM-F64-EXP-FAMILY-QUICK.md`. Decisions in force: D-10.51, D-10.54, D-10.64 (new, this
story), D-10.25/D-10.50 (two builds), D-10.3 (CR).

## What is in

* `crates/math/src/fast64/quick.rs`: the kernel from `T_j` on is now one `kernel(kb, rh, rl)` shared by exp (code
  unchanged otherwise), `exp2_quick_parts` and `exp10_quick_parts` (own reductions, module doc section "exp2 and
  exp10" with the proof), `exp{2,10}_quick`, `exp{2,10}_tiered` in both builds; tier 2 is today's
  `fast64::exp2_nearest` / `exp10_nearest` (SSE2) and `fast64::fused::*` (FMA), `#[cold] #[inline(never)]`. Outside the
  quick domain (and exp10's integers 1..=23) the tiered functions call tier 2 directly, which keeps today's special
  values (NaN, infinities) and exact cases.
* `crates/boundary/src/hot/exp_log.rs`: `exp2_nearest` / `exp10_nearest` call the tiered functions; selftest hooks
  `exp{2,10}_quick{,_parts}` (selftest.rs).
* Bounds (D-10.64): FMA `RHO_FAM_FMA = 1.6875·2^-68` (computed exp2 2^-67.64, exp10 2^-67.30), SSE2 `RHO_FAM_SSE2 =
  1.4375·2^-61` (2^-60.51 both). The SSE2 builds drop the ln2/ln10 product's low part rather than paying a Dekker product
  (reasoning in D-10.64). Expected fallback FMA 0.010%, SSE2 1.14%.
* Exact cases: exp2 of an integer gives `zl = 0` exactly, declined without inexact; exp10 of integers 1..=23 is declined
  before the kernel (bit compare + `(x + 1.5·2^52) − 1.5·2^52 == x`, only for 1 <= x <= 23).

## Tests (clause s10_0_f_0 / §10.2 exp2, exp10; D-10.3)

* `quick/tests.rs` (SSE2): `family_constants_and_bounds` (constants vs Mp, bound recomputed, multipliers least),
  `family_error_within_bound` (largest seen 2^-60.820 exp2, 2^-60.818 exp10; bound 2^-60.476),
  `family_matches_correctly_rounded` (6·10^6 operands per function bit for bit; fallback 0.8%-1.6% on [-10, 10]),
  `family_declines_or_rounds_right_on_hard_cases`, `family_domain`.
* `crates/boundary/tests/exp_quick.rs`: `exp2_exp10_quick_fma_build`, `exp2_exp10_quick_sse2_build` (600k operands per
  function per build: quick answers and the `m::exp2`/`m::exp10` entries bit for bit, bound in Mp on every 64th,
  exp10(1..22) exact, FMA declines < 0.05%).
* NOT done: the brief's 10^8-operand four-mode accuracy job per function (the existing `exp_quick_accuracy_job` is exp
  only); the entries' other rounding modes go through today's paths (the quick phase is only reached in nearest, as for
  exp).

## Jobs (keke)

lint `libm-f64-exp-family-quick-lint4` (kernel-index, xtask clippy, static, the unit + boundary tests: rc 0; lint..lint3
SUPERSEDED: doc-list lint, type-complexity lint, a wrong bit count in my own test). Plants
`libm-f64-exp-family-quick-plants`: 4 of 4 CAUGHT, no WARN (exp2 j/k split off by one, exp10 ln10 low part dropped,
exp10 near always true, exp10 exact case not declined; file `handoff/LIBM-F64-EXP-FAMILY-QUICK-plants.txt`).
Gate `libm-f64-exp-family-quick-gate` (direct tier, base origin/next): `affected: ok`, rc 0, peak 11G, oom_kill 0.
Counts `libm-f64-exp-family-quick-counts` (cbench_libm `--compare --counters`, FMA build only): see below.

Counts before (brief, keke libm-coverage-1107-percall, FMA): exp2 133 vs glibc 54, exp10 150 vs 59, expm1 194 vs 89.
Counts after: PENDING (job queued behind another story's counts slot at hand-back; it starts after the gate; summary at
keke:~/work/LIBM-F64-EXP-FAMILY-QUICK/counts.log).

## Not done / next step

1. expm1 (Tang, ACM TOMS 18(2) 1992) on the same kernel: `T·(1 + r + q) − 1` before rounding, own short polynomial
   for `|x| < 2^-5`; its plant (small-|x| branch boundary) goes with it.
2. Booked timing on Ivy Bridge (nico/kotori) and N100 (nozomi) for exp2/exp10: NOT measured (budget); the brief's
   question whether exp2/exp10 share the IVB exp regression is open.
3. SSE2 build counts: NOT VERIFIED (the bench does not take `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`, see
   LIBM-F64-EXP-QUICK-WIRE-2's report).
4. The 10^8-operand accuracy job for exp2/exp10 (copy `exp_quick_accuracy_job`).

## Not to redo

The bound derivation (python check and the Rust test agree; exp's own bound reproduced exactly by the same formulas);
the Dekker-vs-drop choice for SSE2 (D-10.64) unless counts say otherwise. Any quick.rs edit needs `cargo xtask kernel-index`.
