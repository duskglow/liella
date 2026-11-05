# LIBM-WIDE-DD-TIER handoff (LIBM-WIDE-DD-TIER-2: exp/log slice COMPLETE)

Branch task/libm-wide-dd-tier-2 (cut from origin/next 2c1e25b9; carries all of task/libm-wide-dd-tier). Role
kanon:~/w/kn1 (Broadwell-EP). Report: handoff/reports/LIBM-WIDE-DD-TIER-2.json. Decisions in force: D-10.19
(correct rounding of the wide functions), ruling #2760 option A (double-double tier + mp fallback), #2761 (x87 rows
judged in cycles on a booked timing host).

## State
* `crates/math/src/wide_dd.rs`: double-double first tier for expl, exp2l, exp10l, expm1l, logl, log2l, log10l,
  log1pl, hooked in `wide.rs` before the 384-bit route. Bounds proved in the module doc; `decide::<F>` refuses what
  the bound cannot decide (so `_Float128` falls through). Correctly rounded in all modes; no differences row change.
* -2 added (aa1eaeae): `log10`'s exactness test as `5^e` + one comparison (was a 56-step u128 scan, ~1000
  instructions/call: log10l 2205.5 -> 1194.5); `validation/percall/libm_wide_dd.c` (count spec); two plants.
  Docs commit: `docs/saturation/21-x87-exp-log-double-double-tier.md` (counts, cycles, per-call split) + index line.
* Jobs (kanon unless noted): -1's gate `libm-wide-dd-tier-gate` was RED on clippy (fixed by 0e9e83b2 in -1); its
  plants ran on an older head. In -2 on aa1eaeae: gate2 rc 0 (affected ok, conformance 112/0), plants3 7/7 CAUGHT,
  percall3 rc 0, trace-expl rc 0; nico `libm-wide-dd-tier-2-cycles` rc 0 (booking 29, cancelled after).
  Superseded: gate (killed: an ssh foreground jobrun, restarted detached), plants2 (killed, old head), percall
  (rc 1: no cargo env). On the final head (docs only on top of aa1eaeae): gate3, plants4, percall4 started detached.

## Results (full table in saturation section 21)
Instructions/call base -> new (glibc): expl 33381 -> 1054 (87), logl 71118 -> 1162 (52), log1pl 70742 -> 1451 (51).
Cycles/call on the booked Ivy Bridge host: 27x-56x fewer than base; new/glibc 3.9 (expl) to 11.4 (log1pl).
Self-check mismatches vs glibc on log10l and expm1l are glibc's last-bit (Liella and base agree: correctly rounded).

## Where a call goes (wl_trace, expl 1039 / logl 1147)
kernel ~54-59%; `float::round::<F80>` 166; `decide` 154; boundary wrapper 71; `wide::*` entry/decode 53.

## Next step (proposed stories; the coordinator queues them)
1. LIBM-WIDE-DD-EXPLOG-TRIM: the same 8 functions. (a) Horner in mixed precision: high-order terms in `double`,
   only the last 3-4 steps in double-double (re-prove the bound term by term); (b) round the double-double straight
   to x87 bits in `decide` (skip `float::round`, ~320 = 31% together); (c) the long accuracy sweep
   (`cargo test --release -p liella-math --lib wide_dd -- --ignored`) as a job: never run yet.
2. LIBM-WIDE-DD-TRIG: sinl, cosl, tanl, sincosl. Cody-Waite with a 3-part pi/2 for |x| < 2^20 or so, `mp::reduce_wide`
   beyond; dd polynomials; tan needs a division (dd div). Same `decide`.
3. LIBM-WIDE-DD-INVTRIG: atanl, asinl, acosl, atan2l. atan(j/64) table + series on the reduced argument; asin/acos
   through atan2 or sqrt identities (watch cancellation near 1).
4. LIBM-WIDE-DD-HYPER: sinhl, coshl, tanhl, asinhl, acoshl, atanhl from the exp/log kernels here (expm1 for small
   sinh/tanh; log1p for asinh/atanh). Cancellation bounds like expm1's b = 92.
5. LIBM-WIDE-DD-POW: powl (and cbrtl by Newton). Needs a log with ~2^-120 (triple-double or dd + correction) so that
   y*log x keeps 64+ bits after exp; hardest bound; exact cases already settled in `wide.rs`.
Lessons from exp/log: pin every constant against mp in a test; measure the worst error before choosing b (expm1 was
2^-95, not the 2^-100 estimate); look at the code that runs before the tier (log10's exactness scan cost as much as
the tier itself); counts must use 64-bit operands too (`*_full`) to rule out double-widened operand shortcuts.

## Do not redo
The 192-bit middle step of task/perf-libm-longdouble was not merged; splitting `Mp::div_u64` measured slower.
