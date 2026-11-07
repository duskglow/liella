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
* `libm-f64-trig-quick-2-gate`: direct tier (see the report section below for the result).
* Driver `~/work/LIBM-F64-TRIG-QUICK-2/driver.sh` (after the gate, in order): `-counts` (cbench_libm `--compare
  --counters`, default then `--tunables glibc.cpu.hwcaps=-FMA`), `-stage` (`bench --stage
  ~/work/LIBM-F64-TRIG-QUICK-2/stage.tar`), `-accuracy` (10^8 per function, distribution and build).
* Timing bookings 128 (kotori, Ivy Bridge) and 129 (nozomi, N100), 90 min.

## Not covered (say so, do not imply)

The brief's accuracy job asks for "four modes": the quick phase runs only in round-to-nearest (the shared `elem_fast`
envelope picks the accurate path otherwise, unchanged by this story and covered by the existing entry sweeps); the new
job checks nearest only.

## Next step

See the report section (filled at hand-back).

## Not to redo

Base profile (run 1: FMA sin 142, cos 176, tan 169, sin_huge 1395; SSE2 sin 222, cos 257, tan 346, sin_huge 1398, one
call, Zen 1). The `π/64` one-step choice (D-10.61). The plants above.
