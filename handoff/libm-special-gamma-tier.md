# LIBM-SPECIAL-GAMMA-TIER handoff (run 1: CHECKPOINT, WIP, nothing compiled)

State: branch `task/libm-special-gamma-tier` (cut from origin/next de5102e3), WIP code commit 18ad79de.
Role keke:~/w/n3 (not synced yet, no job run). Stopped at the context checkpoint (~150k of 180k; 33 of 90
calls). Pattern: handoff/libm-special-fast-tier.md (erf/erfc). Accuracy contract unchanged (correct
rounding, mp fallback, ruling #2760 option A): no differences row.

## Design in the WIP commit (read the module docs, they carry the proof)
* Generator `crates/mathtools/src/gamma_tables.rs` (called by `cargo xtask tables` after the erf tables):
  ln Gamma Taylor coefficients from Hurwitz zeta (96 direct terms + Euler-Maclaurin, B2..B20), tables
  `LG_T` (ln Gamma on [2^-5, 16), 144 slots, 4 slots by the zeros unused/excluded) and `LG_Z1`, `LG_Z2`
  (ln Gamma(z+t)/t at z = 1, 2). Target 2^-75 relative (so tgamma = e^L stays below 2^-70).
  `special_tables.rs`: `Slot`, `geometry`, `emit_table`, `MAX_DEG` now pub(crate); `choose`/`emit_table`
  take the target (erf tables unchanged: same TARGET passed).
* Runtime `crates/math/src/fast_gamma.rs`: `split(x)` gives ln|Gamma| = L - ln|D|, Gamma = e^L/D for
  2^-60 <= |x| < 16 (shift by n = -floor(x) for x <= -1/2, exact products; the RN(1+x) trick on
  (-1/2, -2^-5]); `lgamma`, `tgamma`, `*_kernel`. `fast64::log_dd` added (log_kernel + log_eps).
  Wired in `special::lgamma/tgamma` for F::PREC == 53; old bodies are `lgamma_accurate`/`tgamma_accurate`.
* Near lgamma's negative zeros: REFUSAL to the mp path (settle refuses on cancellation); no dd step.
* Tests `crates/math/tests/fast_gamma.rs` (3 tests: oracle in 4 modes, identity with accurate path,
  kernel bound + fallback ceiling 1/500 on [-10, 10]). Ceiling is a guess: measure, then set it.

## Exact next step
1. Sync (`sync_role.sh --story LIBM-SPECIAL-GAMMA-TIER --base origin/next <wt> keke n3`), run
   `cargo xtask tables` on the role (regenerates crates/math/src/coeffs/tables.rs; commit it), then
   `cargo test -p liella-math --release --test fast_gamma` and fix what fails (first compile: not done).
2. Check that settle rounds negative y correctly (lgamma on (1,2) and negative tgamma; erf mirrored the
   mode instead, tests in 4 modes decide) and that generator assertions (fmin > 0) hold on every slot.
3. Measure the fallback rate on [-10, 10] (stats line in test 3) and record it; counts before/after
   (`cbench_libm` rows lgamma, tgamma; before = current next), clippy, static, gate (direct tier),
   plants (bound, a table entry, a region edge, the near-zero refusal) via `cargo xtask plants`.
4. Out of scope this run: x >= 16 (Stirling) and x <= -16 (mp still), float rows.

## Things not to redo
* Process slip this run: one python heredoc edit (special_tables.rs visibility); use files instead.
