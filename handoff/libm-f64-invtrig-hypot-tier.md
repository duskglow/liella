# LIBM-F64-INVTRIG-HYPOT-TIER (run -3): plants green, counts recorded, one red test (INCOMPLETE)

State: branch `task/libm-f64-invtrig-hypot-tier-3` (carries runs -1/-2), code head `f6db12c2`, cut from origin/next
2c1e25b9 (next has moved on; no merge done); role kanon:~/w/kn1. Results stay correctly rounded (ruling #2760 A);
no differences row changes. Full numbers: docs/saturation/21-f64-invtrig-hypot-fast-tier.md.

## Done in run -3
* Run -2's jobs on keke never ran (gate queued, plants stopped after its copy line, counts never registered: its
  driver was not detached and died with the session). Re-run here, every job under `setsid nohup`.
* `cargo xtask static` red on -2's head: arch-audit (untagged SSE2 seam in hot.rs) and opaque-audit (invtrig test
  calls libc names). Fixed: `ARCH(x86_64)` tag, `#![no_builtins]` (ece587f5).
* Plants run 1: bound-zero and exact-root SURVIVED. Cause: no operand within the kernel's own error of a midpoint;
  the small exact triples give a root low word of exactly 0. Added `near_midpoints` (tiny-x atan/asin series
  crossings k+1/2 ulp) and `big_triples` (c just below 2^53, legs above 2^52) (4ec8dbc1, MSRV fix f6db12c2).
  plants2 (tree 522efc49) and plants3 (final code): 6/6 CAUGHT (plants3: see JOBS).
* Counts (kanon, FMA build; Liella ignores `glibc.cpu.hwcaps=-FMA`), inline vs out-of-line sqrt, glibc, and a
  wl_trace split (entry + mxcsr + kernel): atan 236.8 (ref 78.7), asin 325.1 (338.4 ool; ref 75.0), acos 293.0
  (310.0; ref 80.1), atan2 265.7 (ref 161.7), hypot 153.3 (159.4; ref 63.8). Inline sqrt kept (POSITIVE row).

## RED: gate3 (FULL tier, escalated by the generated s10_math sources): 5907 passed, 1 failed
`liella-boundary::s10_math_symbols math_double_invtrig_hypot_fast_tier_is_the_accurate_path`:
`acos(5e-324)` in direction 0: entry flags 0x20 (inexact), direct `mr::ret(trig::acos(..))` 0x30 (inexact +
underflow). The fast tier refuses subnormals, so `elem_fast::<HW_ANY>`'s fallback settles flags differently from
`mret::elem` (the pre-story acos path). Not caused by run -3 (the test was new in -1/-2 and never ran).

## Exact next step
1. Black-box probe glibc: flags of acos, asin, atan (and atan2/hypot with a subnormal operand) for 5e-324 and
   other subnormals, all four directions. If glibc raises underflow for acos(denormal), make the elem_fast
   fallback keep the accurate path's underflow (compare with `mret::elem`); if it does not, the accurate path's
   status is the defect (then: spec gap / differences check, RULING NEEDED if unclear). Then rerun only that
   suite: `cargo xtask affected --only liella-boundary:s10_math_symbols` (or the FULL gate if the tier insists).
2. Gate again on the final head; plants (new RAM dir) if code changes.
3. Performance: every row is 1.6-4.3x glibc; the design for 1.05x is in saturation §21 (finer table, single
   `double` quotient with the error in the bound, own asin/acos tables). A new story, after this one lands.

## Do not redo
* `_mm_sqrt_sd` in liella-math (E0133, probed in -2); the out-of-line vs inline sqrt counts (§21); plants 1-6.

## Jobs (kanon, ~/logs/<label>.log; scripts in ~/work/libm-f64-invtrig-hypot-tier-3/)
gate3 (rc 1, above), plants3, counts3 (both on code f6db12c2), trace2 (rc 0). Superseded: gate (static red),
gate2 (MSRV red), plants (2 survived), plants2 (tree before the MSRV fix, 6/6), counts/counts-ool (tree a5af1d2f,
product code identical; the source of §21's numbers), trace (no cargo env).

## Plan rows to read
D-10.13, D-10.22, D-10.25; original brief /home/claude/state/briefs/LIBM-F64-INVTRIG-HYPOT-TIER.md.
