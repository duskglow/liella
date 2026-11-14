# LIBM-CPLX-SINCOS handoff: csinh/csin (double, float) quick tier, overflow step and middle tier

State: branch task/libm-cplx-sincos-7 = -6's head 288a2788e + merge of origin/next 1700e1798 (1dd7bc84c; brings
LIBM-CPLX-POW-6) + 69f3c3967 (stale fast textdomain absent line dropped) + this handoff/report. Role mei:~/w/s1.
(-6's branch: -5's head 399f1a6e0 + merge of origin/next e30374455, HOTTEXT-TIME-CLUSTER-STABLE-3.)
(-5's branch: -4's head 722ba663c + 50bb860cf "Allow the catalog::textdomain hot-text key to be absent in the fast
profile" + merge of origin/next 5e5ec2a57 + e5dc20754 "Pin the csinh overflow step's threshold at 2^1024".)
Decisions in force: D-10.53 (tier structure), D-10.25 (FMA builds), D-10.99 (cexp's middle tier, reused).
The runs 2-4 record (pending in -6) now has the reserved id D-10.119 (288a2788e).

## Run 7: merge only (LIBM-CPLX-POW-6 landed in complex_fast.rs)

* Step 0 (inherited head 288a2788e): libm-cplx-sincos-7-check0 rc 0 (xtask clippy, static).
* Merged origin/next 1700e1798. Two conflicts, both additive: complex_fast.rs keeps `mod sinh` (+ its re-export)
  and POW-6's `mod pow`; xtask/src/longsweeps.rs keeps both long sweeps (csinh_quick_tier_sweep, cpow_decline_rates).
  complex.rs auto-merged (csinh/csin and cpow call disjoint tiers).
* First gate and plants on the merge (libm-cplx-sincos-7-gate rc 1, -plants rc 2) both stopped in `xtask so`:
  "hot-text.absent: 1 stale fast line": with next merged, catalog::textdomain is a function section in the fast
  archive again, so run 5's fast line (#5183) no longer applies. Dropped it (69f3c3967); hot-text.absent now equals
  next's. This is the layout-jitter class (ruling #4347); only our own exemption was removed, no bound changed.
* libm-cplx-sincos-7-lint (full clippy -D warnings, xtask clippy, static on 1dd7bc84c) rc 0; -lint2 (xtask clippy,
  static on 69f3c3967) rc 0.
* Gate libm-cplx-sincos-7-gate2 and plants libm-cplx-sincos-7-plants2 (the -5 plants file, unchanged: no plant
  target is in a merged file) on 69f3c3967: gate rc 0, direct tier, 9 suites, 2645 passed, 0 failed, 561 ignored, conformance 19 passed, peak 3.8G; rc 0, 16 CAUGHT, 0 SURVIVED, 0 WARN (handoff/LIBM-CPLX-SINCOS-7-plants.txt).

## Runs 5-6 (no new math; full notes in handoff/reports/LIBM-CPLX-SINCOS-5.json and -6.json)

* Run 5: ruling #5183 absent lines for catalog::textdomain (run 7 dropped the fast one again, see above); gate held
  by 3 start-up fault tests (hot_text_products) that were next's TscGate::fresh outlining, not this branch.
  `sinh-q-cut` anchored to `let (q, eq) = if a <= Q_MAX {` in `hyp`; new test
  `csinh_overflow_step_threshold_is_two_to_the_1024` catches the `ovf-threshold` plant. Plants 16 CAUGHT.
* Run 6: next e30374455 merged (HOTTEXT-TIME-CLUSTER-STABLE-3); gate 11717 passed / 0 failed.

## Decline rates (run 4, code unchanged since): 0 of 10^6 declines in every sweep regime, csinh and csin

## Next steps (in order)

1. Done in run 7 (next with POW-6 merged, gate and plants on that head): the branch is ready to land. A continuation
   starts at step 2.
2. Percall validation/percall/cplx_sin.c against next and glibc (only cold code was added in run 4; not measured yet).
3. ccos/ccosh on the shared kernel (ccos(z) = ccosh(iz)); abs(t) < 2^-30 and >= 2^20 in the tier; FMA build.

Do not redo: the quick-tier design and its bounds, the middle tier and overflow step, run 3's diagnosis, run 5's
next-vs-branch fault measurement. The textdomain absent lines follow the layout: `xtask so` names a missing or stale
line; this branch has no lines of its own left in hot-text.absent.
