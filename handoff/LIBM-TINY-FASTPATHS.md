# LIBM-TINY-FASTPATHS (handoff + report)

State: branch task/libm-tiny-fastpaths, head = the commit adding this note (code head bbf6d8488). INCOMPLETE: one
family of five done (exp family, tiny operands); final-head jobs were still running at hand-back. Decision in
force: D-10.108 (amends D-10.63). Run stopped at the context checkpoint (Effort: max budget, 180k).

## Done: expf / exp2f / exp10f, normal |x| < 2^-27

Path traced from code (fast32/exp_log.rs): `exp_onepass` refuses |x| < 2^-26 (ONEPASS_AMIN) on the bits, so tiny
operands left via `elem32_onepass_cold` -> `elem` -> Real::exp -> fast32::eval (exp_core answers ~1, `round`
refuses within delta of 1) -> exp_log accurate path. Now the refusal branch answers a normal |x| < 2^-27 as
RN/RD/RU/RZ of the `double` 1 + x converted in the caller's direction (proof in the code and D-10.108; 2^-27 not
2^-26 because 10^x - 1 reaches the midpoint 1 - 2^-25 near x = -2^-26.2). Zeros and subnormals keep the envelope.

Probe (validation/libm_tail on origin/task/libm-tail-probe, x32 mode = every float input, batches of 1024; mei;
instructions per call, Liella / glibc). Before = LIBM-TAIL-PROBE binades-f32.tsv.

| band | before | after (bbf6d848 parent, cold call) | glibc |
|---|---|---|---|
| 2^-126 <= \|x\| < 2^-60 | 233 | 22 (exp10f 26) | 26-32 |
| 2^-60 <= \|x\| < 2^-43 | 832-928 | 22-26 | 26-32 |
| 2^-43 <= \|x\| < 2^-27 | 113-116 | 22-26 | 26-32 |
| 2^-27 <= \|x\| < 2^-26 (unchanged path) | 113 | 131-134 | 26-32 |
| kernel range | 46 / 46 / 50 | 48 / 48 / 52 | 26-32 |
| mean over all inputs (expf) | 240.5 | 132.4 | 27.2 |

Factor in the tiny band: 0.8x glibc. The "after" run had the tiny path as an out-of-line cold call, which made the
entry non-leaf (+2 in the kernel range, +18 on NaN/huge). bbf6d8488 inlines it; probe2 re-measures (must show the
kernel range back at 46/46/50 and the NaN band [128..128] back near 20). If it does not: revert to the parent's
`return None` and keep the tiny path behind a cheaper test.

Jobs: lint2/gate2 (rc 0) and plants2 (2 CAUGHT) passed on 1f06a3e05 (code = cold-call variant); gate (rc 1: only
`static`, a bare `|` in the record, fixed) and plants (2 CAUGHT) on e467e8cbd. Sweeps never completed: two runs
were stopped to sync newer heads (all 2^32 inputs x 4 directions x 3 functions against the envelope takes > 40 min).
Final head: lint3, gate3, plants3, sweeps3, probe2 on mei (role s3), labels libm-tiny-fastpaths-*.

## Not done (next stories, in score order from LIBM-TAIL-PROBE)

1. sin/cos/tan/sincos |x| >= 2^19 (~1300-1450 vs 60-124, 42.6%): Payne-Hanek in fixed point; plan: per-exponent
   table T[E] = (2^(E-23)*64/pi) mod 128 with 121 fraction bits (u128, built at compile time from the 2/pi bits the
   accurate path already holds), P = m*T mod 2^128 gives n mod 128 and r; reuse trig_core's evaluation (SIN_K_PI64)
   in a cold branch of trig_core_with's range test. Tiny |x| < 2^-12 (2^-30..2^-23 cost 1088-2304): RN answers x,
   1, x (sin, cos, tan) with inexact via x - x^3/6, 1 - x^2/2, x + x^3/3 in double (sketch proofs in this run).
2. Exp family huge |x| (>= 1024: 220 vs 25, ~46% of inputs; 128..1023: 655-1409): needs overflow/underflow flags
   and errno, so not one-pass; a dedicated cold path cheaper than `elem` (whose cost is ~80 + accurate path).
3. asinf |x| < 1.57e-7 (272-1179 vs 26): x + x^3/6; log1pf tiny (608): x - x^2/2; expm1f: x + x^2/2 and x <= -18
   -> -1 + tiny. log1pf/expm1f entries are `elem` (no one-pass) and 130-141 typical vs 20-81: a one-pass kernel.
4. Also seen: 2^-27 <= |x| < 2^-26 for expf/exp2f (kernel could start at 2^-27 for all three).

Plants: handoff/LIBM-TINY-FASTPATHS-plants.txt (bound raised to 2^-25, sign dropped; both CAUGHT by
math_expf_onepass_entries_match_the_envelope). No exception table exists in this change (brief's plant kinds map to
the bound = margin).
