# LIBM-RETHINK-GAMMA: handoff (run 1, INCOMPLETE at the context checkpoint)

State: branch `task/libm-rethink-gamma`, fast-forwarded to origin/next b888f8ff (the branch was cut on 11-12 while
the story was held for LIBM-F32-REST-NEXT-3; it had no commits). Code head: see handoff/reports/LIBM-RETHINK-GAMMA.json.
Role keke:~/w/r5. Reserved decision id: D-10.105 (record NOT written yet).

## Step 1: traced bands (probe tool, `~/work/libm-rethink-gamma/before` on keke; instr/call, Liella / glibc)
One input per 2^16-block (f32) / per 1/64 binade (f64), validation/libm_tail/run.sh, band summary by my bands.py:
| band | Liella before | glibc | path today |
|---|---|---|---|
| f64 lgamma 0<\|x\|<2^-60 | 393 275 | ~117 | fast_gamma::domain stops at TINY=2^-60 -> 384-bit lgamma_accurate |
| f64 tgamma 0<\|x\|<2^-60 | 426 160 | ~142 | same (domain kept \|Γ\| <= 2^61 for settle) |
| f64 lgamma x>=16 | 79 395 | 121.6 | domain stops at HI=16 -> 384-bit |
| f64 lgamma/tgamma x<=-16 | 11 525 / 11 609 | 127 / 133 | 384-bit (non-integers in (-2^52,-16]) |
| tgammaf 0<\|x\|<2^-20 | 428 777 | ~165 | gamma_core domain \|x\|>=2^-20 -> 384-bit |
| tgammaf x<=-34 / x>=35 | 36 289 / 13 707 | 144 / 137 | fast32::round refuses subnormal and overflowing results -> 384-bit |
| lgammaf x<=-42 | 35 494 | - | gamma_core domain -> lgamma_edge -> fast_gamma::domain false -> 384-bit |
| lgammaf tiny | 421.5 | 87-99 | D-10.87 lgamma_edge (landed after LIBM-TAIL-PROBE): already off the 384-bit path |

## RETHINK design (before reading the code)
A (adopted): per-band closed forms in double-double with a proved bound, Ziv's test (fast64::settle) for f64; for
f32 the same values rounded by the offline-proved fast32 rounding. Tiny: lnΓ(x) = -ln|x| - γx + O(x²), Γ(x) = 1/x - γ
+ O(x) (DLMF 5.7.1/5.7.3); large: Stirling (DLMF 5.11.1, remainder < first omitted term, 5.11.ii); large negative:
reflection Γ(x)Γ(1-x) = π/sin(πx). Bound: ~log_dd + 20-40 instructions (tiny), ~log_dd + 60 (Stirling).
B (rejected): extend the LG_T piecewise-Taylor tables over the new ranges: ~1000 binades of slots up to the overflow
(MBs) while Stirling needs <= 4 terms above 2^10; tiny |x| reduces to the series anyway. C (f32 only, open): plain
double + exception table from the exhaustive sweep instead of double-double; decide by the sweep's failure count.

## Done (run 1)
* fast_gamma: `lgamma_tiny` (log_dd), `tgamma_tiny` (exact residual via two_prod, 2^512 scaling below 2^-512),
  `lgamma_large` (Stirling, 10 terms, 1/(12x) in dd; x*2^-512 above 2^512 because Dekker's split overflows above
  2^996), `lgamma_band`/`tgamma_band` (overflow for x >= 2^1015); wired in special::lgamma/tgamma after the table tier.
* tests/gamma_bands.rs: bits, status and sign identical to the 384-bit path in all four modes on spread operands
  and band edges, plus the share answered by the band form.
* Jobs: libm-rethink-gamma-check (rc 1: missing_docs), -check2 (rc 1: trace IDs), -check3 (clippy, static,
  gamma_bands + fast_gamma tests) on the final head.

* check3 result: clippy and static pass; fast_gamma 4/4; gamma_bands: every operand AGREES with the 384-bit path
  (bits, status, sign, four modes), the large-x test passes (>= 98% by the band form), the tiny test fails at
  its tgamma share assert (line 64, < 85% of tiny operands answered by tgamma_band). NOT DIAGNOSED. Suspects:
  settle's slack (2^-100·|y.hi|) against lo once |x| < 2^-100 (lo/q tiny when r is small), or the 2^512 scaling.
  Print the refusal rate per exponent first.

## Not done / exact next step (brief handoff/LIBM-RETHINK-GAMMA/LIBM-RETHINK-GAMMA-2.md)
1. Gate (`cargo xtask affected --run`, base origin/next), plants (Stirling coefficient sign; γ dropped in
   tgamma_tiny; margin halved = equivalent: the bounds are conservative), after-probe:
   `jobrun ... bash ~/work/libm-rethink-gamma/probe-run.sh after` and compare with `before`.
2. D-10.105 record (`cargo xtask records new decision D-10.105`), method as above with the citations.
3. Bands left: f64 x <= -16 (reflection, needs sin(πr) in dd), f64 lgamma [2^1012, 2^1015) and tgamma subnormal
   x (settle refuses m > 1022: add an m = 1023 step with overflow), tgammaf tiny (1/x - γ + c1·x in double, round
   with margin; powers of two x = ±2^-k, k >= 53, sit on a float: decide by the sign of -γ, no table), tgammaf
   x <= -34 / >= 35 and lgammaf x <= -42 (fast32::round has no subnormal/overflow results: add them, or reuse
   the f64 band kernels through lgamma_edge as D-10.87 does).
## Not to redo
The before measurement; the trace of every band (table above); the design comparison.
