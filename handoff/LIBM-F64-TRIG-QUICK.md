# LIBM-F64-TRIG-QUICK(-2): binary64 sin/cos/tan quick phase with a shared reduction

State: branch `task/libm-f64-trig-quick-2` (continues `task/libm-f64-trig-quick`, stacks on LIBM-F64-EXP-QUICK-WIRE),
role keke:~/w/r5. Briefs: `/home/claude/state/briefs/LIBM-F64-TRIG-QUICK.md` (spec), `-2.md`. Decisions in force:
D-10.51 (two-phase double functions), D-10.25/D-10.50 (two builds), D-10.3 (CR), **D-10.61 (this story; row written
in plan/07-decisions/s10-mathematics.md, includes the accepted one-step `π/64` reasoning)**.

## What is in

* `crates/math/src/trig/quick.rs` (run 1): quick phase for sin, cos, tan, sincos, `plain` (SSE2) and `fused` (FMA)
  builds, one reduction (Cody-Waite `π/64` below 2^20, integer Payne-Hanek with 3 u64 products above), today's
  `SIN_K_PI64` table, Taylor + Estrin, Ziv test; `*_tiered` = quick, then today's kernel cold. Bound and method: module
  doc and D-10.61. Wired in `crates/boundary/src/hot/trig.rs`.
* Run 2: selftest hooks `selftest::trig_quick(which, x)` / `sincos_quick` (`hot::trig_quick`, cfg selftest) and
  `crates/boundary/tests/trig_quick.rs`: FMA build bit-equal to the CR result on [-10,10], [-1e6,1e6] and uniform bit
  patterns over [2^-30, 2^1024) (Payne-Hanek path), sincos == (sin, cos), both builds bit-identical where both answer,
  and the ignored 10^8 accuracy job (`TRIG_QUICK_N`). FMA fallback on uniform [-10,10]: sin 0.83%, cos 0.81%, tan 1.62%.
* Plants file `handoff/LIBM-F64-TRIG-QUICK-2-plants.txt` (6 plants: the brief's four on the SSE2 tests, quadrant and
  rounding-constant again on the FMA boundary test).

## Jobs (keke unless named)

* `libm-f64-trig-quick-2-t1` rc 101 (lint: one unsafe block held three calls; fixed), `-t2` rc 0 (clippy -p
  liella-boundary, trig_quick 2/2). Superseded by the gate.
* Step 0 on the inherited head (foreground): `cargo xtask clippy` 0, `cargo xtask kernel-index` up to date, `cargo
  xtask static` 0.
* `libm-f64-trig-quick-2-plants` rc 0: 6 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY/NO-MSG (WARN-DEV-SHM only: no test here
  mounts /dev/shm).
* `libm-f64-trig-quick-2-gate` rc 1 on 2d543fd81: every suite ok (9 suites, 7288 passed, 0 failed, conformance 6/6,
  peak 11G, oom_kill 0) but `STEP FAILED: unsafe-audit: 1 violations`: the selftest hook's `unsafe fn` pointer line in
  src/hot/ had no UNSAFE comment (I ran static on the inherited head only, before adding it). Fixed in 757f653e9 (one
  commented unsafe block per call). `-counts` (first driver) was killed while queued (stale tree); never ran.
* Final head: driver `~/work/LIBM-F64-TRIG-QUICK-2/driver2.sh`, started detached at hand-back, runs in order (one
  build at a time in r5): `-lint` (full clippy --all-features, xtask clippy, static), `-stage2` (`bench --stage
  ~/work/LIBM-F64-TRIG-QUICK-2/stage.tar`), `-counts2` (cbench_libm `--compare --counters`, default build then
  `--tunables glibc.cpu.hwcaps=-FMA`; the -FMA tunable may not reach the bench programs, see
  LIBM-F64-EXP-QUICK-WIRE.md: compare the sin rows of the two runs), `-gate2` (direct tier), `-accuracy2` (10^8
  operands per function, distribution and build; 0 mismatches = rc 0), `-plants2` (the same 6 plants on the final
  head, dir /dev/shm/claude/LIBM-F64-TRIG-QUICK-2/plants-2; the first `-plants` ran before the hook fix). keke's slots were full at hand-back (a
  queued foreground clippy waited 10+ min and was stopped), hence the detached driver.
* Timing bookings 128 (kotori) and 129 (nozomi) cancelled unused: the stage tarball could not be built in budget.

## Not covered (say so, do not imply)

The brief's accuracy job asks for "four modes": the quick phase runs only in round-to-nearest (the shared `elem_fast`
envelope picks the accurate path otherwise, unchanged by this story and covered by the existing entry sweeps); the new
job checks nearest only.

## Next step

1. Read the driver's five jobs (`~/bin/jobwait libm-f64-trig-quick-2-{lint,stage2,counts2,gate2,accuracy2}` on keke).
   A red: fix and rerun only that job. Counts: sin/cos/tan/sin_huge rows of `-counts2` against the base (brief:
   FMA sin 152 vs glibc 105, cos 156/107, tan 181/112, SSE2 sin 232/127; target sin <= 110 FMA); if any measured row
   is slower than its base, profile one call (wl_trace) before landing. Add the counts to D-10.61.
2. Timing (owner exception: the change alters the instruction mix, a division in tan): copy
   keke:~/work/LIBM-F64-TRIG-QUICK-2/stage.tar to an Ivy Bridge host (kotori or nico, less busy) and nozomi (N100),
   unpack whole into a new dir, `ops timing-book`, then `~/bin/timing <label> ./xtask-bin bench --no-build --compare
   cbench_libm` (try the command once briefly first); report sin, cos, tan, sin_huge ratios; the first hand-back line
   wants `IVB <x>x / N100 <y>x`.

## Not to redo

Base profile (run 1: FMA sin 142, cos 176, tan 169, sin_huge 1395; SSE2 sin 222, cos 257, tan 346, sin_huge 1398, one
call, Zen 1). The `π/64` one-step choice (D-10.61). The plants above.
