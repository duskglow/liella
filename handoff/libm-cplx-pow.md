# LIBM-CPLX-POW handoff: cpow/cpowf quick + middle tiers, middle bounds proved (run 3)

State: branch task/libm-cplx-pow-3 (continues -2's branch; 11 commits behind next, no merge allowed). Code head
78bff2d8f; role mei:~/w/s1. Run 3 stopped at the 180k context checkpoint (Effort: max budget).
Decisions in force: D-10.53, D-10.74, D-10.99, D-10.25, D-10.17. No decision record for this tier yet.

## Blocker found in run 3 (not this branch): `cargo xtask so` fails on this tree

-2's gate (276 product-test failures) and plants (rc 2, BASELINE FAILED) share one cause: `xtask so`
stops with `hot-text.list: 1 key(s) match no function section of the fast-profile libc.a:
_RNvNtC6liella6locale7builtin`. Reproduced on mei (job libm-cplx-pow-3-lint0: clippy 0, static 0, so 1).
The branch touches only crates/math; another story's probe of next plus a 3-line dummy change failed the same
way (mei job lhis2-probe-nextdummy, 11-12 13:44), as did libm-cplx-sincos-2-plants on keke: the inlining of
`locale::builtin` flips with unrelated code growth (layout-jitter family; HOTTEXT-REGEN-HOSTS chain). Not chased
(no regen, no hot-text.absent line). Until it is fixed, `xtask plants` (it builds the products first), the
`affected` gate and percall cannot pass on any tree.

## Done in run 3 (#5023, #5022)

* `exp_log::log_dd` and `trig::atan_dd` carry step-by-step proofs (doc comments): primitives (JMP 2017 Alg. 6
  and 4 cited; mul_f64 3u², mul 8u², div 5u² derived there), series truncation, f64 tails, table facts,
  per-branch sums. Proved 29.8u² and 34.6u²; constants `LOG_DD_REL = 32u²`, `ATAN_DD_REL = 36u²`.
* complex_fast_pow.rs: `LR_REL`, `LR_ABS` (sum_squares' 2^-104.4 halved), `LI_REL` (+ div, sub, 2·PI_2)
  derived from them (the old 2x estimates are gone; the old constants were 4-7x above the proofs, so no
  past result was wrong).
* Test `cpow_middle_bounds_cover_their_proofs`: |s| <= 2^-8.4 per LOG_C bucket, LOG_L/ATAN_J64/LN2/PI_2
  within u² (mp), the proofs' sums vs every constant, 4000 sampled log_dd/atan_dd errors vs mp.
* Sweep `cpow_decline_rates` per |w log z| band (2^-6..1, 1..16, 16..512), 2^21 operands each: asserts no
  middle decline and the bound-implied rate <= 2^-30 per part; long-sweeps row in xtask/src/longsweeps.rs.
  Result: see job libm-cplx-pow-3-check1 (mei) and the report.
* Plants file handoff/LIBM-CPLX-POW-3-plants.txt (strict test + each bound halved); NOT RUN (blocker above).

## Next step (continuation, after the hot-text fix lands)

1. Sync; run `cargo xtask plants handoff/LIBM-CPLX-POW-3-plants.txt` and the `affected` gate (base origin/next).
2. Decision record: `cargo xtask records new decision D-NEW --series D-10 --story <story>` (method, proofs,
   strict test, measured rates; cite Joldes-Muller-Popescu 2017).
3. In-place entry: `cplx2_fast` in crates/boundary/src/mathret.rs (mirror `cplx_fast`), rows `cpow f`/`d` in
   posix/s10_math.tsv, `cargo xtask math-gen`.
4. percall typical and hard rows (with and without `--tunables glibc.cpu.hwcaps=-FMA`) vs next and glibc.
