# MATH-M6-F64-GAP-EXP-REDUCE (register 13.5 #2): CHECKPOINT, NOT READY

State: branch task/math-m6-f64-gap-exp-reduce, code commit 80b39d15 (on 7ff703c7 = assembly-2 head a164711c + origin/next;
the register conflict resolved by keeping both sides). Role keke:~/w/n4 (synced at 80b39d15, `.changed` from base origin/next).

## Done
- `fast64.rs`: `reduce_ln2` (C0 36 bits, -C1 35, -C2 34 of `LN2_64_CW`, the fourth part dropped), `reduce_exp`
  (x - k*C0 exact, one Fast2Sum for -k*C1, low part s.lo - k*C2, renormalizing Fast2Sum; no x.lo add), `reduce_pow`
  (z.lo joins the low part), `ln2_times` for exp2 (Dekker with ln2 pre-split `LN2A`/`LN2B`, not renormalized,
  |lo| <= 2^-52.5*|hi|). `dd::split` made pub(crate). exp10 and the accurate path keep `exp_log::reduce`.
- Bounds restated in the code, term by term: exp r within 2^-106.5, pow 2^-96.9, exp2 2^-104*|r|; all inside the
  kernel's 2^-90. The Fast2Sums are exact through the exponent condition (first operand a multiple of the second's last
  place: r0 a multiple of 2^-60 when k != 0, s.hi of 2^-80), so tiny r near multiples of ln2/64 is covered.
- New mp-oracle tests `fast64::bounds::exp_reduction_stays_within_its_bound` (20k+4k operands plus every 11th
  multiple of ln2/64 and its neighbours) and `exp2_product_stays_within_its_bound`; existing kernel test uses
  `reduce_exp`. keke: `cargo test --release -p liella-math --lib fast64` 4 passed; workspace clippy -D warnings rc 0;
  `cargo xtask static` rc 0.
- Plants (keke job math-m6-f64-gap-exp-reduce-plants, RAM copy plants-1): 10 plants, 6 CAUGHT with the expected
  message at checkpoint time (drop-kc2, drop-slo, c0-one-ulp, exp-r-2m105, pow-drop-zlo, pow-r-2m95); the four exp2
  plants were still running. Script exits non-zero on any survivor.

## Not done / open
- DIRECT GATE NOT RUN. `cargo xtask affected --run` escalates to FULL here: without --base (gate), and with
  --base origin/next (gate2) because the stacked-on assembly-2 commits include the generated
  `crates/boundary/src/posix/s10_math_gen.rs`; all stopped (rc 143). gate3 (`--base 7ff703c7`) was queued and stopped
  too: the role tree reads `.changed` from the sync, and re-syncing with `--base 7ff703c7` was refused, sync_role.sh rc 4
  (role busy with the queued gate). Next: sync with `--base 7ff703c7` (the merge commit before this story's code) once
  the role is idle, then run the direct gate; or wait for assembly-2 to land and use --base origin/next.
- Counts: job math-m6-f64-gap-exp-reduce-percall (keke, ~/work/math-m6-f64-gap-exp-reduce/counts.sh) runs `cargo xtask
  bench --compare --counters cbench_libm exp exp2 log log2 log10 pow` default and `--tunables glibc.cpu.hwcaps=-AVX2`
  for base (7ff703c7's fast64.rs/dd.rs) and after, in RAM copies. Not read yet. Then: register 13.5 row #2 and D-10.13
  get the measured saving (not edited yet: no numbers).
- Sweep: job math-m6-f64-gap-exp-reduce-sweep (SLOT_SIZE=M, SLOT_MAX_MIN=300) runs `math_double_fast_entry_sweep` on
  a RAM copy of the final code.
- INFORMATIONAL: origin/next's docs/saturation-register.md has three sections headed "## 13. Next ranked perf stories"
  and this branch's "## 13. f64 exp/log/pow" now shares the number; renumbering is the coordinator's call.

Not to redo: the proofs, the split constants (unit-checked), the tests, the plants script.
Plan rows: D-10.13 (plan/07-decisions/s10-mathematics.md); register section 13.5.
