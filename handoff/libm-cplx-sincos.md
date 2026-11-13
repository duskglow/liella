# LIBM-CPLX-SINCOS handoff: csinh/csin (double, float) quick tier, overflow step and middle tier

State: branch task/libm-cplx-sincos-5 = -4's head 722ba663c + 50bb860cf "Allow the catalog::textdomain hot-text key to be
absent in the fast profile" + merge of origin/next 5e5ec2a57 (cb1918d3f, no conflicts) + e5dc20754 "Pin the csinh
overflow step's threshold at 2^1024" (test only) + this handoff/report. Role kanon:~/w/kn1.
Decisions in force: D-10.53 (tier structure), D-10.25 (FMA builds), D-10.99 (cexp's middle tier, reused).
Pending: records/pending/LIBM-CPLX-SINCOS-2-1.toml (D-NEW, series D-10; body covers runs 2-4; the coordinator assigns).

## Run 5 (no new math work, per the brief): blocked on LAYOUT-JITTER (ruling #4347)

* Step 0 (inherited head 722ba663c): libm-cplx-sincos-5-check0 rc 0 (xtask clippy, static).
* Ruling #5183 applied in crates/libc-so/hot-text.absent: `_RNvNtC15liella_boundary7catalog10textdomain` may be
  absent in both profiles. The file takes ONE profile per line (`parse_absent`, xtask/src/hottext.rs, accepts `fast`
  or `release` only), so "fast,release" is two lines, each citing #5183. The key stays in hot-text.list and .unpinned.
* libm-cplx-sincos-5-so2 rc 0 (merged head): `LIELLA_FULL_PROFILE=1 cargo xtask so` then `cargo xtask so`; the
  12 layout contracts hold. The textdomain blocker is gone.
* Gate libm-cplx-sincos-5-gate2 (final code head e5dc20754, `affected --run --base origin/next`): rc 1, 6492 passed,
  3 failed, all in crates/core/tests/hot_text_products.rs (start-up page faults):
  - init_first_localtime_r_and_mktime_stay_within_their_combined_page_fault_bound: 21 page faults > 20
  - init_first_mktime_after_localtime_r_takes_no_fault: 1 page fault (want 0)
  - init_zero_fault_calls_hold_at_every_text_phase: mktime 1 fault at every pre and cold pad (want 0)
  Deterministic: rerun alone (libm-cplx-sincos-5-brfaults, rc 101) gives the same numbers.
  On origin/next 5e5ec2a57 (the merged next, exported to a RAM copy on kanon and built with the fast profile:
  libm-cplx-sincos-5-nextfaults, rc 0) all three PASS (<= 20, and 0 for mktime).
  This branch touches no start-up code (cold csinh/csin code in liella-math, a test, the absent lines), so per
  ruling #4347 nothing was chased: no regen, no section moves, no bound change. Since the build no longer has
  catalog::textdomain as a function in the fast profile either, its caller's inlining changed with libm's growth.
  That fits the "unrelated code growth moves the layout" class, but it was not measured.
* Plants libm-cplx-sincos-5-plants2 (final code head): 16 CAUGHT, 0 SURVIVED, 0 WARN
  (handoff/LIBM-CPLX-SINCOS-5-plants.txt = the -2 file + the -4 file, with results). Two changes against those files:
  `sinh-q-cut` now matched two sites (the middle tier added `if a <= Q_MAX {` in `mid_hyp`), so it is anchored to
  `let (q, eq) = if a <= Q_MAX {` in `hyp`, its original target. `ovf-threshold` (2^1024 -> 2^1023) SURVIVED the
  first run (libm-cplx-sincos-5-plants): the random overflow test rarely sees a quick decline inside the top binade.
  The new test `csinh_overflow_step_threshold_is_two_to_the_1024` calls `overflow` on both sides of 2^1024 and
  catches it.

## Decline rates (run 4, code unchanged since): 0 of 10^6 declines in every sweep regime, csinh and csin

## Next steps (in order)

1. When the layout fix lands (LAYOUT-TEXT-PIN / LAYOUT-DATA-CLUSTERS, or whatever the coordinator names), merge next
   and rerun the gate. Nothing else is left for landing.
2. Percall validation/percall/cplx_sin.c against next and glibc (only cold code was added in run 4; not measured yet).
3. ccos/ccosh on the shared kernel (ccos(z) = ccosh(iz)); abs(t) < 2^-30 and >= 2^20 in the tier; FMA build.

Do not redo: the quick-tier design and its bounds, the middle tier and overflow step, run 3's diagnosis, the
textdomain absent lines, the next-vs-branch fault measurement above.
