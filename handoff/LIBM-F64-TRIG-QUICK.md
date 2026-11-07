# LIBM-F64-TRIG-QUICK(-2, -3): binary64 sin/cos/tan quick phase with a shared reduction

State: branch `task/libm-f64-trig-quick-3` (continues `-2` and the first run; merge base 799313e1 =
LIBM-F64-EXP-QUICK-WIRE-2), role mei:~/w/s1. Code head 3e2d8ba81 (this run's only code change). Briefs:
`/home/claude/state/briefs/LIBM-F64-TRIG-QUICK.md` (spec), `-2.md`, `-3.md`. Decisions in force: D-10.51 (two-phase
double functions), D-10.25/D-10.50 (two builds), D-10.3 (CR), **D-10.61 (this story; now with counts and timing)**.

## What is in

* `crates/math/src/trig/quick.rs`: quick phase for sin, cos, tan, sincos, `plain` (SSE2) and `fused` (FMA) builds,
  one reduction (Cody-Waite `π/64` below 2^20, integer Payne-Hanek with 3 u64 products above), `SIN_K_PI64` table,
  Taylor + Estrin, Ziv test; `*_tiered` = quick, then the previous kernel cold. Wired in `crates/boundary/src/hot/trig.rs`.
* Selftest hooks `selftest::trig_quick` / `sincos_quick`, `crates/boundary/tests/trig_quick.rs` (FMA build bit-equal
  to CR, builds agree, sincos == (sin, cos), ignored 10^8 accuracy job `TRIG_QUICK_N`).
* Run 3 (3e2d8ba81): fixed the two static findings that turned `-2`'s lint red: the selftest hook's
  `#[allow(unsafe_code)]` on the three-arm match now carries its own `UNSAFE(hot-kernel)` comment (existing category);
  the test's `liella_math::trig::{sin,cos,tan}(x, Rounding)` calls are marked `// opaque-ok:` (mangled two-argument
  Rust functions, not the C names, so LLVM cannot fold them as builtins).

## Jobs (mei unless named), all on 3e2d8ba81

* Step 0 on the inherited head 33cb84ef0: `cargo xtask static` rc 1 (the two findings above, the cause of `-2-lint`
  rc 1). After the fix: static 0, `cargo xtask clippy` 0 (foreground).
* `libm-f64-trig-quick-3-lint` rc 0 (full clippy --all-features -D warnings, xtask clippy, static).
* `libm-f64-trig-quick-3-plants` rc 0: 6 CAUGHT, 0 SURVIVED, 0 WARN (plants file `handoff/LIBM-F64-TRIG-QUICK-2-plants.txt`).
* `libm-f64-trig-quick-3-gate` rc 0: direct tier, 9 suites ok, 7288 passed, 0 failed, conformance 6/6, peak 13G, oom_kill 0.
* `libm-f64-trig-quick-3-stage` rc 0 (stage.tar sha256 b6ec74e2...), `-counts` rc 0, `-stage-base` rc 0 (merge base
  799313e1 from `git archive` in /mnt/zram, for the timing base).
* `libm-f64-trig-quick-3-accuracy` rc 0: 10^8 operands per function (sin, cos, tan) x distribution (uniform
  [-10,10], [-2^20,2^20], bit patterns over [2^-30, 2^1024)) x build, nearest: 0 mismatches; FMA fallback 0.81-1.97%.
* Timing (bookings 130 kotori, 131 nozomi, both cancelled after): `kotori:libm-f64-trig-quick-3-ivb`, `-ivb-base`,
  `nozomi:libm-f64-trig-quick-3-n100`, `-n100-base`, all rc 0 (`./xtask-bin sweep run --only cbench_libm`, ~2 min
  each). The nozomi head run started 3 min inside the booking's settle window (host idle, 0 jobs); its spreads are
  0.006-0.025.

## Results

Counts (cbench_libm instr/op, Skylake-SP; base = `libm-f64-exp-quick-wire-2-counts` on mei, which is exactly the
merge base): FMA sin 158.6 -> 131.7 (glibc 100.2), cos 156.4 -> 123.7 (104.7), tan 176.1 -> 157.9 (107.6), sin_huge
1345.1 -> 239.6 (365.8), sin_upward 490.4 -> 490.1. SSE2 (the `-FMA` tunable did reach the bench this time; counts
differ from the FMA run): sin 173.1 (122.3), cos 169.0 (126.1), tan 239.3 (124.2), sin_huge 291.7 (379.3); no same-tool
SSE2 base (the base job ran without the tunable). Every other row within 1% (mxcsr_pair 3.0 -> 3.1 is the harness row).
Timing, time ratio vs glibc before -> after: Ivy Bridge sin 2.22 -> 1.71, cos 2.13 -> 1.60, tan 2.83 -> 2.33,
sin_huge 4.56 -> 0.71; N100 sin 2.13 -> 1.67, cos 1.83 -> 1.53, tan 2.64 -> 2.01, sin_huge 5.95 -> 0.87. Rows whose
time moved > 5% without an instruction or cycle change (kotori exp_small, exp_tiny, sinh; nozomi logf, sqrt): the
cycle ratios are unchanged (e.g. exp_tiny cycles 3.516 -> 3.520); the base run's time column disagreed with its own
cycles (spread 0.48-0.56), so this is timing noise, not a regression.

## Not covered (say so, do not imply)

Accuracy job checks round-to-nearest only (the quick phase runs only there; other modes take the unchanged accurate
path, covered by the entry sweeps). Brief target sin <= 110 instr (FMA) not reached: 131.7.

## Next step

1. None required: the story is done. Optional follow-up toward sin <= 110: profile one FMA call (validation/wl_trace.sh) against glibc's 100 and aim
   at the phase holding the surplus (the envelope was ~21 instructions for exp, LIBM-F64-EXP-QUICK-WIRE.md).

## Not to redo

Base profile (run 1, Zen 1). The `π/64` one-step choice (D-10.61). The 6 plants. The base timing above (same hosts,
same stage method).
