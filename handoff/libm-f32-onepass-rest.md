# LIBM-F32-ONEPASS-REST: handoff (binary32 functions onto the one-pass `float` path)

State: branch `task/libm-f32-onepass-rest-4` (run 4; stacks on run 3's 3db6e91b2, no merge, no code change this run).
Role kanon:~/w/kn1. Decisions in force: D-10.72 (extends D-10.63 under D-10.67, D-10.68). Plan rows to read: D-10.63,
D-10.67, D-10.68, D-10.72. Reports: handoff/reports/LIBM-F32-ONEPASS-REST{,-2,-3,-4}.json.

## Done (all runs)
* Runs 1-2: exp10f, logf, log2f, log10f on the one-pass path (`fast32::exp10_onepass_any`, `fast32::log_onepass`),
  exhaustive 2^32 x 4 directions, 0 mismatches each.
* Run 3 (93732e3f0): sinf, cosf, tanf (`fast32::trig_core`, also sincosf, float complex, Bessel), asinhf and acoshf
  build their `double` operand from the bits (no `cvtss2sd`); partial-write ALLOW list empty. Exhaustive sweep of the
  five (mei:libm-f32-onepass-rest-3-sweep rc 0), gate and plants green (see run 3's report json).
* Run 4: the counts red explained and re-measured; the five run-3 functions timed on both timing hosts against a base.

## Run 3's counts red (mei:libm-f32-onepass-rest-3-counts, rc 124): cause
* Not a slow bench. The job's script waited for the sweep job INSIDE its own jobrun (`after.sh counts` looped on
  `jobwait` from 15:46 to 18:04), and a shared slot job is capped at SLOT_MAX_MIN = 150 min (~/bin/slots), so the cap
  ran out 12 minutes into the real work (FMA section done, SSE2 section killed). mei's load added little.
* Fix (run 4): every job is its own jobrun, started only after the previous one ended; nothing waits inside a timed
  job (~/work/LIBM-F32-ONEPASS-REST-4/kn1.sh on kanon). Measured: both counts builds take 16 min on kanon
  (18:36-18:52); started with SLOT_MAX_MIN=60.

## Counts (kanon:libm-f32-onepass-rest-4-counts, rc 0; code = 3db6e91b2; instructions per call, Liella / glibc)
| row | FMA build | SSE2 build (hwcaps=-FMA) |
|---|---|---|
| sinf, cosf | 91.2 / 42.0, 42.1 | 91.2 / 50.8, 51.0 |
| tanf | 99.2 / 106.3 | 99.2 / 106.3 |
| asinhf | 100.0 / 89.0 | 100.0 / 94.4 |
| acoshf | 93.3 / 73.0 | 93.3 / 79.8 |
| powf | 249.8 / 76.1 | 249.7 / 91.2 |
sinf/cosf were 85.2 in run 2 (the bit-built operand costs ~6 instructions over one `cvtss2sd`).

## Timing (booked: kotori Ivy Bridge #153, nozomi N100 #154; Liella/glibc time ratio, cbench_libm sweep, 2 rounds each)
Base = this head with trig.rs, hyperbolic.rs and xtask/src/partial_write.rs from 07ceda237 (run 3's change undone),
staged on kanon (~/work/LIBM-F32-ONEPASS-REST-4/base, base.tar sha256 52c4a8b9...; branch libm.tar 6af2c118...).
| row | IVB base | IVB branch | N100 base | N100 branch |
|---|---|---|---|---|
| sinf | 1.77, 1.79 | 1.84, 1.84 | 2.34, 2.39 | 2.30, 2.22 |
| cosf | 2.30, 2.28 | 1.82, 1.82 | 3.10, 3.05 | 2.15, 2.17 |
| tanf | 1.32, 1.33 | 0.81, 0.82 | 1.23, 1.23 | 0.68, 0.68 |
| asinhf | 2.28, 2.30 | 1.42, 1.42 | 3.07, 3.09 | 1.68, 1.69 |
| acoshf | 2.38, 2.39 | 1.43, 1.43 | 3.88, 3.88 | 2.03, 2.03 |
| powf (control) | 4.47, 4.48 | 4.46, 4.47 | 4.86, 4.86 | 4.86, 4.86 |
| expf, logf (control) | 1.40, 1.37 | 1.39, 1.36 | 1.66, 1.61 | 1.66, 1.61 |
Jobs: kotori:libm-f32-onepass-rest-4-ivb, -ivb-base; nozomi:libm-f32-onepass-rest-4-n100, -n100-base (all rc 0).
Verdict: run 3's change is 20-48% faster on cosf, tanf, asinhf, acoshf on both hosts and on sinf on the N100; sinf on
Ivy Bridge is ~3.5% slower (spread < 0.04 on both sides). Kept (RULING NEEDED in the report). sinf and cosf share
`trig_core` and the same 6 extra instructions, so the sinf-only Ivy Bridge loss points at code placement or the
sinf tail, not the operand build; not investigated (budget).

## Not done / open
* powf (IVB 4.46x, N100 4.86x; 249.8 vs 76.1 instructions). Notes for its design: the bench's operands (x in
  [0.01, 10], y in [-10, 10]) all stay in `pow_core`'s range, so the 250 instructions are the fast path itself, and
  the time ratio (6.4x on the Skylake-SP host) is twice the instruction ratio (3.3x): look for serialising work
  (MXCSR read/write in `mret::elem` + `Real::pow` dispatch), not only instruction count. A one-pass powf needs (a) a
  proof in every direction: `pow_core`'s Fast2Sum steps (`a_lo`, `h_lo`, `t_lo`) are exact only in round-to-nearest,
  so bound their error in RD/RU/RZ or restructure; (b) exact results refused on the bits before any FP operation
  (spurious inexact otherwise): x a power of 2 (fraction bits 0) gives exact 2^n for many y; otherwise an exact
  x^y needs y = n/2^k with k <= 3 and 0 < y < 16 (x's odd part m >= 3 must be a 2^k-th power and m^y < 2^24), a
  bit test on y like the existing integer test. Refusing y = 2 or 0.5 sends common calls to the envelope: measure
  that, or prove the kernel exact there.

## Next (in order)
1. powf: profile first: validation/wl_trace.sh on a powf percall case (split envelope / dispatch / `round` /
   `pow_core`), write the split here, then design from it (notes above).
2. Optional: sinf on Ivy Bridge (per-call trace of sinf vs cosf in the branch build) if the ruling asks for it.

## Do not redo
* The exhaustive sweeps of all nine functions (code unchanged since). The counts and timings above (code unchanged).
* The partial-write CI red (fixed in run 2) and the hot-text regen (not needed on mei in run 3).
