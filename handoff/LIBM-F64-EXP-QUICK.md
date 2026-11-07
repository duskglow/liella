# LIBM-F64-EXP-QUICK: binary64 exp quick phase (kernel, proof, tests; not wired)

State: branch `task/libm-f64-exp-quick`, head after this file's commit (code head f4e08c24f). INCOMPLETE: context
checkpoint (134k of the ~150k limit) at 44 of 80 calls. Brief: `/home/claude/state/briefs/LIBM-F64-EXP-QUICK.md`
(same text as `handoff/libm-rethink/LIBM-F64-EXP-QUICK.md`).

## Done (verified on mei:~/w/s4)

* `crates/math/src/fast64/quick.rs`: `exp_quick` / `exp_quick_parts`, SSE2 build plus `quick::fused` (FMA), one `quick!`
  macro; the step-by-step bound in the module doc. Bounds: SSE2 2^-61.503 (stated `RHO_SSE2` = 1.4375·2^-62 = 2^-61.476,
  `C_SSE2` 0x3ff017213fcbb4d5, expected fallback 0.565%); FMA 2^-67.727 (stated `RHO_FMA` = 1.25·2^-68 = 2^-67.678,
  `C_FMA` 0x3ff00050019007d1, fallback 0.0076%). Term list as the brief's, but redone for Estrin: evaluation 2^-69.04,
  low sum 4·2^-71.06 (SSE2) / 3·2^-71.06 (FMA), rl·rh 2^-70.53, tl·q 2^-71.06, truncation 2^-72.00, reduction 2^-78.41.
  The r1 = x − kd·L1 exactness uses a grid argument (multiples of 2^-61, |r1| < 2^-8), not Sterbenz (|k| = 1 is a
  boundary case for Sterbenz).
* Tables from `cargo xtask tables` (`crates/mathtools/src/tables.rs`): `EXP2_J128`, `LN2_128_CW`, `SCALE_128` in
  `coeffs/exp_log.rs`; `crates/math/KERNELS.md` refreshed by the same command.
* `quick/tests.rs` (SSE2 build, §10.2 exp, D-10.3): constants plus the bound recomputed from them (`quick_constants_and_bounds`),
  error against `Mp` (measured max **2^-61.583**, bound 2^-61.476), **10^7 operands, 0 mismatches**, fallback within
  0.4-0.8% on uniform [-10, 10], hard cases (the 144 e^(k·2^-53) plus every raw misrounding in a 4·10^5 sample; at
  least 20 needed), domain. Job `mei:libm-f64-exp-quick-unit`: 5 passed (before the Safety-doc commit, no code change since).
* D-10.54 (constants and bounds, under D-10.51) in `plan/07-decisions/s10-mathematics.md`.
* Lint: `libm-f64-exp-quick-lint` (static ok; clippy red: missing `# Safety`), fixed; `libm-f64-exp-quick-lint2` clippy clean.

## Next step (exact)

1. Boundary access for the FMA build (unsafe stays in liella-boundary): in `crates/boundary/src/hot/exp_log.rs` add
   `#[cfg(feature = "selftest")] pub(crate) fn exp_quick` and `exp_quick_parts` dispatching on `dispatch::fma()` like
   `exp_nearest` (`// UNSAFE(hot-kernel) SAFETY: ...`); `exp_quick_parts` in `quick!` is `pub(crate)`: make it `pub` with
   `#[doc(hidden)]`; export `RHO_FMA`/`RHO_SSE2` the same way; `selftest::exp_quick`, `selftest::exp_quick_parts` next to
   `set_math_fma` (selftest.rs ~line 700).
2. `crates/boundary/tests/exp_quick.rs` (clause s10_0_f_0 / §10.2 exp, D-10.3): FMA vs SSE2 bit-identity, each answer CR
   (`liella_math::fast64::exp_nearest`, else `Real::exp`), >= 10^7 FMA operands; FMA fallback < 0.05% on uniform [-10, 10];
   FMA max relative error against `liella_math::mp` below `RHO_FMA` (print it); `#[ignore]` accuracy test with
   `EXP_QUICK_N` (default 10^8) over uniform [-10, 10], uniform bits over [2^-60, 708) with both signs, uniform
   [-745.2, 709.8], both builds (use `std::thread::scope`), 0 mismatches; `#[ignore]` count loop (4096 operands in
   [-10, 10], `EXP_QUICK_CALLS`) for a `perf stat -e instructions:u` difference N vs 0 calls, both builds; glibc side: a
   C loop over the same operands with and without `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA` (pattern
   `handoff/libm-rethink/proto/proto_percall.c`). Add the test to `math_set` in `xtask/src/affected.rs` if not selected.
3. Plants file `handoff/LIBM-F64-EXP-QUICK-plants.txt` (`docs/plants.md` format, `msg:` each), run with
   `cargo xtask plants`: C4 bits ...5555 -> ...5556 (msg `C4 is not RN`); the SSE2 invocation's `C_SSE2` -> the c of
   rho = 2^-64 (`1.0009775171065494`, msg `hard case|correctly rounded`); `pc = $ma(r2, C6, pb)` -> `$ma(rh, C6, pb)`;
   `EXP2_J128[j][0]` -> `[(j + 1) & 127][0]`; `$ma(th, rl + q,` -> `$ma(th, q,` (needs the FMA tests: cmd runs
   liella-math quick tests and the boundary exp_quick test); `1023 * 128` -> `1022 * 128`.
4. Jobs: direct-tier gate, plants, the 10^8 accuracy job, the count job. Timing: none possible before wiring;
   LIBM-F64-EXP-QUICK-WIRE books Ivy Bridge and the N100.

Design targets (module doc): FMA <= 80 and SSE2 <= 95 instructions per call with fallback (NOT MEASURED yet); FP
operations 31 (FMA) / 39 (SSE2) including two compares; critical path 16 (FMA) / 21 (SSE2) dependent FP operations
plus the compare.

## Informational

* `KERNELS.md` changed by 147 lines from `cargo xtask tables` (kernel-index); generated, committed as written.
