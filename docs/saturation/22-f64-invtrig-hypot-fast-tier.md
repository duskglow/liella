## 22. f64 atan/asin/acos/atan2/hypot fast tier: counts and the residual over glibc (LIBM-F64-INVTRIG-HYPOT-TIER-3, 2026-11-05, counted)

Code: the round-to-nearest fast tier in `crates/math/src/invtrig.rs` (Ziv's first step, double-double, correctly
rounded or refused; D-10.13), wired in `crates/boundary/src/hot.rs`. Broadwell host (Xeon E5-2699 v4, 35 vCPU, FMA
build in force), `cargo xtask bench --compare --counters cbench_libm`, instructions per call, operands uniform over
the cbench_libm ranges (atan [-10,10], asin/acos [-1,1], atan2 [-10,10]^2, hypot [-100,100]^2).

| Row | before (accurate path; other CPU) | sqrt out of line | sqrt inline (kept) | glibc | ratio | wl_trace, one call: entry + mxcsr + kernel |
|---|---:|---:|---:|---:|---:|---|
| atan | 1,345 | 236.8 | 236.8 | 78.7 | 3.01 | 231 = 18 + 3 + 210 |
| asin | 1,517 | 338.4 | 325.1 | 75.0 | 4.33 | 309 = 18 + 3 + 288 |
| acos | 1,569 | 310.0 | 293.0 | 80.1 | 3.66 | 273 = 18 + 3 + 252 |
| atan2 | 1,577 | 265.8 | 265.7 | 161.7 | 1.64 | 255 = 28 + 3 + 224 |
| hypot | 3,713 | 159.4 | 153.3 | 63.8 | 2.40 | 145 = 22 + 3 + 120 |

* **Inline square root (POSITIVE, kept):** the safe `#[target_feature(enable = "sse2")]` `sqrtsd` in the boundary
  instead of the out-of-line `liella_machine_sqrtsd` saves 13 (asin), 17 (acos), 6 (hypot) instructions per call,
  no row slower.
* **The `glibc.cpu.hwcaps=-FMA` pass** leaves Liella's rows unchanged to the decimal: Liella's own FMA dispatch does
  not read that tunable, so both passes measure Liella's FMA build (glibc's no-FMA atan is 109.8, atan2 197.9).
  The Dekker build is not counted here (open).
* **Where the kernel's instructions go (first principles, from the code):** `atan_unit` is one double-double
  quotient (two divisions, one exact product, a 5-operation remainder), one 2Sum and four fast 2Sums, the table
  index with its clamp (about 12 instructions of `cvttsd2si`/`min`/`max`/`cmov`), a degree-4 polynomial and the
  double-double table add; `settle` about 15; many of the listing's instructions are `movapd` register copies
  (estimates from the code and the disassembly, not a per-block count). `|x| > 1` (90% of
  the atan operands) adds a reciprocal, its exact residual and a double-double subtraction from `pi/2`; asin/acos
  add a double-double square root (and asin a second double-double quotient) before the same `atan_unit`.
* **Residual, not closed:** every row is 1.6-4.3x glibc. Parity is not reachable by trimming this structure: the
  double-double quotient, its remainder and the table index alone are an estimated 40-50 instructions, more than
  half of glibc's whole atan.
  A 1.05x design needs fewer double-double steps: a finer table (j/256 or a two-level table, shrinking `v` so a
  degree-2 polynomial in `double` suffices), the quotient as one `double` division with the error carried in the
  rounding test's bound instead of a correction step, and asin/acos on their own tables near 1 instead of through
  `atan` of a quotient. That is the next story's design question; this run measured and recorded only.

### Folded constants and a bit-read index (LIBM-F64-INVTRIG-HYPOT-TIER-5, 2026-11-05, POSITIVE, kept)

Per-phase split of the FMA build's `atan` kernel for `|x| > 1`, counted from Liella's own listing (static binary,
`objdump`): about 131 instructions = range check 7, reciprocal and its residual 5, table index `nearest_int` plus
the float-to-int conversion and clamp 16, `1 + c·t` as a double-double 11, the double-double quotient 10, polynomial 8,
table add 8, the separate `π/2 − r` double-double subtraction 15, `settle` and the sign 37 (plus spills). The two
pieces cut here: the constant `π/2`, `π` (and `π/2` added) folded into three const tables built from `ATAN_J64` at
compile time, so `atan` for `|x| > 1`, `asin` near 1 and three `atan2` quadrants end in one table add with a sign;
and the index read from the low bits of `64 th + 1.5·2^52` (the same `j`). Same host and command as above, before =
the merge with next, instructions per call:

| Row | before | after | glibc | ratio after |
|---|---:|---:|---:|---:|
| atan | 236.8 | 217.3 | 78.8 | 2.76 |
| asin | 325.1 | 314.7 | 75.1 | 4.19 |
| acos | 292.9 | 286.1 | 80.2 | 3.57 |
| atan2 | 265.8 | 248.8 | 161.8 | 1.54 |
| hypot | 153.4 | 153.4 | 63.9 | 2.40 |

No row slower; `|x| <= 1` results bit-identical, the rest under the same bound. Still open, the remaining structure:
`settle` (37) is now the largest single phase of `atan`; then the double-double quotient and `1 + c·t` (21), which
the single-`double`-quotient design above replaces only together with a finer table.
