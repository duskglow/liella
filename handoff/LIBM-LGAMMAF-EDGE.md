# LIBM-LGAMMAF-EDGE handoff (run 2, LIBM-LGAMMAF-EDGE-2: PENDING JOBS)

State: branch `task/libm-lgammaf-edge-2`, stacked on LIBM-F32-BESSEL-LARGE-7's head af1c82203 (BESSEL-LARGE-4's
table method; REST-NEXT-3's `lgamma_edge` is on next and in the base: both prerequisites checked). Code head
1d1a7e435 (3 commits); later commits are handoff/ only. Role shiki:~/w/sh2. Decision: pending record
records/pending/LIBM-LGAMMAF-EDGE-2-1.toml (series D-10; id to be assigned). Spec: ~/state/briefs/LIBM-LGAMMAF-EDGE.md
plus the -2 brief's corrections.

## Done
- `lgamma_reflect` (crates/math/src/fast32/gamma.rs), reached from the cold `lgamma_edge` for x < -42: reflection in
  double, exact reduction r = x - 2·nearest(x/2), sin πt by the Bessel band's octant Taylor polynomials (now
  pub(super) in bessel.rs), Stirling with ln64. Proved bound 8.2 ulp of y (doc comment), margin 16; measured worst
  3.0 ulp on 4459 operands. Sign from r (no integer conversion, D-10.95).
- `lgamma_rest` (cold, out of line, only on a rounding-test refusal): for margin 11 264 (2^-8 ≤ |y| < 1) the
  `lgamma_edge` kernel as the second try, then the table for the reflection and that piece.
- Table crates/math/src/fast32/lgamma_tab.rs: 18 entries, all below -42, WRITTEN by
  `lgammaf_table_is_the_refused_set` (LIELLA_LGAMMAF_TABLE_OUT=<file>; long-sweeps row added). The enumeration found
  every small-|y| refusal decided by the second try (49 in [-2.6, -2.46] alone).
- Oracle (oracle32.rs `lgamma_unknown`): reflection now covers -2^23 < x < -64, so the exhaustive sweep checks the
  new operands without mp; `lgamma_oracle_decides_integers_and_large_operands` updated (-64.5 is now decided).
- Tests: lgamma_reflection_is_within_its_bound, lgammaf_below_minus_42_takes_the_fast_path, lgammaf_table_entries,
  lgammaf_small_refusals_take_the_second_try, gamma_margins row. Unit run (release) 12 passed; enumeration matched
  (jobs libm-lgammaf-edge-2-gen3). Lint: xtask clippy, workspace all-features clippy, static all rc 0 (lint3).
- Percall rows added to validation/percall/lgammaf.c: lgammaf_big, lgammaf_tab, lgammaf_retry.

## Pending (started detached on shiki at 09:38Z; collect with `~/bin/jobwait <label>` over ssh)
libm-lgammaf-edge-2-gate (affected --run --base origin/next), libm-lgammaf-edge-2-plants (9 plants,
handoff/LIBM-LGAMMAF-EDGE-2-plants.txt), libm-lgammaf-edge-2-sweep (exhaustive lgammaf, 2^32 x 4 modes, plus the
table enumeration), libm-lgammaf-edge-2-percall (lgammaf.c: typical rows pos/neg/r_neg/zero against base, plus the
three new rows). NOT VERIFIED until these are green.

## Open / next
- If the sweep or a plant is red: fix, rerun only that job. If percall shows a typical row slower: the change to
  `eval_lgamma`'s refusal branch is the only hot-path edit (a call to the cold `lgamma_rest` in place of `?`).
- Follow-up (not this story): `lgamma_edge`'s own refusals (608 operands: 594 at |x| < 2^-60 with margin 72 against a
  proved 2.04 ulp, 14 in 2^-60..2^-20) still take the accurate path; a margin of 5 for the tiny piece would cut 594 to
  about 40, which a table could then hold.
Not to redo: the prerequisite check, the error analysis, the enumeration (current for margin 16).
