# LIBM-F32-ONEPASS-REST: handoff (exp10f, logf, log2f, log10f on the one-pass `float` path)

State: branch `task/libm-f32-onepass-rest` (cut from origin/next df285561b, stacked on LIBM-PARTIAL-WRITE-CHECK, which
is already in that base). Code d11779338, generated files and plants bfbd374af, this note and the report on top.
Role shiki:~/w/sh1. Decision in force: D-10.72 (extends D-10.63 under D-10.67, D-10.68). Plan rows to read: D-10.63,
D-10.67, D-10.68, D-10.72.

## Done
* `exp10f`: `fast32::exp10_onepass_any` = `exp_onepass` with c = RN(128·log2 10), |x| < 37.9 (|z| < 2^13.98, so
  D-10.63's bound holds unchanged). 10^n (n = 1..=10) is a `float` while z is inexact there: operands in [1, 10] with
  their low 20 bits clear are refused on the bits first (`test` + `jcc` on the hot path).
* `logf`/`log2f`/`log10f`: `fast32::log_onepass::<BASE>`. Positive normal x only (bit test); x = 2^e·m split on the
  bits; m and e reach the `double` domain by integer operations and a full-register `movq` (e by the 1.5·2^52 shift):
  no `cvtss2sd`/`cvtsi2sd`. Kernel = `log_core`'s table (LOG_INV/LOG_C) and degree-6 Estrin. log2 = e + t·(1/ln 2), so
  x = 2^n is exact with no flag; log10 refuses x = 10^n by one load from a 32-entry table (index = exponent mod 32).
  The directed-rounding bound (≤ 2^-44.7, < 313 ulp, margin LOG_DELTA 640) is the doc comment of `log_onepass`.
* tsv rows exp10/log/log2/log10 `f` -> `mret::elem32_onepass`; math-gen, kernel-index and codemap regenerated.
* `xtask/src/partial_write.rs`: the seven ALLOW entries of these four functions removed (the check now reports
  5 findings, all allowed: acoshf, asinhf, sinf, cosf, tanf).
* Hot paths (libm.so.6, objdump on the role): exp10f, logf, log2f, log10f each straight-line to one `cvtsd2ss`, no
  call, no stack (logf ~45 instructions to the return).

## FP-op and critical-path targets (stated per the brief; counts below)
* exp10f: as expf, 13 FP ops (z, shift add/sub, r, r², 3 Estrin mul+add, S·p, cvtsd2ss), chain z -> k -> r -> r² ->
  r²·(...) -> sum -> S·p -> cvt: 9 dependent FP ops.
* log family: about 19 FP ops (e, m·L - 1, r², r⁴, 3 Estrin mul+add, 2 sums, e·LN2 + LOG_C + r + s, cvt; log2/log10
  +1 multiply); chain movq m -> m·L -> -1 -> r² -> r⁴ -> ·(...) -> s -> y -> cvt: 9 dependent FP ops, the e·LN2 + LOG_C
  part off the chain.

## Results (all on the code at bfbd374af)
* Gate shiki:f32rest-gate (affected --base origin/next): 32 suites ok, 11408 passed, 0 failed, conformance 126/126,
  peak 16G (page cache), rc 0. Lint shiki:f32rest-lint3 (codemap + static with the partial-write check) rc 0;
  f32rest-lint2 ran math-gen, kernel-index, `cargo xtask clippy` and `cargo xtask so` (partial-write: 5 findings,
  all allowed) before its static failed on the codemap only.
* Exhaustive shiki:f32rest-sweep, rc 0: exp10f, logf, log2f, log10f, each 17,179,869,184 calls (2^32 x 4 directions)
  against the envelope (bits, errno, flags): 0 mismatches each.
* Plants shiki:f32rest-plants (handoff/LIBM-F32-ONEPASS-REST-plants.txt): 8 CAUGHT, 0 SURVIVED, 0 WARN. Per function:
  reduction constant's low part dropped (INV_M, LN2, INV_LN2), refusal tests skipped (exp10 exact powers, log margin
  test, log10 10^n table), partial-write conversion reintroduced (exp10f cvtss2sd, log family cvtsi2sd: `cargo xtask
  so` fails with the finding).
* Counts shiki:f32rest-counts (cbench_libm --compare --counters; instr/call Liella vs glibc; "before" is the last
  recorded count of the old code, cited from the handoffs named, not re-run here):
  | row | before | after | glibc FMA | glibc SSE2 (-FMA) |
  | exp10f | 64.2 (libm-expf-ratchet) | 62.1 | 43.1 | 43.1 |
  | logf | 72.2 (libm-expf-ratchet) | 58.1 | 43.1 | 50.1 |
  | log2f | 78.2 (libm-f32-onepass) | 59.1 | 42.1 | 50.1 |
  | log10f | not recorded | 65.1 | 86.1 | 93.1 |
  expf 55.1 / expf_qnan 28.1 unchanged. Liella has one build of these (no FMA dispatch), the same in both runs.
  Unbooked indicative time on the build host (Liella/glibc): exp10f 1.57/1.56, logf 1.67/1.34, log2f 1.79/1.44, log10f
  0.85/0.73 (FMA/SSE2 glibc); expf 1.70/1.45 on the same run, which is 1.24 on Ivy Bridge booked: the build host
  is not the verdict.
* Timing (`~/work/LIBM-F32-ONEPASS-REST/run.sh`, staged tree libm.tar sha256 3aef4c75888fdced..., two rounds of
  `xtask-bin sweep run --only cbench_libm`, Liella/glibc median time ratio):
  - Ivy Bridge (kotori:f32rest-ivb, rc 0, load 0.2-0.4, booking #150, cancelled after): round 1 / round 2:
    logf 1.160 / 1.160, log2f 1.180 / 1.180, log10f 0.699 / 0.699, exp10f 1.441 / 1.442 (11-07 sweep: logf 2.60,
    log2f 2.35, exp10f 2.60). Target <= 1.3x: met for logf, log2f, log10f; NOT met for exp10f (1.44).
  - N100 (nozomi:f32rest-n100, booking #151): queued at hand-back, not read by this run.
* exp10f's gap, found in the disassembly: LLVM compiles the exact-power refusal (`b & 0xf_ffff == 0 && b - bits(1)
  <= bits(10) - bits(1)`) branchless (`test; sete; lea; cmp; setb; test; jne`: 7 instructions before the range test,
  vs 2 intended): 62 instructions vs expf's 55. Fix for the continuation: one test, `(b - 0x3f80_0000).rotate_right(20)
  <= 0x1a` (low 20 bits clear and b in [1, 10]; 4 instructions), then re-sweep exp10f and re-time.

## Not done
* N100 timing: read `~/logs/f32rest-n100.log` on nozomi (rows exp10f, logf, log2f, log10f), cancel booking #151.
* exp10f <= 1.3x on Ivy Bridge (the rotate test above).
* Before-counts were not re-run on this base (the role is not a git tree, so `bench --base` cannot stage a base).
* Per-function sequencing of the brief (timing before the next function) was batched: four functions, one gate, one
  sweep job, one timing run (budget).

## Next: LIBM-F32-ONEPASS-REST-2
0. exp10f: the rotate test above, re-sweep, re-time. Then sinf/cosf/tanf (ALLOW entries `cvtss2sd %xmm0,%xmm3`; operand from bits, re-prove the trig margin for directed
intermediates, only x = 0 exact), acoshf/asinhf's ALLOW entries, then powf (many exact cases: refuse on the bits or
prove the kernel exact there). Start from `onepass_scaled` and `log_onepass`'s bits split.
