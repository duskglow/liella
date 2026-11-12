# LIBM-RETHINK-ERFC: erfcf/erff/erf at tiny and subnormal x (handoff + report)

State: branch task/libm-rethink-erfc (head: see `git log -1`). Decision: D-10.107 (records/decisions/D-10/D-10.107.toml).
Role kanon:~/w/kn1. Status: INCOMPLETE (context ceiling reached; the verification jobs were still running at hand-back).

## Step 1: traces (kanon, validation/percall/libm_erf_tiny.c, wl_trace.sh, call 11)
* erfcf_tiny: 17975 instructions: fast32::erfc -> `round` refuses (y within ERFC_DELTA of the float 1) ->
  special::erfc::<f32> -> special::erf_mp (Mp::mul 52%, pack 25%, div_u64 8%).
* erff_sub: 12966: fast32::erf -> `round` refuses (subnormal result, outside [2^-126, FLT_MAX]) -> special::erf_mp.
* erf_sub (f64): 250: erf_nearest refuses \|x\| < 2^-55 (LINEAR) -> elem_fast_cold -> special::erf::<f64> ->
  fast_special::erf + fast64::settle. The probe's 1.5-3e5 is gone on next (a later story).

## Before (libm_tail probe u32/u64, kanon; Liella / glibc mean instructions per input)
erfcf tiny + 11126.8 / 21.0, tiny - 11166.8 / 21.0, typical 630.9 / 42.9; erff subnormal-result 13139.5 / 33.0,
typical 114.3 / 29.9; erf (f64) subnormal 253.6 / 30.7, tiny normal 2^-1022..2^-28 217.4 / 21.1 (u64:erf:4).
Raw: kanon ~/work/LIBM-RETHINK-ERFC/tail-before, log ~/logs/libm-rethink-erfc-base.log.

## Design (RETHINK) and change
Three approaches (D-10.107): direct exact path per band (adopted), a subnormal/representable-aware rounding test,
per-mode constant tables. Proofs by the continued fractions of 2/sqrt(pi) (scratch tool cf.py: convergents, exact
integer arithmetic): f32 n < 2^23: RN boundary distance >= 2^-24.40, directed >= 2^-23.40; f64 n < 2^52:
2^-56.36; m < 2^53: 2^-56.36 (same binade), 2^-57.36 (next binade). Code: crates/math/src/fast32/erf.rs
`erf_tiny`, `erfc_tiny`; fast32.rs `eval` calls them only when `finish` refuses (typical path unchanged).

## Jobs (kanon) and what to check
* libm-rethink-erfc-verify: clippy, static, quick erf tests, the two exhaustive sweeps (every float, every mode),
  then the after-counts (percall --base none, tail-after + bands.py). Check `SWEEP-FAILED`/`QUICK-FAILED` lines
  and the band means at the end of ~/logs/libm-rethink-erfc-verify.log.
* libm-rethink-erfc-plants: handoff/LIBM-RETHINK-ERFC-plants.txt (4 plants: erfc mode table, c precision,
  round-toward-zero step, underflow flag). No exception table exists (the bounds hold for every operand), so the
  brief's "table entry removed" plant has no target; the c-precision plant is the margin analogue.
* NOT STARTED: the gate (`cargo xtask affected --run --base origin/next` under ~/bin/gate). Start it after verify.

## Next step
1. Read verify + plants; start the gate; fix what they find. 2. f64 erf for \|x\| < 2^-55 (50% of bit patterns,
217-254 vs 21-31): RN(erf x) = round(c*m) with round(c*2^127) = 0x906eba8214db688d_71d48a7f6bfec344 (two 64x64
products, error < 2^-62 units), exact by the bounds above. Open problem: raising exactly inexact (and underflow for
subnormal results) in hardware in the in-place nearest entry: x*C_hi is exact for x = 2^k; x*C_lo underflows for
\|x\| < 2^-966; scaling, or the cold entry with a Status, are the candidates.
