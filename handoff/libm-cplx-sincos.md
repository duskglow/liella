# LIBM-CPLX-SINCOS handoff: csinh/csin (double, float) quick tier, overflow step and middle tier

State: branch task/libm-cplx-sincos-4 (= -3 + merge of origin/next 163707549.., merge f07917c35; one conflict in
xtask/src/longsweeps.rs, resolved by keeping the csinh sweep line and next's new sweep entries). Role kanon:~/w/kn1.
Decisions in force: D-10.53 (tier structure), D-10.25 (FMA builds), D-10.99 (cexp's middle tier, reused).
Pending: records/pending/LIBM-CPLX-SINCOS-2-1.toml (D-NEW, series D-10; body now covers run 4; the coordinator assigns).

## Run 4

* Step 0 (inherited head 463c61f7a): libm-cplx-sincos-4-check0 rc 0 (xtask clippy, static).
* Product build still red after the merge, on ONE key: libm-cplx-sincos-4-check1 and -so1 (rc 1). `cargo xtask so`:
  "hot-text.list: 1 key(s) match no function section of the fast-profile libc.a: `_RNvNtC15liella_boundary7catalog10textdomain`
  (extra, no trace)". `cargo xtask hot-lists` (the train's regeneration) fails on the same key: regen.sh --allow-drop drops
  only trace keys (it dropped one release key, global_locale::setlocale), an "extra" seed key stays, and the recheck
  refuses ("the rewritten list fails the grouping check"; STAMP not written). So HOTLIST-TRAIN-REGEN-4 does not cover
  this case; the seed-key split is HOTLIST-TRAIN-REGEN-3 (handoff/HOTLIST-TRAIN-REGEN-2.md). No workaround here
  (coordinator #5111): no hot-text.absent line, no prune, no list edit from this branch.
* Code (crates/math/src/complex_fast_sinh.rs, comment block "After a quick decline"): the envelope tier (`csinh`, `csin`;
  the in-place `*_nearest` entries unchanged) answers a declined part by (1) the overflow value when every value within
  its bound is >= 2^1024 (±inf or ±DBL_MAX by sign and mode; OVERFLOW|INEXACT, ERANGE only with an infinite part: the
  route's report, found by the status assertions), else (2) the middle tier: `exp_dd(a)` and `exp_dd(-a)` (2^-100 each;
  a second exp_dd instead of the designed reciprocal: no new proof, cold path), all-positive series below 2^-3, cexp's
  `sin_or_cos` with the relative bound 2^-99·|T| + 2^-137 (derived in the comment, checked against 384-bit), eps =
  A·(B + 2^-40)·2^-96, `middle_round`. Both are `#[cold] #[inline(never)]` (`rescue`).
* Tests: the sweep and hard cases now compare the tier's status with the route's and accept an in-place decline;
  new `csinh_tier_answers_overflowing_parts_as_the_route`, `csinh_middle_tier_bounds_and_answers` (factor and part
  bounds against mp; forced middle answers equal the route in 4 modes).
* libm-cplx-sincos-4-test3 (rc 0, head 8f725ca1f): xtask clippy, static, `complex_fast` lib tests 19 passed, sweep.

## Decline rates: sweep 10^6 per regime, nearest answers (every answer in 4 modes equals the route, status too)

| regime | before (run 2) | after (run 4) |
|---|---|---|
| 0 typical [-3,3]² | 999729 | 1000000 |
| 1 series | 999676 | 1000000 |
| 2 cancellation [1/8,1] | 999663 | 1000000 |
| 3 large x [30,746] | 950661 | 1000000 |
| 4 y near kπ/2 | 896581 | 1000000 |

Same for csin. Target <= 2^-20 declines in every regime: met (0 of 10^6).

## Next steps (in order)

1. RULING NEEDED: the extra key catalog::textdomain blocks `xtask so` on this branch (above). Land the seed-key split
   (HOTLIST-TRAIN-REGEN-3), or rule a hot-text.absent line here. Then rerun `cargo xtask hot-lists && cargo xtask so`.
2. Gate (`cargo xtask affected --run`) once so builds; percall validation/percall/cplx_sin.c against next and glibc
   (the in-place nearest path is unchanged; only cold code was added; not measured this run).
3. ccos/ccosh on the shared kernel (ccos(z) = ccosh(iz)); abs(t) < 2^-30 and >= 2^20 in the tier; FMA build.

Do not redo: the quick-tier design and its bounds, the middle tier and overflow step, run 3's diagnosis.
Plants: handoff/LIBM-CPLX-SINCOS-4-plants.txt (9 plants, written, NOT RUN): libm-cplx-sincos-4-plants3 (rc 2) stopped at
its baseline `cargo xtask so` on the same hot-text key; the tool has no lib-only mode. Run it (and the -2 file) once
`so` builds. Earlier tries: plants (a `|` in a replacement: refused), plants2 (records body newline). Records check: ok.
