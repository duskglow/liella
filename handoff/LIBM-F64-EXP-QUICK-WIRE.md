# LIBM-F64-EXP-QUICK-WIRE: exp quick phase wired as tier 1 (timing not done)

State: branch `task/libm-f64-exp-quick-wire` (stacks on LIBM-F64-EXP-QUICK-2), code head 5f398bb36 (handoff commits
on top). Role keke:~/w/n4. Brief: `/home/claude/state/briefs/LIBM-F64-EXP-QUICK-WIRE.md`. Decisions in force: D-10.51
(two-phase double functions), D-10.54 (quick kernel constants and bounds), D-10.25/D-10.50 (two builds), D-10.3 (CR).

## What is in

* `crates/math/src/fast64/quick.rs`: `exp_tiered` in both builds (inside the `quick!` macro): quick phase; on a
  miss `tier2(x)`, a `#[cold] #[inline(never)]` function per build (SSE2: `fast64::exp_nearest`; FMA:
  `#[target_feature(enable = "fma")]` calling `fast64::fused::exp_nearest`). Outside `2^-60 <= |x| < 708` (tier 2's
  own domain) it returns `None` at once, so the accurate path runs without a useless tier-2 attempt.
* `crates/boundary/src/hot/exp_log.rs` `exp_nearest` (the name the generated exp entries use, so `s10_math_gen.rs`
  is unchanged) now calls `quick::exp_tiered` / `quick::fused::exp_tiered` behind `dispatch::fma()`: one dispatch
  load and branch per call, as before. `mret::elem_fast`'s contract holds: the quick result is inexact by its test
  (`w != 0`), and a refused attempt's inexact/denormal bits are in `FAST_ATTEMPT_HW`.
* Tests: `exp_quick.rs::exp_entry_rounds_quick_misroundings_right` (m::exp, SSE2 build, on the >= 20 raw quick
  misroundings of a seeded 4·10^5 sample; a `BUILD` mutex now serialises the build switch with `with_fma`);
  `libm_fma_select.rs::libm_product_exp_quick_misses_take_tier_two` (product, `-FMA`: 64 quick misses that tier 2
  decides cost at most hits + 400 instructions per call, and return the CR bits). Existing entry suites unchanged:
  `s10_math_symbols.rs` (exp in all four modes, flags, errno), `math_fma_paths.rs` (both builds bit-identical).
* Plants `handoff/LIBM-F64-EXP-QUICK-WIRE-plants.txt`: tier-1 value on a miss, tier 2 skipped, MXCSR default test
  inverted, FMA build chosen despite the tunable.

## Profile (before, one call, Zen 1 host, `validation/wl_trace.sh`, job keke:libm-f64-exp-quick-wire-trace0)

Case `exp` over 64 operands in [-10, 10] (spec `~/work/LIBM-F64-EXP-QUICK-WIRE/exp_case.c` on keke, = the
parity_libm1105 `F1` macro). FMA build: 125 instructions = `exp` entry/envelope 18 + `liella_machine_mxcsr_get` 3
(21) + kernel 104. SSE2 build (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`): 179 = 21 + 158. The surplus is the kernel
(tier 2's double-double evaluation on every call); the quick phase replaces it (LIBM-F64-EXP-QUICK-2 counted 72 / 86
for the kernel through a selftest wrapper).

## Envelope items of DESIGN.md §5.1 (not changed: RULING NEEDED)

* Inline `stmxcsr` in `fpu::peek`: inline assembly is allowed only at the `tb-field` site (docs/architecture.md
  S2); no category fits. Saves the call, `ret` and a reload: about 2-3 of the 21 envelope instructions.
  Recommend a new category (one `stmxcsr` into a local, one named file) or extending hot-kernel.
* Register-only barrier in place of `core::hint::black_box(x)`: needs `asm!("", inout(xmm_reg) x)`, same rule.
  Saves a store and a load (2 instructions) and a store-forwarding delay on the critical path (latency).
* `dispatch::fma()`: kept (one load and branch). Removing it needs the whole entry duplicated under
  `#[target_feature]` or an indirect call chosen at startup; not tried.

## Jobs (keke)

lint `libm-f64-exp-quick-wire-lint` (kernel-index, clippy, static: rc 0 on 8bb365c11 + regenerated KERNELS.md),
gate `libm-f64-exp-quick-wire-gate` (running at hand-back: liella, liella-boundary incl. interposed, liella-legacy,
liella-math ok so far), plants `libm-f64-exp-quick-wire-plants` (rc 1: tier1-on-miss, mxcsr-inverted,
fma-without-tunable CAUGHT; **tier2-skipped SURVIVED**), counts `libm-f64-exp-quick-wire-counts` (queued behind the
gate: cbench_libm `--compare --counters`, plain then `-FMA`), diagnostic `libm-f64-exp-quick-wire-plants-diag` (the
tier2-skipped plant alone with `--nocapture`: its baseline log and mutant log print "exp instructions per call, SSE2
builds: quick-phase misses M, hits H"; evidence, not a verification job).

Why tier2-skipped survived: exp's accurate path (`elem_fast_cold` -> `Real::exp`) itself starts with the same
`exp_near` kernel, so a skipped tier 2 costs only the cold envelope on top, well under the test's 400-instruction
margin. The test logic is right; `TIER2_MARGIN` must sit between (M with tier 2 − H) and (M without − H) from the
diag logs (`/dev/shm/claude/libm-f64-exp-quick-wire/plants-diag-1/logs/` on keke). If the gap is under ~20
instructions, a count test cannot separate them robustly: then mark the plant `equivalent:` with that reason and
the numbers (the cost is the envelope only), or have the test count a directed-mode call as the reference.

## Next step

1. Counts: `cargo xtask bench --compare --counters cbench_libm`, plain and with `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`
   (exp rows; base 125 / 179 per traced call). 2. Timing (required, instruction mix changes): `ops timing-book`
   kotori or nico (shorter queue) and nozomi, `cargo xtask bench --stage`, run cbench_libm exp rows with
   `CBENCH_ROWS`; expected Ivy Bridge about 1.4x glibc (DESIGN.md §4.1). 3. exp2/exp10 on the same table (same
   kernel shape, own constants). 4. The two envelope rulings above.

## Not to redo

The kernel, its bound and tables (D-10.54); the base profile above. Any quick.rs edit needs `cargo xtask kernel-index`.
