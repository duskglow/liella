# LIBM-CPLX-LOG-SQRT handoff: clog/csqrt (and clogf, csqrtf) quick tiers

State: branch task/libm-cplx-log-sqrt-3 (continues task/libm-cplx-log-sqrt-2, origin/next merged at run 3), HEAD in
handoff/reports/LIBM-CPLX-LOG-SQRT-3.json. Role mei:~/w/s3. Decisions in force: D-10.53 (design), D-10.74 (this
story's record, records/decisions/D-10/D-10.74.toml: windows, bounds, Kahan 1987, Dekker 1971, de Dinechin-Lauter-Muller
2007), D-10.55, D-10.25. Full run-2 results (counts, timing, sweep): handoff/reports/LIBM-CPLX-LOG-SQRT-2.json and
the previous version of this note on task/libm-cplx-log-sqrt-2.

## Done

* Runs 1-2: code, proofs, tests, D-10.74, plants (6/6), gate, counts, booked timing, 10^7 sweep (green: every tier
  answer equals the 384-bit route in four modes).
* Run 3 (LIBM-CPLX-LOG-SQRT-3):
  * Merged origin/next (ruling #4253). Conflict in crates/math/src/invtrig.rs: next gave `atan2_nearest` a quick
    phase (`two_tier`), this branch had split out `atan2_abs` for clog. Resolved with one shared reduction
    `atan2_reduce`: `atan2_nearest` takes next's two-tier answer unchanged; `atan2_abs` keeps the accurate
    `atan_unit` sum that clog's bound (2^-65.5) assumes. The FMA build's `atan2_abs` copy is now unused
    (`allow(dead_code)` with the reason).
  * `cargo xtask codemap` (docs/codemap/symbols.tsv: line shifts from this branch's s10_math rows).
  * BENCH COMMIT (separate, #4269): cbench_libm rows `clogf_axis clog_axis csqrtf_axis csqrt_axis` (x in [-3, 3],
    y = +0 and -0 in turn) and `clogf_near1 clog_near1` (| |z| - 1 | log-uniform in [2^-52, 2^-1]); budget rows
    ratio_host, not gated. Not yet timed or counted.
  * #4270 route-entry count, test `bench_rows_reach_the_route_only_where_measured` (complex_fast_ls_tests.rs):
    cbench_libm's own operand sets through each in-place tier and the mode tier behind it, as `cplx_fast`
    dispatches them. [declined in place, reached the 384-bit route] of 256:

    | fn | plain square [-3,3]^2 | _axis | _near1 |
    |---|---|---|---|
    | cexp | 1, 1 | 256, 256 | 0, 0 |
    | clog | 0, 0 | 256, 256 | 10, 10 |
    | csqrt | 0, 0 | 256, 256 | 0, 0 |
    | cexpf | 0, 0 | 256, 256 | 0, 0 |
    | clogf | 0, 0 | 256, 256 | 0, 0 |
    | csqrtf | 0, 0 | 256, 256 | 0, 0 |

    VERDICT on the hypothesis: refuted for csqrt (no bench operand leaves the tier, so its 3.9x-by-time vs
    2.1x-by-instructions gap is in the tier itself: latency of the dependent sqrt -> div -> sqrt -> div chain is
    the remaining suspect). For cexp, exactly one operand of 256 reaches the route; whether that one call carries
    the time gap is NOT VERIFIED (needs a per-operand count/time of that operand). Every exact-zero part goes to
    the route in all six functions (cexp too: `cexp(x + 0i)` is a real exp passed as complex).

## Results (run 3)

* Lint `libm-cplx-log-sqrt-3-check` (mei, head 74ac7a459 content): `cargo xtask clippy` + `static` green, the
  route-count test passes, the bench compiles (`gcc -Wall -Wextra -fsyntax-only`).
* BLOCKER, NOT FIXED: the merged tree's products do not build. `cargo xtask so` (fast profile) fails its hot-text
  check: `hot-text.list: 2 key(s) match no function section of the fast-profile libc.a`:
  `Features::detect<CSysImpl<false>>` (startup tier) and an intl catalog closure (extra tier). origin/next alone
  builds clean (job `libm-cplx-log-sqrt-3-nextso`, scratch copy, rc 0), and this branch was green before the merge,
  so the combination does it: this branch's extra boundary code (s10_math rows, hot csqrt entries) changes rustc's
  inlining of those two generic functions. Gate `libm-cplx-log-sqrt-3-gate` (rc 1: 6 product suites fail with
  "the products are missing") and plants `libm-cplx-log-sqrt-3-plants` (BASELINE FAILED at `so`, no plant applied)
  both stop there.
* Tried the tool's remedy, `tools/hottext/regen.sh . --build` then `LIELLA_FULL_PROFILE=1 ... --build` (job
  `libm-cplx-log-sqrt-3-hottext`, rc 1): the fast regen rewrote the list (keys reordered, several stdio keys dropped:
  `liella_internal_memcpy`, `internal_move`, `internal_large`, `liella_machine_copy_avx2`); the release regen then
  failed `STARTUP-TEXT-WINDOWS BROKEN: start-up text spans 5 windows > budget-release 4` and flagged one stale
  hot-text.absent release line. Nothing from it is committed (the role's hot-text.list is modified; the next sync
  restores it).

Next run, step 0 (before anything else): make the merged products build. Options, in order of preference: (a) find
why the two functions are inlined now (look for the caller that gained them; `#[inline(never)]` on
`Features::detect` would make the key build-stable if the layout owner agrees: RULING NEEDED); (b) regen in both
profiles, then fit start-up text into the release budget (ALWAYS FORWARD: never raise budget-release); (c) a
hot-text.absent line only if the absence is by design. Then rerun the gate (`--files` as below) and the plants
(handoff/LIBM-CPLX-LOG-SQRT-3-plants.txt, 4 plants, not yet run). Gate files: `git diff --name-only
origin/next...HEAD` minus records/ and handoff/ (records/ escalates `affected` to FULL).

## Next steps (in order)

1. LIBM-CPLX-AXES (new story, now the biggest lever by the table above): exact-zero parts answered in the in-place
   tier for cexp/clog/csqrt and the float variants: `csqrt(x ± 0i)` = `(sqrt x, ±0)` for `x > 0`,
   `(+0, ±sqrt(-x))` for `x < 0`; `clog(x ± 0i)` = `(log|x|, ±0 or ±π)`; `cexp(x ± 0i)` = `(exp x, ±0)`; the
   `x = ±0` axis likewise. Annex G signs; flags from the real function (exact cases raise no inexact). Measure with
   the new `_axis` rows and the percall `_axis` cases.
2. csqrt by time (3.9x): profile `csqrt_bench` per phase (two `sqrt_dd`, `div_b`, two `round_root`) and its
   dependency chain; the route is not involved (table above).
3. cexp: per-operand cost of the one routed bench operand (instructions and time) to settle whether it carries the
   5.4x-by-time row; if so, an intermediate double-double tier for cexp's hard cases.
4. clog `_near1`: 10 of 256 reach the route (|z| within ~2^-40 of 1 or a hard rounding); time the new rows.
5. Continuations: LIBM-CPLX-TRIG (csin/ccos), LIBM-CPLX-POW, then the long double variants.

Do not redo: the bound derivations in complex_fast.rs, the atan2_abs split / atan2_reduce merge, the float margins,
the plants (handoff/LIBM-CPLX-LOG-SQRT-2-plants.txt, -3-plants.txt), the route-count table.
