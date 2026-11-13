# LIBM-RETHINK-BESSEL handoff (run 2: LIBM-RETHINK-BESSEL-2)

State: branch `task/libm-rethink-bessel-2`, code head 9d9f58f63 (on next c32adc349; this note and the report json
are the commits after it). Decision in force: D-10.106 (f32 j0f/j1f/y0f/y1f below 2^-20 from the leading series
terms; body corrected in run 2: errno rule). Role mei:~/w/s1. Report: handoff/reports/LIBM-RETHINK-BESSEL-2.json.

## What run 2 found (run 1's jobs, keke)
- gate rc 1: `cargo xtask static` (records: bare `|` in D-10.106), fixed by run 1's 9d2108944. Its `affected` stage
  never ran, so run 1's code was never gated. Step 0 on mei: clippy and static clean on 9d2108944.
- percall rc 2: `--base c32adc349` is not a 40-hex sha. Rerun with c32adc3499f1318e0524f767220dd21c6e8d82ae.
- sweeps: j1f and y1f FAILED on errno; plants: baseline test failing (below). Both ended or were stopped by run 2.

## Fixes (run 2)
- 94cf124ab: the tiny band set ERANGE for any underflow or overflow (`Status::range_errno`). Spec
  (spec/behaviour/libm-exact.md, and the recorded probes in libm-special.glibc.txt.gz: `j1f U 00000001 = 00000001
  ---UX 0`, `y1f U 00000001 = ff7fffff --O-X 0`, `y1f N 00000001 = ff800000 --O-X ERANGE`): ERANGE only for an
  underflow to zero or an overflow to -inf. J1 now checks m == 0, Y1 the directions that round to -inf (cheap local
  tests, the rule of `crate::float::range_errno`); D-10.106's body states the rule.
- 9d9f58f63: `bessel_tiny_band_agrees_with_the_accurate_path` compared the statuses' Debug text, which includes
  `Status::hw` (a hint: any superset is correct; `finish` reports inexact + denormal, the accurate path every bit), so
  it failed on Y0(0x1p-149) with equal value, flags and errno. It now uses Status's equality (flags, errno), as the
  sweeps do.

## Counts (wl_percall, instructions per call, Skylake-SP, deterministic; glibc dynamic; base = next c32adc349)
| case | glibc | base | new 9d9f58f63 | new/glibc |
|---|---|---|---|---|
| bes_j0f_tiny | 54.3 | 6401.4 | 94.0 | 1.73 |
| bes_j1f_tiny | 67.8 | 16447.4 | 119.7 | 1.76 |
| bes_y0f_tiny | 98.2 | 148628.2 | 340.9 | 3.47 |
| bes_y1f_tiny | 64.5 | 152382.9 | 313.6 | 4.86 |

Before the errno fix (9d2108944, never landed): j1f 123.9, y1f 305.5; the +8.1 on y1f is not analysed (NOT
VERIFIED where it goes). Self-check MISMATCH on y0f/y1f(1e-10f): base and new agree and differ from glibc in the
last place; by hand Liella's are the nearest floats (Y1: true -6366197638.7, ulp 512, Liella -6366197760, glibc
-6366197248). Pre-existing, not from this change.

## Jobs on 9d9f58f63 (mei)
libm-rethink-bessel-2-gate3 rc 0 (clippy, static, affected --base origin/next: 6 suites ok, 1 skipped, 1571 passed,
0 failed; peak 3.3G), -plants2 rc 0 (handoff/LIBM-RETHINK-BESSEL-2-plants.txt: 8 CAUGHT, 0 SURVIVED, 1 EQUIVALENT,
no WARN), -percall3 rc 0 (the table above), -sweeps2 PENDING at hand-back (exhaustive j0f/j1f/y0f/y1f against the
oracle from 2^-149; y0f done clean, the rest running with no failure).

## Next step
1. If -sweeps2 is red: read its panics (`grep -A3 panicked ~/logs/libm-rethink-bessel-2-sweeps2.log` on mei).
2. Then the follow-up briefs, in order: handoff/LIBM-RETHINK-BESSEL/LIBM-F32-BESSEL-LARGE.md (rank 4),
   handoff/LIBM-RETHINK-BESSEL/LIBM-F64-BESSEL-BANDS.md (ruling #4953). The y1f +8 can go with the first (same function).
Not to redo: the probe (run 1's before data); the series derivation (D-10.106); the errno rule (settled by the probes).
