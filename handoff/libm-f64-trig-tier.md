# LIBM-F64-TRIG-TIER handoff (run 1: kernels written, not yet wired; WIP)

Branch task/libm-f64-trig-tier, role shiki:~/w/sh1, base origin/next. Decisions in force: D-10.13 (envelope),
D-10.25 (FMA builds), D-10.50 (libm decides FMA at load). No new decision row; no compat id.

## Baseline (from handoff/LIBM-F64-FAST-KERNELS.md, kanon, cbench_libm instr/call)

sin 338.6 / cos 336.0 / tan 494.9 (FMA and -FMA the same: trig had no FMA build); glibc 100.2 / 104.7 / 107.7 (FMA),
122.5 / 126.2 / 124.2 (-FMA).

## Per-phase split of one `sin` call (ESTIMATE from the source operation count, register 16 method (3); NOT traced)

| phase | now (~) | after this design (~, -FMA / FMA) | why it saves |
|---|---:|---:|---|
| envelope `mret::elem` (MXCSR read, black_box spills, `e.result`, exit flag merge, errno) | 70 | 8 / 8 | in-place `elem_fast` entry as exp/log (register 16.5 #4) |
| `trig_special` generic `float::decode` | 25 | 3 / 3 | one integer range test on the bits |
| reduction (Cody-Waite, 3 parts) | 20 | 20 / 20 | unchanged (already exact and short) |
| polynomial + assembly (3 Dekker products, 2 two_sum) | 125 | 125 / 80 | FMA build: each product 17 -> 2 flops |
| rounding test (`settle`, `Status` built) | 25 | 4 / 4 | constant test first (fast64::near), exact test only on a miss |
| fallbacks to the accurate path (bound 2^-61 rel.: ~1% sin/cos, 2.5% tan measured 10-23) | 15-20 (tan ~60) | ~2 | bound restated 2^-68 abs. (S != 0): fallback ~2^-9..2^-14 |
| tan: second reduction-free `eval` (poly computed twice) | (tan +90) | 0 | poly shared, only the assembly repeated |

Projection (not measured): sin/cos ~180 (-FMA) / ~125 (FMA), tan ~260 / ~190. 1.05x needs a structure with fewer
exact products on the -FMA build (pre-split table halves, rh split once: ~-23) and a minimax polynomial (D-10.3 tool:
~-4); say so with data after the count.

## Done (commit 21af2247 + the rename commit)

`crates/math/src/trig.rs`: `trig_kernels!` (as fast64's `kernels!`) builds `trig::kernel` (Dekker) and
`trig::kernel_fma` (FMA, `#[target_feature]`): `reduce`, `poly` (shared), `assemble`, `quotient`, and the in-place
entries `sin_nearest`, `cos_nearest`, `tan_nearest`, `sincos_nearest` (Option<f64>, nearest only, hardware raises
inexact only; `2^-339 <= |x| < 2^-30` answered in place, smaller/subnormal/>= 2^20/NaN/inf -> None). The directed
`fast::{sin,cos,tan}` now use the same kernels and the new `eps`. The module comment carries the term-by-term bound
(E = 2^-68 abs. when S != 0; 2^-89 + 2^-63|y| when S = 0), the constant test (rho 2^-62 sin/cos, 2^-60 tan;
C_SIN = 1+2^-8+2^-14, C_TAN = 1+2^-6+2^-11, checked by hand in the comment).
Check: job shiki:libm-f64-trig-tier-check2 (`cargo test -p liella-math --lib trig`): 10 passed (includes
trig_fast_path_matches_the_accurate_path: fast == accurate, all four modes, 50k samples + hard cases). check1 failed
(module name `fused` clashed with `use crate::fused`; renamed).

## Exact next step

1. `crates/boundary/src/hot.rs`: `sin_nearest`/`cos_nearest`/`tan_nearest`/`sincos_nearest` choosing
   `liella_math::trig::kernel_fma::*` under `dispatch::fma()` (copy `exp10_nearest`'s shape and UNSAFE comment).
2. `crates/boundary/src/mathret.rs`: `sincos_fast` (= `sincos` with `fpu::peek` in-place try, cold fallback like
   `elem_fast_cold`, out-pointers checked first with `ret::must(conv::req(..))`).
3. `s10_math.tsv`: split `sin`/`cos`/`tan`/`sincos` rows into `flq` (unchanged) and `d`
   (`mret::elem_fast::<{ math::Status::HW_ANY.0 }, _, _>({x}, crate::hot::sin_nearest, math::real::Real::sin)`;
   sincos: `mret::sincos_fast(...)`), then `cargo xtask math-gen` as a job. The gate becomes FULL (#2921).
4. Tests: add sin/cos/tan/sincos to `double_fast_entries()` and the `match name` in
   crates/boundary/tests/s10_math_symbols.rs plus trig hard cases (multiples of pi/2 +-2 ulp, 2^-30, 2^-339, 2^20,
   cell edges k*pi/64 +- pi/128); add them to crates/boundary/tests/math_fma_paths.rs (bit-identity of both builds).
5. clippy + static, gate, plants (`cargo xtask plants`, new RAM dir): E_ABS -> 2^-70, a C_SIN bit lowered, R25
   check dropped, P3 last bit, LIMIT_BITS -> 2^21, SMALL_BITS -> 2^-400; counts `cbench_libm` before/after, default
   and `--tunables glibc.cpu.hwcaps=-FMA`.

Do not redo: the bound derivation (in the module comment); the old `fast::eval` is gone (split into poly/assemble).
