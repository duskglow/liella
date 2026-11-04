## 16. f64 exp/log/pow: where the 3.4-5.4x over glibc goes (MATH-M6-F64-GAP-BREAKDOWN, 2026-10-24, counted)

Head e7dfac98 (MATH-M6-F64-FAST-ENTRY-3 code), the static `target/liella/libm.a` + `libc.a` that `cargo xtask bench` links,
The Zen 1 host (AMD EPYC 7551), round to nearest, operands uniform over the cbench_libm ranges (exp/exp2 [-10,10]; logs
[0.01,100]; pow x [0.01,10], y [-10,10]). No library code changed.

**Method.** (1) Exact instructions:u per call: `perf stat` of a probe at R and 2R passes over 4096 operands, difference
divided by the calls (includes the caller's loop, about 4). It reproduces the bench (exp 344.0 vs 344.7, log 295.4 vs
295.7, pow 761.5 vs 756.2); glibc's on the same probe: 67.0 / 51.0 / 63.0 / 63.0 / 102.8 / 141.0. (2) Per function:
`perf record -e instructions:u -c 100003` (40k-150k samples per function), the binary split into basic blocks from
`objdump -d`, a block counted as run once per call when its sample density is at least 10% of the entry block's; this
attributes 91-94% of the exact totals (the rest, "unattributed", is the Ziv fallbacks to the accurate path plus blocks
under the threshold; the fallback rate itself was not counted). Sampled per-instruction shares were NOT used as counts:
on Zen 1 the skid piles samples after slow instructions (stmxcsr). (3) Within a function (no line tables survive in
libm.a): the function's count split over its phases in proportion to the source's operation count (Fast2Sum 3, 2Sum 6,
Dekker product 17, `Dd::add` 20, `Dd::mul` 22, `Dd::mul_f64` 21 flops), so phase rows inside one function are estimates
(marked ~); function totals are measured. Scripts: probe.c, run.sh, run2.sh, exec.py on that host
`~/work/math-m6-f64-gap-breakdown/` (jobs `math-m6-f64-gap-breakdown-phases`, `-blocks`).

**What the measured functions show.** In libm.a the kernel pieces are separate calls (`fast64::exp_kernel` 137,
`exp_log::reduce` 106, `fast64::settle` 20-26, `fast64::log_kernel` 227-233, `fast64::pow` 501 with log_pow and the exp
reduction inlined, `liella_machine_mxcsr_get` 3); about a third of the executed instructions are loads, stores and
moves (double-doubles passed through memory, constants reloaded), and `reduce` runs its full double-double chain for
`exp` although `x.lo = 0`.

**First-principles minimum** (a correctly rounded round-to-nearest fast path at about 2^-68..2^-70 relative, Ziv's
two-step strategy [Ziv 1991]; table-driven reductions [Tang 1989, 1990]; a rounding test of one multiply, one add and
one compare with a precomputed constant [de Dinechin, Lauter, Muller 2007; CRlibm report 2006]; double-word operation
costs and bounds [Handbook of Floating-Point Arithmetic, Muller et al. 2018; Joldes, Muller, Popescu, TOMS 2017]).
Two columns: baseline x86-64 (SSE2, no FMA, Liella's default build) and with FMA (2MultFMA = 2 instructions; the
x86-64-v3 build, `target/liella-v3`). The literature's claim that a CR fast path costs about what a faithful one does
[CRlibm report; Sibidanov, Zimmermann, Glondu, "The CORE-MATH Project", ARITH 2022] holds in this budget: what a CR
path adds over a faithful one is the rounding test (3-5) plus carrying the leading terms in double-double (10-30
without FMA). glibc's totals (67 exp, 63 log, 141 pow, about 4 of each being the caller's loop) are the cost of a
complete fast path on this machine; the minimum below for exp with FMA (about 64) sits at glibc's, without FMA about
90 (1.35x). So parity on Zen 1 is reachable only with FMA or a structure that needs fewer double-double steps.

