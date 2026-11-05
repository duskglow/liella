# LIBM-SPECIAL-BESSEL-TIER handoff (run 2, LIBM-SPECIAL-BESSEL-TIER-2)

State: branch `task/libm-special-bessel-tier-2` (stacked on run 1's `task/libm-special-bessel-tier`, which sits on
`task/libm-special-fast-tier-2`, now in next). Code commits 44cc3631 (the fixes) and be0ce2c0 (clippy-clean), then this handoff commit. Role keke:~/w/n4
(Zen 1, EPYC 7551); sync with `--base origin/next`. Accuracy contract unchanged (correct rounding, ruling #2760 A);
no differences row, no compat option, no new D-row.

## Run 2: the red causes, found and fixed (44cc3631)
1. **The j0 fallback rate of 4e-4 was one region, not the slot bounds.** All 40 of the 40 fallback operands were
   Hankel operands above 1.4e300 (2^997). Cause: `dd::two_prod` splits each operand Dekker-style
   (`(2^27+1)·a`), which overflows to inf for `|a| >= 2^997`, so the product is NaN. `Dd::div` multiplies the
   divisor by the quotient, so Hankel's `1/x` was NaN for every x above 2^997. Fix: `1/x = (1/m)·4^-k`, `x = m·4^k`
   (exact scaling).
2. **The same cause in y1:** `(2/π)/x` gives a quotient above 2^997 for x < 2^-997.6, and the division splits it
   (NaN kernel error at 1.5e-301). Fix: divide at `x·2^64` and scale back by 2^64 (exact).
3. **j1's tiny range:** `|y|·2^-90` underflowed to 0 below 2^-984 (an unsound bound), and below 2^-358 the series
   value is exactly x/2, which no directed mode can settle. Fix: under 2^-27 j1 is `x/2` moved toward 0 by under
   `2^-56·|x/2|` (`wide::beside`, as j0 is beside 1); it declines below 2^-1021 (x/2 subnormal). The series bound
   carries an absolute 2^-1068 floor, so `kernel` stays sound down to 2^-1000.
The tests now print every fallback operand, sample Hankel's range up to f64::MAX (it stopped at 2^1000 and missed
cause 1) and the new switches 2^-1021, 2^996, 2^997.

## Results (test job libm-special-bessel-tier-2-test2 on 44cc3631, rc 0; be0ce2c0 changes lints only)
* Fallback in 10^5 random calls (half on the bench range, half log-uniform from 2^-1000 to the largest double; Nearest
  and Up): j0 1, j1 0, y0 1, y1 0. Like the erf tier (about 1e-5); the zeros do not force more. The one j0 and one
  y0 fallback are the expected rounding-test refusals (rate 2·eps/ulp, about 2^-20 for eps 2^-74 against values
  around 0.3, more near a zero).
* Kernel error over bound, worst per function: j0 0.80, j1 0.72, y0 0.69, y1 0.82 (8 operands per slot, both ends,
  the series range, Hankel up to f64::MAX). All 4 modes match the oracle on the hard operands (slot ends, switches,
  40 doubles around each zero, huge operands), and the fast tier matches the 384-bit path bit for bit (value and
  status) wherever it settles.
* On be0ce2c0: workspace clippy (all targets, all features) and `cargo xtask static` pass (foreground, keke n4).
  Jobs on be0ce2c0 (keke): `libm-special-bessel-tier-2-gate2` (asked for the direct tier with `--base origin/next`;
  `affected` ESCALATED it to FULL by design because the branch changes generated sources, crates/math/src/coeffs/tables.rs
  and crates/mathtools/src/tables.rs from run 1, so it is long and was left running at hand-back), then
  `libm-special-bessel-tier-2-counts-after`; `libm-special-bessel-tier-2-plants2` (10 plants,
  handoff/libm-special-bessel-tier-2-plants.txt, dir /dev/shm/claude/LIBM-SPECIAL-BESSEL-TIER-2/plants-2).
  Superseded: plants (run on 44cc3631, killed after its baseline and 3 of 10 CAUGHT:
  row-bound, table-coef, hankel-switch), lint (rc 1: the clippy errors fixed in be0ce2c0), gate (killed in the queue, stale head), test1
  (rc 101, the red run before the fixes).

## Counts (cbench_libm, instructions per call, Liella / reference)
Before (run 1, `libm-special-bessel-tier-counts-before`, Haswell host): j0 121,247.3 / 415.1; j1 121,324.6 / 420.4;
y0 472,291.5 / 435.9; y1 472,301.1 / 435.6.
After: in `libm-special-bessel-tier-2-counts-after` (keke, Zen 1; the before is Haswell, the reference column of the after run is the same-host comparison).

## Open / next
* Cost: read the after counts first (`tail ~/logs/libm-special-bessel-tier-2-counts-after.log` on keke). The bench range is all table region, so the row cost is the Taylor row (D 18, M 8) plus settle.
* `dd::two_prod` (Dekker) costs about 17 operations and has a 2^997 operand limit; `two_prod_fma` (D-10.25 run-time
  dispatch) is 2 and has no split. The Taylor rows take 8 double-double steps (layout D 18, M 8, driven by Y1 near
  2^-5 and by the top binade's |t| up to 1/2): an FMA build of the kernel is the obvious next cut (results are the
  same bit for bit wherever `two_prod` is exact, which every product here now is).
* `trig::sin_cos`'s 2^-90 absolute is argued in the module doc, not proved in trig.rs; the kernel test covers it over
  Hankel's whole range (now up to f64::MAX).

## Not to redo
* The ODE recurrence is stable at 640 bits (the generator checks it against 960 bits to 2^-300 on every slot).
* Fallback diagnosis: the per-operand print in the rate test settles which region refuses in one run.
