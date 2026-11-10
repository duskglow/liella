# LIBM-CPLX-LOG-SQRT handoff: clog/csqrt (and clogf, csqrtf) quick tiers

State: branch task/libm-cplx-log-sqrt-4 (continues -3, -2, -1; origin/next merged at run 3, not since). HEAD in
handoff/reports/LIBM-CPLX-LOG-SQRT-4.json; code head 421f9dea7. Role keke:~/w/r5 (Zen 1); hot-text work on mei
(Skylake-SP, the CI class) in RAM copies under /mnt/zram/claude/LIBM-CPLX-LOG-SQRT-4/. Decisions in force: D-10.53,
D-10.74 (records/decisions/D-10/D-10.74.toml), D-10.55, D-10.25, D-X.56 (hot-text regen), HOTTEXT-PINNING (owner:
no `#[inline(never)]` on start-up keys). Runs 1-3: see handoff/reports/LIBM-CPLX-LOG-SQRT-{2,3}.json.

## Done in run 4

* Step 0: `cargo xtask clippy` + `static` green on the inherited head (job libm-cplx-log-sqrt-4-step0); its
  `cargo xtask so` fails as run 3 said (2 dead fast-profile keys).
* AXES (coordinator #4305), commit e5bb15487 (+ 421f9dea7 test lint fix and plants file): the in-place entries
  answer exact-zero-part operands, out of line, only after the general tier declines (general path unchanged):
  `csqrt(x±0i)` = hardware root (`(√x,±0)` / `(+0,±√|x|)`), `csqrt(±0+iy)` = `(t, ±t)`, `t = √(|y|/2)`;
  `clog(x±0i)` = `(fast64::log_nearest|x|, ±0 or ±π)`, `clog(±0+iy)` = `(ln|y|, ±π/2)`, `clog(1±0i)` exact, other
  `|v| = 1` decline; `cexp(x±0i)` = `(exp_nearest x, ±0)` (both FMA builds), `cexp(±0±0i)` = `(1, ±0)`; `clogf`
  via `clog_core`'s formula with `b = 0` (exact `a²`, `(a-1)(a+1)`), `csqrtf` via the double root (Figueroa 1995,
  `53 >= 2·24+2`). Not done: `cexpf` axis, `cexp(±0+iy)` (needs sincos), the zero/subnormal cases (route).
* Tests: boundary `complex_axes_in_place_match_the_route` (s10_math_symbols.rs: bits, flags, errno of the
  exported functions vs the 384-bit route, both axes, 21 magnitudes x signs x signed zeros, 6 functions); route
  count now asserts `_axis` `[0, 0]` for cexp, clog, csqrt, clogf, csqrtf. Job libm-cplx-log-sqrt-4-step1b (keke,
  head 421f9dea7): clippy, static, `complex_fast` lib tests (10 pass) and the axis test pass.
* Plants file handoff/LIBM-CPLX-LOG-SQRT-4-plants.txt (10 plants): NOT RUN (the tool's baseline needs `so`).

## BLOCKER (not fixed, numbers for LAYOUT-STABILITY-RETHINK / coordinator)

Clean env (`env -i`, no LC_*), Skylake-SP, RAM copies of this branch:

| build | list | result |
|---|---|---|
| origin/next 9af6dff86 release | next's | ok: 4 windows (budget-release 4), hot group 0x413280-0x4a204c, 101/101 start-up fns |
| origin/next fast | next's | ok: 5 windows (budget-fast 5), 107/107 |
| branch release | inherited | ok: 4 windows, hot group 0x413280-0x4a204c, 101/101 (same as next) |
| branch fast | inherited | FAIL: 2 dead keys `Features::detect<CSysImpl<false>>` (startup), intl `catalog::variants::each` closure (extra) |
| branch fast, `regen.sh . --build` | regenerated | ok: 5 windows (budget-fast 5), 106/106; list 588 lines: 21 removed, 20 added |
| branch release, `LIELLA_FULL_PROFILE=1 regen.sh . --build` (after the fast regen) | fast-regenerated | FAIL at its first build: STARTUP-TEXT-WINDOWS 5 > budget-release 4, hot group 0x413280-0x4a1f8c |

So the branch's code does not push release start-up text over budget; the fast regen's rewritten list does
(it reorders/re-tags keys, which moves the release layout). The fast-regenerated list is kept on mei at
~/work/LIBM-CPLX-LOG-SQRT-4/hot-text.list.fast-regen. Per the coordinator note, stopped here: no budget raised,
no `#[inline(never)]` added. Jobs: libm-cplx-log-sqrt-4-regen2 (rc 1, the table's last two rows),
libm-cplx-log-sqrt-4-reldiag (rc 0 by design, rows 1-4). Recommendation (RULING NEEDED): repair only the two
dead fast keys (as the fast regen does for them) and keep the rest of the list's order, or make regen's merge keep
the existing order for keys traced in the other profile; that keeps release at 4 windows.

## Next steps (in order)

1. Unblock `so` (ruling above), then gate (`--files` = `git diff --name-only origin/next...HEAD` minus records/
   and handoff/), plants (handoff/LIBM-CPLX-LOG-SQRT-4-plants.txt and -3-plants.txt, 4 not yet run), counts of the
   `_axis` rows (cbench_libm `clog_axis csqrt_axis clogf_axis csqrtf_axis`) vs glibc.
2. Decision record for the axis paths (`records new decision D-NEW --series D-10`, cite Figueroa 1995, C Annex
   G.6.3.1/G.6.3.2/G.6.4.2), or an `amends` of D-10.74.
3. Then run 3's list: csqrt by time (dependent sqrt/div chain), cexp's one routed operand, clog `_near1`, and the
   cexpf / `cexp(±0+iy)` axes.

Do not redo: bound derivations in complex_fast.rs, the atan2 merge, the route-count table, the release diagnosis above.
