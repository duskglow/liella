# LIBM-F32-POWF: handoff (one-pass `powf`)

State: branch `task/libm-f32-powf` (cut from origin/next 0d0d82e5d, no merge). Code 8e913f324, plants 33b3c36c6,
record fix dc05891b6, this note on top. Role mei:~/w/s1. Decision: D-10.78 (extends D-10.63 under D-10.67).
Plan rows to read: D-10.78, D-10.63, D-10.67, D-10.68.

## 1. Profile before the change (mei, Skylake-SP; wl_trace powf_typ, call 11; job mei:libm-f32-powf-trace2)
| part | instructions | what |
|---|---|---|
| `powf` + `mret::elem` entry | 34 | MXCSR read through a call (`liella_machine_mxcsr_get`), `black_box` spills |
| `fast32::pow` before the kernel | 32 | bit tests, two `cvtss2sd` (partial writes), indirect call through the GOT to `pow_core` |
| `pow_core` | 98 | log (7-deep Horner), Fast2Sum double-double, exp; one `cvtsi2sd` (partial write) |
| `fast32::pow` after (Ziv `round`) | 48 | `cvtsd2ss` -> `cvtss2sd` round trip, a jump table on the rounding mode, status |
| `elem` exit + `mathret::exit` | 50 | flag merge, push/pop; no `ldmxcsr` in the default mode, no division |
| total | 262 | 4 calls/returns, 4 partial-register writes, one serial chain from x to the result |
No division and no MXCSR write in the default mode: the latency is the call chain, the MXCSR read and the long
serial log/double-double/exp chain with partial-write merges. Cycles on the booked timing hosts: NOT RUN (budget).

## 2. Done
* `fast32::exp_log::pow_onepass` (+ `fast32::pow_onepass_any`), `mret::elem2_32_onepass`, tsv row `pow f`
  (powf, powf32). Bit-decided before any FP operation: y = ±0 (1, x not NaN); special operands (envelope); y = 1 (x);
  y = 2 for 2^-63 <= |x| < 2^64 (x² exact in double, one conversion); x < 0 integer-y test with parity; exact
  candidates to the envelope (|x| a power of two; 0 < y < 16 with 8y an integer: the only exact cases, proof in the
  doc comment). The rest: one double evaluation, operands built from bits (no cvtss2sd/cvtsi2sd: partial-write check
  12 findings, all pre-existing ALLOW rows, none in powf), answer only for y0 in [2^-126, largest float) outside a
  4096-ulp margin.
* Error bound (proof in the doc comment of `pow_onepass`): in every direction ln|x| to 3.9·2^-52 relative (split ln 2,
  r^8 series, cases A/B1/B2 of log_core), z = (y·L)·RN(128/ln 2) with |η| <= 2^-49.32, exp kernel 2^-44.51 (|r| < 1),
  |ln f| <= 88.73 for answered results: ε <= 2^-42.45, < 1499 ulp of the double; margin 4096 >= 1.5·1499 + 1.
  Every intermediate is a normal double or an exact zero, so only inexact is raised, and only for inexact results.
* Bug fixed on the way (also in `eval_pow`): x < 0 with an odd y in [2^23, 2^24) returned a positive result.
* Found by the new test before commit: the upper bound must be the largest float, not 2^128 (above it RN and RU give
  infinity); fixed, and the plant `overflow-threshold-ulp` holds it.

## 3. Results (final code 33b3c36c6 = head minus record/handoff commits)
* Counts (mei:libm-f32-powf-counts, wl_percall, instructions/call incl. loop): powf_typ 275.3 -> 155.9 (glibc
  78.0, 2.00x), powf_int 328.2 -> 160.1 (glibc 73.1, 2.19x), expf_typ control 58.0 -> 58.0. One traced call: 138
  straight-line instructions, no call (mei:libm-f32-powf-trace3). Target (<= 1.5x glibc time) NOT VERIFIED: no booked
  timing run (Ivy Bridge, N100) in this story. Only the default (FMA) build counted, not the hwcaps=-FMA build.
* Gate 1 (mei:libm-f32-powf-gate): 2408 passed, 0 failed, conformance 16/16, peak 3.9G, rc 1 from `xtask static`
  (records: bare `|` in D-10.78's body; fixed in dc05891b6, static then clean). Gate 2 on the final head:
  mei:libm-f32-powf-gate2 (pending at hand-back).
* Plants (mei:libm-f32-powf-plants, handoff/LIBM-F32-POWF-plants.txt): 5 CAUGHT, 0 SURVIVED, 0 WARN.
* Random pairs (mei:libm-f32-powf-pairs): 10^9 pairs x 4 directions + hard pairs x 16 modes, bits/errno/flags vs the
  envelope (pending at hand-back; the test asserts 0 mismatches). The in-tier test (16 modes, hard pairs + 2^16
  random pairs per mode) passes.

## Open / next (in order)
1. Read gate2 and pairs (both rc 0 expected). Then book Ivy Bridge (nico or kotori, less busy) and the N100 (nozomi)
   for powf_typ/powf_int against base 0d0d82e5d.
2. If time is still above 1.5x: the log is the long pole (128-entry LOG_INV/LOG_C, 8-term series, needed for the
   2^-50 relative bound at |t| up to 88). A log2-domain table (t = y·log2 x with e exact) or a 2^-46 bound with a
   wider margin would shorten it; y = 3, 4 and 0.5 still go to the envelope (exact candidates): cube/square
   products or sqrt could take them on the bits.
Not to redo: the profile above; the exact-case proof.
