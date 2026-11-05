# LIBM-F32-ERF: handoff (erff, erfcf on the fast32 PATTERN)

State: branch `task/libm-f32-erf-2` (stacks on `task/libm-f32-erf`), code head `772c17ab`, role keke:~/w/n1.
LIBM-F32-ERF-2 ran out of its 70-call budget. Verdict: INCOMPLETE. Jobs on keke, started detached on 772c17ab and
QUEUED behind another story's sweeps when this was written: `libm-f32-erf-2-gate` (`cargo xtask static`, then
`cargo xtask affected --base origin/next --run`, which escalates to FULL because of the generated files) and
`libm-f32-erf-2-plants` (`cargo xtask plants ~/work/libm-f32-erf-2/plants.txt --dir
/dev/shm/claude/LIBM-F32-ERF-2/plants-1`). Their results were not read. NOT run: counts, the tiered oracle, sweeps.

## What -2 found and fixed (the -1 gate `libm-f32-erf-gate` rc 1)
* Cause 1: the fits for erfcx pieces 3 and 4 covered only half of each piece (±27/64, ±81/128 instead of ±27/32,
  ±81/64), and piece 5 was centred on 1101/128 instead of 539/64. The kernel extrapolated: -855 ulp at 3.376378.
  Fixed in the fit files and `ERFCX_MID[5]`, coefficients regenerated (remez on keke: 0.65/0.72/0.12u after
  rounding), Horner bound restated for |t| ≤ 1.27. `erf_cores_are_within_their_bounds` now checks every piece is
  centred on its `ERFCX_MID`.
* Cause 2: erfcf's fallback-rate row counted 561 fallbacks, the same with old and new coefficients: x in
  [9.194, 9.25) has a subnormal erfc, which `round` refuses after the kernel. `ERFC_MAX` is now 9.1875
  (erfc = 2^-125.8 there), so they are refused before the kernel.
* Test bug: the bound test compared erfc's saturation value (x ≤ -4) with the `double` result; that region is now
  left to the agreement tests, like erf's.
* Unit run `libm-f32-erf-2-refit2` (on the fit fix, before cause 2 was fixed): 13 passed, the 2 above failed. The
  run on 772c17ab is the gate.

## Plants (`~/work/libm-f32-erf-2/plants.txt` on keke; copy in the hand-back)
erfc-delta (ERFC_DELTA 248 -> 200), erfcx1-coeff (one bit of piece 1's x^2), piece-lo (ERFCX_LO 3.375 -> 3.0),
piece-mid (ERFCX_MID 6.328125 -> 6.25), erf-sat (ERF_SAT 4.0 -> 3.5). cmd `cargo test -p liella-math --lib
fast32::tests::erf`.

## Next step (exact, in order)
1. `~/bin/jobwait libm-f32-erf-2-gate libm-f32-erf-2-plants` on keke; fix what they find (no reruns to pass).
2. Counts: `cargo xtask bench --compare --counters --rows cbench_libm/erff,cbench_libm/erfcf` (after). Before:
   erff 228,016 instructions (brief); erfcf before not measured (the base tree's mp path).
3. Ruling #2889: the tiered test oracle as a shared helper (one call per operand for all four modes): a
   double-double value with a proven relative bound and the rounding test per mode, mp only when undecided. Plan:
   erf x = (2x/√π)·e^(-x²)·Σ (2x²)^n/(1·3···(2n+1)) (all terms positive, no cancellation) for |x| < ~4;
   erfc x for x ≥ 1 needs its own dd evaluation (1 - erf loses up to 126 bits near 9.19): the continued fraction
   with a truncation bound, or erfcx by the same series route. Report the mp-fallback rate. Then size the sweeps
   with LIELLA_SWEEP_STRIDE=64, one jobrun job per function (7-CPU slot, well inside 150 min).

## Decisions in force
Rulings #2760 (Ziv fast tier), #2784 (one story per family), #2783 (software sqrt), #2889 (tiered oracle, no
stride in place of the exhaustive check). D-10.3: coefficients from the Remez tool, derivation in the fit files.

## Not to redo
The trial degree search (degree 15 holds). The fallback design (subnormal erfc results on the accurate path).