### 16.1 exp (Liella 344, glibc 67; 5.1x)

| phase | Liella | measured in | min (no FMA) | min (FMA) | gap (no FMA) |
|---|---:|---|---:|---:|---:|
| entry/envelope + filter (MXCSR read, range check, call setup) | ~43 | `exp` 47 (~40 of it), mxcsr_get 3 | 10 | 10 | 33 |
| argument reduction (k, x − k·ln2/64) | ~116 | `reduce` 106, `exp` ~7, range check 3 | 12 | 10 | 104 |
| table lookup 2^(j/64) | ~6 | `exp_kernel` | 3 | 3 | 3 |
| polynomial (r + r²/2 exact, tail deg 5) | ~58 | `exp_kernel` | 28 | 16 | 30 |
| result assembly (t + t·p as double-double, 2^m) | ~67 | `exp_kernel` ~61, `settle` ~6 | 28 | 13 | 39 |
| rounding test (eps, settle's compare) | ~29 | `exp_kernel` ~9, `settle` ~20 | 5 | 5 | 24 |
| exit + caller loop | 4 | `main` | 5 | 5 | 0 |
| unattributed (fallbacks, sub-threshold blocks) | 21 | exact − attributed | 0 | 0 | 21 |
| **total** | **344** | | **~91** | **~62** | **253** |

### 16.2 exp2 (Liella 269, glibc 51; 5.3x)

| phase | Liella | measured in | min (no FMA) | min (FMA) | gap |
|---|---:|---|---:|---:|---:|
| entry/envelope + filter (incl. the integer test) | ~43 | `exp2` 83 (~40), mxcsr_get 3 | 12 | 12 | 31 |
| reduction (x − k/64 exact, r·ln2 by `Dd::mul_f64`) | ~46 | `exp2` ~43, `exp_kernel` 3 | 25 | 8 | 21 |
| table | ~6 | `exp_kernel` | 3 | 3 | 3 |
| polynomial | ~58 | `exp_kernel` | 28 | 16 | 30 |
| assembly | ~66 | `exp_kernel` ~61, `settle` ~5 | 28 | 13 | 38 |
| rounding test | ~24 | `exp_kernel` ~9, `settle` ~15 | 5 | 5 | 19 |
| exit + loop / unattributed | 4 / 22 | | 5 / 0 | 5 / 0 | 21 |
| **total** | **269** | | **~106** | **~62** | **163** |

### 16.3 log, log2, log10 (Liella 295 / 332 / 345; glibc 63 / 63 / 103)

`log_kernel` is shared (227-233 measured): ~62 reduction (integer decompose, index, Dekker `m·c`, 2Sum, range check),
~5 table (`LOG_C`, `LOG_L`), ~65 polynomial (deg 7 tail, Dekker `rh²`, 2Sum), ~89 assembly (`e·ln2` by `Dd::mul_f64`,
two accurate `Dd::add`), ~6 eps.

| phase | log | log2 | log10 | min log (no FMA) | min (FMA) | gap log / log2 / log10 |
|---|---:|---:|---:|---:|---:|---:|
| entry/envelope + filter | ~23 | ~25 | ~18 (incl. powers-of-ten test) | 10 | 10 | 13 / 15 / 8 |
| reduction (e, m, r = m·c − 1 exact) | ~62 | ~63 | ~63 | 10 | 3 | 52 / 53 / 53 |
| table | ~5 | ~5 | ~5 | 3 | 3 | 2 |
| polynomial | ~65 | ~66 | ~66 | 29 | 21 | 36 / 37 / 37 |
| assembly (e·ln2 + log c + log1p r; log2/log10: × 1/ln b as `Dd::mul`) | ~89 | ~123 | ~133 | 12 | 10 | 77 / 111 / 121 |
| rounding test (eps + settle, inlined in the wrapper) | ~20 | ~20 | ~35 | 5 | 5 | 15 / 15 / 30 |
| exit + loop / unattributed | 4 / 27 | 4 / 26 | 3 / 22 | 5 / 0 | 5 / 0 | 26 / 25 / 20 |
| **total** | **295** | **332** | **345** | **~74** (log2/log10 the same with 1/ln b folded into the tables) | **~57** | |

### 16.4 pow (Liella 761, glibc 141; 5.4x)

| phase | Liella | measured in | min (no FMA) | min (FMA) | gap |
|---|---:|---|---:|---:|---:|
| entry/envelope + filter (log domain, y·2^11 integral test) | ~50 | `pow` wrapper 47, mxcsr_get 3 | 12 | 12 | 38 |
| log: reduction + table | ~64 | `fast64::pow` | 13 | 6 | 51 |
| log: polynomial (r³/3 carried as two double-double products) | ~169 | `fast64::pow` | 45 | 30 | 124 |
| log: assembly | ~92 | `fast64::pow` | 15 | 12 | 77 |
| product y·ln x (`Dd::mul_f64`) + range checks | ~40 | `fast64::pow` | 25 | 8 | 15 |
| exp: reduction (on a double-double argument) | ~109 | `fast64::pow` | 15 | 12 | 94 |
| exp: table + polynomial + assembly | ~131 | `exp_kernel` 137 | 59 | 32 | 72 |
| rounding test (el, dz, eps, settle) | ~53 | `fast64::pow` ~27, `exp_kernel` ~6, `settle` 20 | 6 | 6 | 47 |
| exit + loop / unattributed | 4 / 49 | | 5 / 0 | 5 / 0 | 48 |
| **total** | **761** | | **~195** | **~123** | **566** |

### 16.5 Candidate changes, ranked (instructions saved per call summed over the six round-to-nearest rows; every row is the common path)

| # | change | saves per call (est.) | rows | proof that must still hold |
|---:|---|---:|---|---|
| 1 | Cheaper double-double assembly: Fast2Sum chains where the magnitudes are ordered (`t + t·p` with `abs(t·p) < 0.0062·abs(t)`; `e·ln2` with an `ln2.hi` of trailing zeros so `e·ln2.hi` is exact, then `log c` and `log1p r` by Fast2Sum) instead of two accurate `Dd::add` and a Dekker product | exp ~35, exp2 ~35, log ~60, log2 ~60, log10 ~60, pow ~95; about 345 | all | the kernel bounds restated term by term (Joldes-Muller-Popescu for the double-word algorithms used; every new term below 2^-80·abs(y) or added to MARGIN); `math_double_fast_entry_sweep` 0 mismatches; the worst-case lists **DONE (MATH-M6-F64-GAP-ASSEMBLY-2 with #6, `cargo xtask bench --compare --counters cbench_libm`, instructions:u per call, base = this head with the pre-assembly `fast64.rs`):** exp 344.7 -> 320.6 (-24.1), exp2 257.5 -> 233.4 (-24.1), log 295.5 -> 235.4 (-60.1), log2 332.7 -> 272.6 (-60.1), log10 345.9 -> 285.8 (-60.1), pow 756.1 -> 633.8 (-122.3, #1 and #6 together); about 351 summed; the same at `--tunables glibc.cpu.hwcaps=-AVX2` (-24.1/-24.1/-60.0/-60.1/-60.2/-122.1). No row slower. glibc 67.1/51.1/63.1/63.1/102.9/141.2 (default), so after: 4.78x/4.57x/3.73x/4.32x/2.78x/4.49x. Bounds restated in `fast64.rs` and D-10.13, mp-checked by `fast64::bounds`, 10 plants caught (term drops and the result put one bit outside each bound); `math_double_fast_entry_sweep` 0 mismatches. exp saved less than estimated (24 vs ~35); why is not measured. |
| 2 | exp reduction: a three-part Cody-Waite constant with 36-bit parts (abs(k) < 2^17, so every k·Ci is exact), x − k·C1 exact by Sterbenz, one Fast2Sum, the low part from k·C3; no trailing `x.lo` add for exp | exp ~100, pow ~90, exp2 ~30 (exp2's r·ln2 by the same split constant); about 220 | exp, exp2, pow | the `reduce` bound abs(r − exact) <= 2^-90 restated (truncation of ln2/64 after three parts: 2^16·2^-114); sweep; exp worst cases |
| 3 | One fused, fully inlined fast function per entry (no calls to `exp_kernel`/`reduce`/`settle`/`log_kernel`/`mxcsr_get`, double-doubles in registers, constants folded, `x.lo = 0` propagated) | ~25-35 each; about 180 (overlaps 1, 2) | all | bit-identical results (no numeric change): the same tests; check the disassembly has no calls on the hot path |
| 4 | Envelope: MXCSR read inline (one stmxcsr, one compare, the owner's single read kept), the fallback (accurate path + `fpu::settle` + errno) moved to a cold out-of-line function so the hot path saves no callee-saved registers; the `black_box` spill replaced by a register dependency on the MXCSR read | ~25-30 each; about 165 | all | the ordering barrier (no fast-path arithmetic before the read, unmasked-trap callers), FTZ/DAZ honoured: plants skip-ftz-daz-check and wrong-rounding-dispatch must stay CAUGHT; `s10_math_symbols` |
| 5 | Rounding test with a precomputed relative constant (nearest only: `y.hi == y.hi + y.lo·C` style) instead of computing eps from abs(tail) and settle's two adds and bit compare | ~15-20 each; about 110 | all | C >= the largest eps/abs(y) over the fast domain (2^-49·abs(T) + 2^-70 with abs(T) < 2^-24.5, so about 2^-69.9); the sweep must also report the fallback rate, which must stay below a stated cap (count it first: not measured here) |
| 6 | pow's r³/3: one exact product from the already exact `rh²` and a single-double 1/3 correction instead of `s.mul_f64(rh).mul(THIRD)` | ~40 | pow | `log_pow`'s bound (2^-48·abs(T) + 2^-100·abs(rh)) restated **DONE (MATH-M6-F64-GAP-ASSEMBLY-2):** counted with #1 in the pow row (-122.3 for both, against ~95 + ~40 estimated); not counted alone. |
| 7 | log2/log10: tables of log_b c and coefficients pre-scaled by 1/ln b instead of the final `Dd::mul` (+1.5 KiB tables each) | log2 ~32, log10 ~40; about 72 | log2, log10 | the stored double-double tables within 2^-104 (generated and checked by `cargo xtask math-gen --check`); the scaled bounds; sweep **DONE (MATH-M6-F64-GAP-LOG-TABLES):** `cargo xtask bench --compare --counters cbench_libm`, instructions:u per call base -> after (glibc): log2 268.6 -> 241.8 (63.1), log10 275.9 -> 255.7 (102.9), default and -AVX2 alike (-26.8 / -20.2, about 47 against ~72 estimated); exp, exp2, log, pow unchanged (within 0.5). Tables +2,928 B (`LOG2_L`, `LOG10_L`) +16 B (`LOG10_2_E`). |
| 8 | FMA path (2MultFMA for every Dekker product: 17 -> 2): only the x86-64-v3 build, or a run-time dispatch in the default build | exp ~45, exp2 ~40, logs ~50, pow ~110; about 335 | all, FMA hardware | the bounds already hold with or without FMA (fast64.rs header, #932); the sweep on both builds. Run-time dispatch in libm is a design choice: RULING NEEDED before a story |

Projection (not measured): 1-7 bring exp to about 100-110 and log to about 90-100 on the default build (1.5x glibc),
pow to about 260 (1.8x); with 8 on FMA hardware, about 70 / 65 / 160, near parity. Parity on the no-FMA build needs a
structure with fewer double-double steps (larger or two-level tables shrinking r and the polynomial), which is a
design question for the story after 1-7. Directed rounding (exp_upward 509, log_upward 448, pow_upward 1114) takes
the full envelope and is not covered here.
