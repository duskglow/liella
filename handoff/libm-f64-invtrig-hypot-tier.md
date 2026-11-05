# LIBM-F64-INVTRIG-HYPOT-TIER (run -2): inline square root; gate, plants, counts started (INCOMPLETE)

State: branch `task/libm-f64-invtrig-hypot-tier-2` (carries run -1's commits), code head `9fc38c3f`, cut from
origin/next 2c1e25b9; role keke:~/w/n4 (synced tree 232dab0f). Results stay correctly rounded: no differences
row changes (accuracy contract unchanged, ruling #2760 option A).

## Done in run -2
* Run -1's head compiles: `cargo clippy --workspace --all-targets --all-features -- -D warnings` rc 0 on n4
  (foreground, tree cec5796c, before this run's commit).
* Probe (pinned 1.99.1, a `no_std` + `forbid(unsafe_code)` crate): `_mm_sqrt_sd`/`_mm_sqrt_pd`/`_mm_set_sd`/
  `_mm_cvtsd_f64` in a plain fn are E0133 (SSE2 in the target baseline does not make them safe); `f64::sqrt` does
  not exist in core (E0599); the same calls inside a `#[target_feature(enable = "sse2")]` fn compile without
  `unsafe`. So liella-math cannot issue `sqrtsd` itself; the kernels keep the sqrt parameter.
* `hot.rs`: the sqrt the boundary passes is now `sqrtsd_sse2` (a safe `#[target_feature(enable = "sse2")]
  #[inline]` fn, `_mm_sqrt_pd(_mm_set_sd(x))`, upper lane +0 so no extra flags), reached through ONE
  `UNSAFE(hot-kernel)` call (sse2 is baseline). LLVM may inline it into every x86-64 fn and the FMA builds. No
  `unsafe` in liella-math. Run -1's inexact fix (`settle` returns `y.hi + y.lo`) kept.
* New test `double_atan_fast_tier_table_agrees_with_the_multiprecision_atan` (crates/math/tests/invtrig.rs):
  the accurate path reads the same `ATAN_J64`, so kernel-vs-accurate agreement cannot catch a wrong entry;
  this checks 16 operands around each node j/64 against `mp::atan` (no table), kernel and entry, both builds.

## Jobs (keke, started detached by ~/work/libm-f64-invtrig-hypot-tier-2/driver.sh; all on code head 9fc38c3f)
* `libm-f64-invtrig-hypot-tier-2-gate`: clippy -D warnings + `cargo xtask static` + `affected --base origin/next --run`.
* `libm-f64-invtrig-hypot-tier-2-plants`: `cargo xtask plants ~/work/libm-f64-invtrig-hypot-tier-2/plants.txt
  --dir /dev/shm/claude/LIBM-F64-INVTRIG-HYPOT-TIER-2/plants-1` (6 plants: bound-loose, bound-zero,
  table-entry, recip-term (|x|>1 branch), exact-root, sqrtsd-inline; copy in this story's scratch).
* `libm-f64-invtrig-hypot-tier-2-counts`: cbench_libm, FMA on and `--tunables glibc.cpu.hwcaps=-FMA` (inline sqrt).
* `libm-f64-invtrig-hypot-tier-2-counts-ool2` (evidence): the same counts in a RAM copy with the inline-sqrt
  hot.rs change reversed (`patch -R`): the out-of-line sqrt's per-call cost = counts-ool2 - counts on asin/acos/hypot.
* Superseded: `...-counts-ool` (cancelled while queued, replaced by counts-ool2); `...-sqrtprobe` (diagnostic, rc 0).
* Where the time went: keke's slot queue (counts-ool waited 35+ min without starting; gate/plants entered at
  positions 8-9), not the builds.

## Before (handoff/libm-slow-path-cause/report.md §3.3; docs/benchmarks.md, other CPU)
atan 1,345 (ref 79); asin 1,517 (ref 73); atan2 1,577 (ref 163); hypot 3,713 (ref 64); acos 1,569 (benchmarks.md).

## Exact next step
1. `~/bin/jobwait libm-f64-invtrig-hypot-tier-2-gate libm-f64-invtrig-hypot-tier-2-plants` on keke; fix any red
   (a SURVIVED bound-zero plant means the hard-case set is too weak near a rounding boundary: add operands).
2. From the two counts logs, fill rows atan/asin/acos/atan2/hypot (FMA on/off, Liella vs glibc, inline vs ool).
   If the out-of-line sqrt costs more than a few instructions on asin/acos/hypot the inline form stays (it is
   already in); if inline measures no better, revert 9fc38c3f's hot.rs half and record the NEGATIVE row.
3. Any row above 1.05x of glibc: record the residual with its first-principles count in docs/saturation (the libm
   section file) per the original brief; f32 rows (atanf, asinf, ...) are not wired here (follow-up).

## Do not redo
* The `_mm_sqrt_sd`-in-liella-math attempt (E0133 on 1.99.1, probed); the proof in invtrig.rs's module comment
  (checked in run -1).

## Plan rows to read
D-10.13, D-10.25; original brief /home/claude/state/briefs/LIBM-F64-INVTRIG-HYPOT-TIER.md.
