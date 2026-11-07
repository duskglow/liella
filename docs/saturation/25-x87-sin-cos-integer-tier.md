## 25. x87 long double sin/cos: the integer fixed-point first tier (LIBM-LD-TRIG-INT, -2, 2026-11-07, Skylake-SP counts, Ivy Bridge and N100 cycles)

`crates/math/src/wide_int.rs` (D-10.52; method and measurements D-10.56): `sinl` and `cosl` (and `sincosl`, which
calls both) evaluated in 64-bit limbs with a proved relative bound `2^-94`, Ziv's rounding test as an integer test
(bound `2^-92`), the 384-bit route behind it. Results stay correctly rounded in every mode. Spec §10.3, D-10.19.

**Counts.** `validation/percall/libm_ld_trig.c` (64-bit significands in [-10, 10], near 1e10 and near 1e300),
`validation/wl_percall.sh`, Skylake-SP build host (Xeon Platinum 8151), instructions:u per call (sd 0), base =
bb7f6ae1c (`next`: the 384-bit route on every call), new = a3abee27b, glibc 2.39 dynamic.

| Case | Instr base | Instr new | Instr glibc | new/glibc |
|---|---|---|---|---|
| `sinl` | 82 228.5 | 582.2 | 1127.2 | 0.52 |
| `cosl` | 82 227.4 | 583.3 | 1126.7 | 0.52 |
| `sinl_1e10` | 73 612.7 | 588.2 | 1104.9 | 0.53 |
| `cosl_1e10` | 73 611.7 | 589.2 | 1103.9 | 0.53 |
| `sinl_1e300` | 74 203.1 | 586.6 | 1093.5 | 0.54 |
| `cosl_1e300` | 74 202.0 | 587.7 | 1096.3 | 0.54 |
| `sincosl` | 164 371.0 | 1080.6 | 1227.5 | 0.88 |

**Cycles.** The same binaries (Liella static; glibc dynamic, against the timing host's own glibc 2.39), cycles:u per
call, `perf stat -r 5`, (C(N) − C(0))/N with N = 200 000, glibc and Liella interleaved per case, three rounds, the
median shown (the rounds agree within 2% but for one Liella round of `sinl_1e300` on Ivy Bridge, +14%), booked idle
timing hosts. The instruction counts there equal the build host's.

| Case | Ivy Bridge (E3-1220L v2) glibc | Ivy Bridge new | new/glibc |
|---|---|---|---|
| `sinl` | 547.6 | 264.9 | 0.48 |
| `cosl` | 551.3 | 263.6 | 0.48 |
| `sinl_1e10` | 550.3 | 277.5 | 0.50 |
| `cosl_1e10` | 551.1 | 276.2 | 0.50 |
| `sinl_1e300` | 549.9 | 276.6 | 0.50 |
| `cosl_1e300` | 552.7 | 275.9 | 0.50 |
| `sincosl` | 630.2 | 466.8 | 0.74 |

The N100 run of the same script and binaries finished after this section was committed: its table is in the story
report, `handoff/reports/LIBM-LD-TRIG-INT-2.json` (`informational`).

**Where one call goes** (`validation/wl_trace.sh`, call 11 of `sinl`, new): 560 instructions. `wide_int::kernel` 371
(66%: the reduction, the table step, both polynomials, the two table products); `wide::sin` 86 (15%: decode, special
cases, the tiny-argument test, the integer rounding); the boundary wrapper `mathret::elem` 71, `sinl` and
`liella_sinl_bits` 23, the MXCSR get/set and x87 control-word read 9 (together 18%, the same wrapper as section 21's
`expl`).

**What was tried.**

| Change | Result |
|---|---|
| Integer fixed-point tier, proved bound `2^-94`, rounding test `2^-92`, 384-bit fallback | kept: sinl 82 228 -> 582 instructions (141x fewer), 0.48x glibc's cycles on Ivy Bridge; 10^7 seeded operands in 4 exponent bands (8x10^7 calls in four modes): 0 undecided, every result equal to the 384-bit route, worst error `2^-95.68` |
| One Payne-Hanek path for every `\|x\| >= 2^-34` (no Cody-Waite branch for moderate `\|x\|`) | kept: the 256-bit window costs four products at every exponent; near 1e300 counts within 1% of [-10, 10] |

**Not saturated.** The brief's target (300 instructions) is not reached: 582. Open, in expected order of gain:
(1) `sincosl` reduces twice (1080.6, two calls of 540): one reduction and table step for both results; (2) the shared
x87 entry (wrapper, decode, special cases: 189 instructions, a third of the call; every x87 function pays it);
(3) the kernel's variable-count 128-bit shifts (it normalises `d`, then shifts `u` and `S(d)` back) where `j >= 1`
needs only absolute precision (the result is at least `sin(π/512)`); (4) two checks per table word in the reduction
(`checked_sub` and the bounds check). Not tried: `mulx`/`adcx`/`adox` variants (D-10.25's dispatch pattern).
