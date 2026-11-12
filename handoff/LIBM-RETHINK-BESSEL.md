# LIBM-RETHINK-BESSEL handoff (run 1)

State: branch `task/libm-rethink-bessel`, head 285b4a1dd (code db3ad7964 + test/record), on next c32adc349. Decision in
force: D-10.106 (new; amends D-10.49's range). Role keke:~/w/r5. Context budget hit at ~165k of 180k: hand-back
PENDING JOBS, with the f32 large band and the f64 bands left as follow-up briefs (below).

## Evidence (LIBM-TAIL-PROBE, before)
f32 per binade (Liella / glibc mean): tiny |x| < 2^-20: j0f 736-19253 / 41, j1f 14669-18703 / 55, y0f 146925 / 83,
y1f 150626 / 51; mid [2^-20, 2^19): 152-288 / 94-348 (already at or under glibc, D-10.49); large >= 2^19:
1.67e5-2.87e5 / 269-356. f64: |x| >= 32 2705 / 830; y0 x < 9.3e-302 1.38e5 / 89; y1 x < 5.6e-17 4466 / 41; j0, j1
tiny 469-504 / 37-50; the probe's hard points near zeros (2.40, 3.83, 0.89, 2.20 ...) 5e4-2.2e5 / 416-525.
Path (from the code, not traced): D-10.49's `bessel_core` returns None outside [2^-20, 2^19) (`BES_MIN`; `trig_core`
stops at 2^19), so `eval` falls to `special::{j0..y1}` -> `*_accurate` (multi-precision). The wl_trace job
(libm-rethink-bessel-before) ended rc 2 (role synced dirty: spec uncommitted) and was not rerun: NOT VERIFIED by trace.

## Design (RETHINK)
Tiny band, two approaches: (A) leading series terms in closed form (DLMF 10.2.2, 10.8.1-2), (B) extend D-10.49's
fits to 2^-149. B keeps an absolute bound (useless for J1/Y1 relative accuracy) and refuses on subnormal or
overflowing results. A adopted: J0/J1 from brackets (no rounding test), Y0/Y1 via the correctly rounded double log
with 8u/4u relative margins, Y1 overflow and J1 subnormal results by bits. Expected cost ~15 (J) to ~60 (Y) instr.
Large band, two approaches: (A) modulus-phase J = M cos θ, Y = M sin θ (DLMF 10.18.17-18) with an exact fixed-point
Payne-Hanek reduction of x·2/π (24-bit mantissa x a 128-bit window of 2/π, subtract (2n+1)/2 in fixed point, one
sin or cos of |t| <= π/4 + 3/(8x)), (B) Hankel P/Q (D-10.49's form) with the same reduction. B cancels near zeros
(P cos - Q sin), so its refusals stay ~5e-6; A keeps the phase exact to ~2^-70 absolute, relative error ~2^-50,
so ~30-60 refusals per function: an exception table, no MP. A recommended, estimate 70-100 instructions vs glibc 280.

## Jobs on head 285b4a1dd (keke, started detached; PENDING)
libm-rethink-bessel-gate (clippy, static, affected --base origin/next), -percall (bands, base c32adc349 vs head vs
glibc), -sweeps (exhaustive j0f/j1f/y0f/y1f against the oracle, now from 2^-149), -plants (5: 4 CAUGHT expected,
1 EQUIVALENT: margin halved, reason in the plants file).

## Next step
1. Collect the four jobs: `~/bin/jobwait libm-rethink-bessel-gate libm-rethink-bessel-plants libm-rethink-bessel-percall libm-rethink-bessel-sweeps` on keke.
   A sweep mismatch on status (underflow ERANGE for j1f subnormals) is the likeliest red.
2. Follow-up briefs: handoff/LIBM-RETHINK-BESSEL/LIBM-F32-BESSEL-LARGE.md, handoff/LIBM-RETHINK-BESSEL/LIBM-F64-BESSEL-BANDS.md.
Not to redo: the probe (its data is the before); the tiny-band series derivation (D-10.106).
