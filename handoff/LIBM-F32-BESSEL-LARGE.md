# LIBM-F32-BESSEL-LARGE handoff (run 1, context checkpoint)

State: branch `task/libm-f32-bessel-large`, code head 22bd8683c (on next fe4c4893d; this note and the report json
follow it). Role shiki:~/w/sh1. Reserved decision id: D-10.109 (record NOT written yet). Brief:
~/state/briefs/LIBM-F32-BESSEL-LARGE.md (= handoff/LIBM-RETHINK-BESSEL/LIBM-F32-BESSEL-LARGE.md).

## What is done (crates/math/src/fast32/bessel.rs, bessel_tab.rs, oracle32/bessel.rs)
- `bessel_eval` sends 2^19 <= |x| < 2^128 (Y: x > 0) to `bessel_large`: modulus and phase (DLMF 10.18.17-18),
  phase in quadrants formed exactly in fixed point: 24-bit significand x the 128 bits of 2/pi ending at bit E-24
  (`coeffs::tables::TWO_OVER_PI`, existing verified words), wrapping u128 product in units of 2^-126, minus
  (n + [Y]) quadrants (Y_n = M cos(theta - pi/2)), plus delta' = (2/pi)[(mu-1)/(8x) + (mu-1)(mu-25)/(384x^3)]
  at 2^-84; top two bits = quadrant, rest g in [-1/2, 1/2]; Taylor sin(pi g/2)/g (k <= 8) or cos (k <= 9);
  M^2 = (2/(pi x))(1 + (mu-1)/(8x^2)), one sqrt. Bound LG_B = 8 units of 2^-53 relative (+ (3e-25 + 5u|dq|)/|g|
  on the sin branch), turned into ulps of y by y's significand (`large_delta`), into `round`.
- Refused operands: table `bessel_tab.rs` (54 j0f, 47 j1f, 44 y0f, 57 y1f), written by the ignored exhaustive
  test `bessel_large_table_is_the_refused_set` (40 s on shiki; prints the table on a mismatch: regenerate with
  `python3 -I mktab.py` from the log, see run notes below). Lookup = binary search, answer rebuilt as F +- 2^-28 F
  through `finish` (all modes, flags inexact). No runtime MP on the band (None is unreachable: the enumeration).
- Oracle: `bessel_est` now covers up to FLT_MAX; from 2^19 `large_reduce` does the same fixed-point reduction with
  its own 2/pi bits from the 384-bit mp constant (independent of the fast path's table); r = g.pi/2 in Dd, cut
  bound 4e-31 added to `err`.
- Tests: `bessel_large_core_is_within_its_bound` (stride over all binades + near-zero operands near 2^19 against
  the 384-bit values; oracle checked there too; worst error/bound 0.41-0.45), `bessel_large_taylor_coefficients`,
  `bessel_large_table_entries` (every entry refused by the core and equal to the accurate path in every mode, +-x).
- Plants: handoff/LIBM-F32-BESSEL-LARGE-plants.txt (table entry removed, margin halved, x^-3 term dropped).

## Jobs (shiki)
On code head c33609c90, verdicts NOT read: libm-f32-bessel-large-gate2 (clippy, static, affected --base
origin/next), -sweeps2 (waits for gate2, then the four exhaustive oracle sweeps + the enumeration).
Superseded: -gate rc 1 (clippy::panic in the new bound test, fixed in c33609c90), -plants rc 2 (BASELINE FAILED
in `cargo xtask so`: "hot-text.list: 1 key(s) match no function section of the fast-profile libc.a", a vdso
`lookup_many` key in liella_boundary; not touched by this change, not analysed: compare on origin/next before any
regen), -sweeps (stopped to sync), -enum (compile error), -enum2 (rc 101 by design: printed the table; the
other three large tests ok). Plants still to run once `so` builds.

## Next step (continuation)
1. Read the three jobs (`~/bin/jobwait <label>` on shiki; `grep -E "panicked|SURVIVED|WARN" ~/logs/<label>.log`).
2. Write D-10.109: `cargo xtask records new decision D-10.109` (method above; cite DLMF 10.17.3, 10.17(iii),
   10.18.17-18; Payne-Hanek via Muller, Elementary Functions ch. 11; error budget: g 1u, pi/2 0.35u, poly sums
   ~2.3u, coefficients < 1u, M 2.28u, product 1u -> < 8u), then `cargo xtask records check`.
3. Counts: `wl_percall.sh --spec validation/percall/libm_bessel.c .` rows bes_{j0f,j1f,y0f,y1f}_large, base =
   next fe4c4893d (full sha), against glibc 275-298 (LIBM-TAIL-PROBE rank 4): done = within 2x.
4. Update the module comment of oracle32/bessel.rs (still says the range ends at 2^19).
Not to redo: the enumeration (table is current for LG_B = 8); the design (bound holds at 0.45 of B).
