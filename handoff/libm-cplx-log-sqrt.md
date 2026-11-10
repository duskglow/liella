# LIBM-CPLX-LOG-SQRT handoff: clog/csqrt (and clogf, csqrtf) quick tiers

State: branch task/libm-cplx-log-sqrt-5 (continues -4, -3, -2, -1; origin/next merged at run 3, not since; next
9af6dff86 unchanged since). Code head 12835f458; HEAD in handoff/reports/LIBM-CPLX-LOG-SQRT-5.json. Role mei:~/w/s1
(Skylake-SP). Decisions in force: D-10.53, D-10.74 (now also records the axis paths), D-10.55, D-10.25, D-X.56,
HOTTEXT-PINNING (no `#[inline(never)]` on start-up keys), coordinator ruling #4357, owner ruling #4347 (LAYOUT-JITTER).

## Done in run 5

* Step 0: clippy + static green on the inherited head (job libm-cplx-log-sqrt-5-step0).
* Hot-text repair (#4357), commit 12835f458: `tools/hottext/prune.py` with the fast and release
  `hot-text.matched` of this tree dropped exactly the two dead keys (`Features::detect<CSysImpl<false>>`, startup,
  fast; intl `catalog::variants::each` closure, extra, fast). Neither matches in the release archive either, so the
  layout is unchanged in both profiles; list order kept. `cargo xtask so`: release 4 windows (budget 4), hot group
  0x413280-0x4a204c (same as next), 101/101; fast 5 windows (budget 5), 106 of 107 (Features::detect inlined).
  `records check` ok (job libm-cplx-log-sqrt-5-so2).
* D-10.74 extended with the axis paths (Annex G.6.3.1/G.6.3.2/G.6.4.2, Figueroa 1995) and its test reference (no new
  id needed: D-10.74 is this story chain's own, not yet landed).
* Plants: handoff/LIBM-CPLX-LOG-SQRT-5-plants.txt (the -3 and -4 files, 14 plants): 14 CAUGHT, 0 WARN
  (job libm-cplx-log-sqrt-5-plants).
* Counts (job libm-cplx-log-sqrt-5-percall, instructions/call, Liella vs glibc dynamic, Skylake-SP): csqrtf 107.0 vs
  127.5, csqrt 307.2 vs 146.2, csqrt_axis 85.0 vs 58.0, clogf 275.1 vs 274.4, clog 527.0 vs 417.6, clog_axis 288.9 vs
  199.6. No base column (base = the 384-bit route for every axis operand, run 3's route count).
* Timing (Ivy Bridge timing host kotori, booking 152, staged tree of 12835f458, job kotori:libm-cplx-log-sqrt-5-timing,
  ns/op glibc vs Liella, ratio): cexp 75.7/404.3 5.34; clogf 138.0/131.2 0.95; clog 158.3/241.1 1.52; csqrtf
  90.1/96.4 1.07; csqrt 64.6/220.3 3.41; clogf_axis 110.6/107.3 0.97; clog_axis 61.9/112.9 1.82; csqrtf_axis
  45.0/55.0 1.22; csqrt_axis 16.2/23.0 1.42; clogf_near1 135.3/121.4 0.90; clog_near1 489.8/432.2 0.88.

## BLOCKER: LAYOUT-JITTER (owner ruling #4347; not chased)

Gate libm-cplx-log-sqrt-5-gate (escalated to FULL because hot-text.list changed; 6317 passed, 3 failed): three
start-up fault bounds, each +1: coreutils-shaped tool 59 > 58, fork child 8 > 7, setlocale+printf 56 > 55 (startup
only 17, bound 17). The repair places nothing new (dead keys), so this is the libm code growth of runs 1-4 moving the
layout. Same three tests, fast products, plain jobrun on mei, deterministic (5 equal runs each):

| count (bound) | origin/next 9af6dff86 (job libm-cplx-log-sqrt-5-nextfaults) | branch 12835f458 (job libm-cplx-log-sqrt-5-branchfaults) |
|---|---|---|
| startup only (17) | 16 | 17 |
| setlocale + printf (55) | 52 | 56 |
| fork child (7) | 7 | 8 |
| coreutils-shaped tool (58) | 55 | 59 |

+4 on locale and tool is above the 1-3 the ruling names, and above next's own pad-sweep range in
handoff/layout-stability-rethink.md (locale 52-55, tool 55-58): see RULING NEEDED in the report. Hot-text inputs vs
next: only the 2 pruned lines; next has one more line in layout/contracts.tsv (added on next after run 3's merge).
No bound changed, no regen loop, no section moved.

## Next steps (in order)

1. When LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS land: merge origin/next, `cargo xtask so` both profiles, rerun the
   gate (`--files` = `git diff --name-only origin/next...HEAD` minus records/ and handoff/; it escalates to FULL while
   hot-text.list differs from next). Plants and counts above stand unless the merge changes the math code.
2. Then run 3's list: csqrt by time (3.41x: dependent sqrt/div chain), cexp (5.34x; one routed operand), clog
   (1.52x) and clog_axis (1.82x), the cexpf / `cexp(±0+iy)` axes.

Do not redo: bound derivations in complex_fast.rs, the atan2 merge, the route-count table, the release diagnosis
of run 4, the hot-text repair (prune of the two dead keys), the plants.
