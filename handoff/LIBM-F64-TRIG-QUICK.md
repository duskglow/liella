# LIBM-F64-TRIG-QUICK: binary64 sin/cos/tan quick phase with a shared reduction (WIP, first checkpoint)

State: branch `task/libm-f64-trig-quick` (stacks on LIBM-F64-EXP-QUICK-WIRE), role keke:~/w/r5. Brief:
`/home/claude/state/briefs/LIBM-F64-TRIG-QUICK.md`. Decisions in force: D-10.51 (two-phase double functions),
D-10.25/D-10.50 (two builds), D-10.3 (CR). Reserved id D-10.61 (the module doc cites it; the plan row is NOT
written yet).

## What is in (WIP commits on top of 799313e1e)

* `crates/math/src/trig/quick.rs` (new; declared `pub mod quick;` in `trig.rs`): quick phase for sin, cos, tan,
  sincos in two builds (`plain` SSE2 with Dekker's product, `fused` FMA), one shared reduction:
  - `2^-26 <= |x| < 2^20`: `k` from the `1.5·2^52` shift (no conversion instruction; `n = bits(t) & 127`), today's
    three-part `π/64` Cody-Waite constants, 2Sum + Fast2Sum.
  - `|x| >= 2^20`: integer Payne-Hanek, 3 u64×u64 products against a 192-bit window of 2/π (zero-prefixed words),
    7 integer bits + 128 fraction bits, declines when `|f| < 2^-60`; nothing cached.
  - Evaluation on today's 128-point `SIN_K_PI64` table (one-step `π/64` reduction; why not `π/2` + `j/64`: module
    doc, last paragraph); only `C·rh` exact; Taylor sin to r^9, cos to r^8, Estrin; Fast2Sum assembly.
  - Bound proved in the module doc: `RHO = 0.875·2^-61`, `C_Q = 1 + 2^-7 + 2^-12`; tan `rho 2^-60` with today's
    `C_TAN` (the quotient is normalized by Fast2Sum before the test: without it the fallback was 32.6%).
  - `*_tiered`: quick, then today's kernel (`trig::kernel{,_fma}::*_nearest`) cold and out of line.
* `crates/boundary/src/hot/trig.rs`: the four entries now call `trig::quick::{plain,fused}::*_tiered`.
* `crates/math/src/trig/quick/tests.rs` (5 tests, SSE2 build): bound terms and multipliers (exact Mp check);
  every assembly within RHO of `mp::sin_cos_f64` (largest seen 2^-63.163, bound 2^-61.193) over [-10,10],
  [-1e6,1e6], small and huge operands; 1.4·10^6 operands per function against the CR path, bit for bit, fallback
  window on [-10,10] (sin/cos 0.4-1.2%, tan 0.8-2.5%); hard cases next to multiples of π/64 and the
  6381956970095103·2^797 case; domain.

## Jobs (keke)

* `libm-f64-trig-quick-trace0` (rc 0): base profile, one call (Zen 1): FMA sin 142 (entry 18 + mxcsr 3, kernel
  ~121), cos 176, tan 169, sin_huge 1395 (generic dd `sin_cos` 571, `__floattidf` 90, `elem::finish` 83, cold
  envelope 34: the huge path never had an in-place tier); SSE2 sin 222, cos 257, tan 346, sin_huge 1398.
* `-check1` (rc 1, a test `as` cast), `-check2` (rc 101: tan fallback 32.6%, fixed), `-check3` (rc 0, final WIP
  head: clippy -p liella-math -p liella-boundary --all-targets -D warnings clean; 5/5 quick tests).

NOT run yet: `cargo xtask static`, `cargo xtask kernel-index` (KERNELS.md likely needs regenerating: new kernel
module), the direct gate, plants, counts, timing.

## Next step (exact)

1. `cargo xtask kernel-index` + `cargo xtask static` on keke r5; full `cargo clippy --workspace --all-targets
   --all-features -D warnings`.
2. A boundary test for the FMA build (as `crates/boundary/tests/exp_quick.rs`: `selftest` hook calling
   `trig::quick::fused::*_quick` when `dispatch::fma()`), checking bit-equality with the CR path; the module tests
   cover SSE2 only.
3. Plants file (brief): quadrant off by one (`assemble(n + 32` -> `n + 31` in `cos_quick`), table index shifted
   (`SIN_K_PI64[i & 127]` -> `[(i + 1) & 127]`), near constant halved (`C_Q` 0x3ff0_2100... -> 0x3ff0_1080...),
   Payne-Hanek third product dropped (`let a2 = m * u128::from(w2);` -> `let a2 = 0u128;`); each with
   `msg:quick_(matches|error).*|panicked`.
4. Direct gate, counts (cbench_libm `--compare --counters`, plain and `-FMA`), D-10.61 row, then timing booking
   (IVB kotori/nico, N100 nozomi) of sin/cos/tan/sin_huge.

## Not to redo

The base profile (above). The `π/64` one-step table choice is argued in the module doc (re-open only with counts).
