# x87 long double libm rows: the 192-bit first step (PERF-LIBM-LONGDOUBLE)

Counted on a Zen 1 host (60 vCPU), `cargo xtask bench --compare --counters --runs 3 cbench_libm`, before
(origin/next) and after, same host, run back to back. Instructions are `instructions:u` per call.

What was done: the x87 `long double` entry points run the elementary-function code of `wide` first in a
192-bit instance of the multi-precision arithmetic (`mpn`) and use the 384-bit instance only for the results the
rounding test cannot decide (about one in 2^30 operands). `Mp::pack` funnel-shifts whole words (all widths).
Result: the exp, log, trig, hyperbolic, inverse and `pow` rows run 3.5 to 4.3 times fewer instructions; no
row of the file has more instructions or more time (minimum of three runs, 3 % tolerance).

| row | instr before | instr after | instr glibc | ratio after | ns before | ns after | ns glibc |
|---|---:|---:|---:|---:|---:|---:|---:|
| expl | 40223.6 | 9336.8 | 82.4 | 113.31 | 7031 | 1910 | 33 |
| exp2l | 40846.9 | 9497.0 | 63.3 | 150.03 | 7115 | 1922 | 27 |
| exp10l | 136532.1 | 27963.1 | 82.4 | 339.36 | 24143 | 5941 | 33 |
| expm1l | 41125.2 | 9501.2 | 70.5 | 134.77 | 7127 | 1932 | 33 |
| logl | 86867.9 | 17724.5 | 47.4 | 373.93 | 15706 | 3874 | 35 |
| log2l | 95216.6 | 20412.8 | 44.4 | 459.75 | 16418 | 4255 | 35 |
| log10l | 191368.4 | 39752.1 | 44.5 | 893.31 | 34130 | 8342 | 35 |
| log1pl | 84808.6 | 17410.1 | 46.2 | 376.84 | 15129 | 3804 | 39 |
| cbrtl | 45974.7 | 10860.5 | 158.1 | 68.69 | 6616 | 1572 | 80 |
| sinl | 96975.2 | 21602.7 | 1082.9 | 19.95 | 17365 | 4057 | 217 |
| cosl | 96968.4 | 21607.5 | 1083.0 | 19.95 | 16912 | 4058 | 218 |
| tanl | 105700.7 | 24048.7 | 1046.8 | 22.97 | 17942 | 4368 | 198 |
| asinl | 218964.8 | 61275.4 | 103.2 | 593.75 | 29915 | 9040 | 50 |
| acosl | 222579.5 | 62108.7 | 44.8 | 1386.35 | 30298 | 9173 | 67 |
| atanl | 201795.6 | 56463.6 | 21.7 | 2602.01 | 27373 | 8381 | 55 |
| sinhl | 51781.1 | 12481.7 | 126.4 | 98.75 | 8516 | 2339 | 52 |
| coshl | 51759.8 | 12460.4 | 112.3 | 110.96 | 9616 | 2300 | 65 |
| tanhl | 60412.0 | 14918.8 | 115.7 | 128.94 | 9651 | 2656 | 46 |
| asinhl | 107351.2 | 23965.7 | 73.8 | 324.74 | 18310 | 4659 | 64 |
| acoshl | 101843.1 | 21987.4 | 85.8 | 256.26 | 17255 | 4479 | 66 |
| atanhl | 94238.5 | 20088.5 | 101.8 | 197.33 | 18255 | 4145 | 79 |
| erfl | 212077.2 | 169957.8 | 189.6 | 896.40 | 36906 | 33701 | 108 |
| erfcl | 212827.2 | 170473.1 | 191.9 | 888.34 | 36938 | 33735 | 108 |
| tgammal | 624534.5 | 503888.5 | 756.9 | 665.73 | 100476 | 85987 | 471 |
| lgammal | 585851.2 | 472599.8 | 439.0 | 1076.54 | 92485 | 80452 | 268 |
| j0l | 142414.4 | 114303.7 | 2426.7 | 47.10 | 29103 | 26537 | 679 |
| j1l | 142471.9 | 114351.4 | 2432.3 | 47.01 | 28984 | 26585 | 675 |
| y0l | 595539.4 | 471906.0 | 2476.3 | 190.57 | 99236 | 88086 | 671 |
| y1l | 596040.9 | 471808.5 | 2477.0 | 190.48 | 98901 | 88062 | 676 |
| powl | 123364.5 | 25724.1 | 666.7 | 38.58 | 21683 | 5496 | 431 |
| hypotl | 4481.5 | 4480.9 | 106.1 | 42.23 | 573 | 566 | 48 |
| atan2l | 208846.8 | 58276.4 | 44.8 | 1300.81 | 28550 | 8616 | 62 |
| jnl | 142663.1 | 114496.0 | 5008.9 | 22.86 | 28893 | 26670 | 1350 |
| ynl | 598992.9 | 474287.9 | 5056.5 | 93.80 | 99375 | 88840 | 1326 |

## Why the rows stay above 1.05

* The glibc rows are x87 hardware instructions: `expl` is 82 instructions of which `fyl2x`/`f2xm1`/`fscale`
  each count as one. A software kernel that rounds correctly in every mode cannot approach that count; the
  counted floor of the present design is the rounding-decision work itself: the 192-bit exp (a series of about B/12 terms and 12 squarings) is about 9 300 instructions after this change (113 times glibc), the logarithms and
  inverse trig about 18 000 to 62 000. The instruction ratio is not the time ratio: the time ratio of `expl` after
  this change is 58 against its instruction ratio of 113 (the x87 instructions cost more than one cycle).
* Next steps by size, none done here: a table-driven exp/log (a 2^(j/64) table and a short series, which removes the 12 squarings; not measured); a two-limb (128-bit) first step for the
  functions whose proved bound is relative (exp, sin, cos, atan; the logarithms near 1 need the third limb for their absolute bound; not measured); the special functions (erf, gamma, Bessel, `hypotl`) have their own 384-bit
  series and were not touched except through `pack` (15 to 20 % fewer instructions).
* Negative result: splitting `Mp::div_u64` into two 64-bit divisions per limb for divisors below 2^32 cut
  instructions of the series rows by about 10 % and raised their time by 5 to 14 % (erf, Bessel, binary128
  series); reverted.
