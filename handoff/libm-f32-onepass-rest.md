# LIBM-F32-ONEPASS-REST: handoff (binary32 functions onto the one-pass `float` path)

State: branch `task/libm-f32-onepass-rest-5` (run 5; stacks on run 4's 463e43fcd, no merge, no code change in runs 4-5).
Role mei:~/w/s2. Decisions in force: D-10.72 (extends D-10.63 under D-10.67, D-10.68); hot text: D-X.56, ruling #4357.
Plan rows to read: D-10.63, D-10.67, D-10.68, D-10.72, D-X.56. Reports: handoff/reports/LIBM-F32-ONEPASS-REST{,-2,-3,-4,-5}.json.
Math work is finished (owner ruling #4424); powf moved to LIBM-F32-POWF and sinf on Ivy Bridge to LIBM-SINF-IVB-RATCHET (ruling #4425).

## Run 5: CI train 572 hot-text red (2 dead fast-profile keys): does not reproduce on this branch
* CI (merge becfe49f6 = next + this branch + DL-MAP-PROBE, LIBM-F64-QUICK-PROFILE, LIBM-LD-TRIG-INVERSE-INT,
  CODEMAP-TRAIN-OWNED, XTASK-SMALL-FIXES-1110): `hot-text.list: 2 key(s) match no function section of the fast-profile
  libc.a`: `Features::detect<CSysImpl<false>>` (startup) and `liella_boundary::iconv::load` (extra). Both are code in
  liella_boundary. This branch changes that crate only in `posix/s10_math_gen.rs` (8 lines). The train also changed
  `crates/rt/src/start.rs` and `crates/boundary/src/ld.rs`. None of the other train stories is on origin/next yet.
* This branch alone, unchanged list, on mei (the CI host class), clean environment:
  - fast `cargo xtask so` (mei:libm-f32-onepass-rest-5-lint0, rc 0): no dead key, STARTUP-TEXT-WINDOWS 5 of budget 5.
  - release `LIELLA_FULL_PROFILE=1 cargo xtask so` (mei:libm-f32-onepass-rest-5-relso, rc 0): STARTUP-TEXT-WINDOWS 4
    of budget 4, all 11 layout contracts hold.
* `regen.sh . --build` per D-X.56 (mei:libm-f32-onepass-rest-5-regen, rc 1): the fast pass found no dead key, but it
  re-ordered 38 lines of the list. The release pass then broke STARTUP-TEXT-WINDOWS (5 windows > budget-release 4). By
  ruling #4357 (repair only dead keys, keep the order), the list stays unchanged: this branch has no dead key to repair.
* The dead keys come from combining branches. Repairing them needs that combined tree, and this story may not merge.
  See RULING NEEDED in handoff/reports/LIBM-F32-ONEPASS-REST-5.json.
* docs/codemap/symbols.tsv was restored to the merge base (train-owned view, D-X.66 on next; the hand-back check
  refuses an edited view). Static on the branch alone (older codemap rule) now reports it stale. Static merged with
  next passes. Gates: FULL mei:libm-f32-onepass-rest-5-gate rc 0 on 276e7c219 (11426 passed, conformance 126).

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
1. Coordinator: find which combination kills the two keys (this branch alone on next, or the train without this
   branch). Repair them in the tree that shows them: `regen.sh . --build` in the fast profile, keeping only the
   dead-key repair if the release windows break (ruling #4357).
2. Math is done here: powf is LIBM-F32-POWF (notes above), sinf on Ivy Bridge is LIBM-SINF-IVB-RATCHET.

## Do not redo
* The exhaustive sweeps of all nine functions (code unchanged since). The counts and timings above (code unchanged).
* The partial-write CI red (fixed in run 2). The hot-text regen on this branch alone (run 5: no dead key in either
  profile; a full regen re-orders the list and breaks the release start-up windows).
