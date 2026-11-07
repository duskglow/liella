# LIBM-F64-LOG-QUICK: binary64 log quick phase (tier 1), checkpoint at the context limit

State: branch `task/libm-f64-log-quick` (cut from origin/next at 799313e1e), code head 47bd1986a; this handoff
on top. Role mei:~/w/s3. Brief `/home/claude/state/briefs/LIBM-F64-LOG-QUICK.md`. Decisions in force: D-10.51
(two-phase double), D-10.60 (this kernel: method, table, bound, measured numbers), D-10.25/D-10.50 (two builds),
D-10.3 (correct rounding).

## What is in (log only; log2/log10/log1p not started)

* `crates/math/src/fast64/quick_log.rs`: Tang's table-driven log with 128 intervals of `m` in `[45/64, 90/64)`,
  8-bit reciprocals so `r = c·m − 1` is exact (Gal & Bachelis), `−ln c` and `ln 2` high parts on the 2^-42 grid,
  Taylor degree 9 in Estrin's scheme, `r − r²/2` as an exact pair, Ziv's one-sided test, `RHO = 2^-64`. Both
  builds (SSE2 macro build, `fused` FMA build). `log_tiered` = quick phase, then `fast64::log_nearest` (tier 2,
  unchanged) on a miss or outside the domain (non-normal / non-positive x go to tier 2, which handles them).
* Why degree 9, not 5-6 as the brief suggested: log's series coefficients do not decay (1/n), and next to 1 (`c = 1`)
  the error must be relative to `r`: degree 6 leaves about 2^-45 there. Degree 9 gives 2^-66.3 truncation.
* Table `LOG_Q128` ([c, b, Lh, Ll] x 128, 4 KiB) and `LN2_Q` from `crates/mathtools/src/tables.rs::log_quick`
  (`cargo xtask tables`); `crates/math/KERNELS.md` regenerated (`cargo xtask kernel-index`).
* Wired: `crates/boundary/src/hot/exp_log.rs::log_nearest` now calls `quick_log::{,fused::}log_tiered`;
  selftest hooks `log_quick`, `log_tiered`, `log_quick_parts`.
* Tests (clause s10_0_f_0, D-10.3): `quick_log/tests.rs` (constants and the bound recomputed per row and case;
  error vs `mp::ln` over 3·10^5 operands; 2·10^6 answers vs the correctly rounded log plus fallback window
  0.03%-0.3%; hard cases incl. misroundings of the unchecked `zh` found among the misses, domain, log(1) = +0);
  `crates/boundary/tests/log_quick.rs` (FMA error bound; both builds' quick phase, tiers and the `log` entry CR).

## Results so far (mei, job libm-f64-log-quick-build1, head before the hard-case fix)

Bound recomputed: case A (e = 0, c = 1) 2^-64.91, B 2^-67.63, C 2^-70.91. Largest error SSE2 2^-65.758, FMA
2^-65.921. Fallback SSE2 797 of 800000 (0.0996%; model C − 1 = 0.098%), FMA 200 of 200000. Every answer correctly
rounded. The hard-case test failed only on its non-vacuity check (no misrounded `zh` in a blind sample); fixed by
drawing from the misses (head 47bd1986a), not yet re-run when this was written.

## Jobs on the final code head 47bd1986a (mei, started detached, NOT collected at hand-back)

* `libm-f64-log-quick-gate`: full clippy, xtask clippy, static, the new tests, then `affected --base origin/next --run`.
* `libm-f64-log-quick-plants`: `handoff/LIBM-F64-LOG-QUICK-plants.txt` (5 plants: near-dropped, index-off-by-one,
  ln2-low-dropped, coeff-one-ulp, fma-square-low-dropped), dir /mnt/zram/claude/LIBM-F64-LOG-QUICK/plants-1.
  Re-attach: `ssh mei '~/bin/jobwait libm-f64-log-quick-gate libm-f64-log-quick-plants'`.

## Not done (exact next steps)

1. Collect the two jobs; fix anything red (likely suspects: the hard-case test's `wrong > 0`, a clippy lint in the
   new files, the static traceability check for the new test file).
2. Counts: `cargo xtask bench --compare --counters cbench_libm` plain and `-FMA` (rows log), before (origin/next)
   and after; target log <= 90 instructions (FMA). Not measured: no instruction claim is made.
3. Step 1 of the brief (the serialising step: wl_trace of one call, latency/throughput pair on a booked Ivy Bridge)
   was NOT done. The envelope items (inline `stmxcsr`, register-only `black_box`) are still under the owner's
   rulings #3938/#3939 (handoff/LIBM-F64-EXP-QUICK-WIRE.md).
4. Booked timing (IVB and N100) of log after the counts.
5. log2 / log10 (same table, `Lh`/`Ll` and `Kh`/`Kl` pre-scaled by 1/ln b: a second table or a scaled final
   product), log1p (Fast2Sum of 1 + x, then this kernel; its own short series for |x| < 2^-5): separate -WIRE stories.

## Not to redo

The table design (8-bit c, exact r in both builds), the bound derivation and its test, the fallback model.
