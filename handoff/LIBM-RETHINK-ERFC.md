# LIBM-RETHINK-ERFC: erfcf/erff/erf at tiny and subnormal x (handoff + report)

State: branch task/libm-rethink-erfc-2 (continues task/libm-rethink-erfc; head: `git log -1`). Decision in
force: D-10.107 (records/decisions/D-10/D-10.107.toml). Last run: LIBM-RETHINK-ERFC-2, role shiki:~/w/sh1.
Status: INCOMPLETE (typical rows slower than base; see below). Plants file: handoff/LIBM-RETHINK-ERFC-plants.txt.

## What is done (do not redo)
* erff subnormal results and erfcf for 0 < \|x\| < 2^-26 answered by exact direct paths on the rounding test's
  refusal side (crates/math/src/fast32/erf.rs `erf_tiny`, `erfc_tiny`; fast32.rs `eval` calls them only when
  `finish` refuses; `round`, `finish` and the cores are unchanged). Method and proof: D-10.107 (continued
  fractions of 2/√π; Khinchin ch. II, Hardy & Wright 10.15).
* -2: the predecessor's red plants baseline (`erf_fast_path_agrees_with_the_accurate_path`, "1e-45 refused") was a
  STALE expectation (case a): for x = 2^-149 the cores answer, `round` refuses (subnormal erf; erfc within its
  margin of 1) exactly as before, and `eval` now answers by the direct paths where it used to fall to the
  accurate path. The test now keeps ±0, ±inf, NaN refused and asserts, for ±n·2^-149 (n = 1 and the 12 n whose
  c·n lies closest to a half-integer or integer on each side), that `eval` equals `erf_tiny`/`erfc_tiny` in
  every mode and agrees with the accurate path; both band ends joined the edge list.
* -2 found and fixed a defect: ERF_TINY_MAX was 0x716ff6 (c·n = 8388630.26 > 2^23), not the documented
  c·n ≤ 8388606.56 (that is n = 0x716fe1). `round` refuses n = 0x716fe2 (c·n = 8388607.69, y < 2^-126), so
  `erf_tiny` answered it with underflow although rounding up (+x) / down (-x) gives the normal 2^-126, not tiny
  after rounding (the accurate path, float.rs: tininess after rounding, as x86). Now 0x716fe1; 0x716fe2 goes
  to the accurate path (one operand per sign). The plant `erf-tiny-band-end` (0x716fe2) is CAUGHT.
* Independent exact check (scratch cf_check.py, c to 128 bits by Machin's formula, integer arithmetic): C is c
  rounded to nearest; min distance of c·n (n < 2^23) to a half-integer 2^-24.405, to an integer 2^-23.405 (the
  record's numbers); RN_double(C·n) rounds like c·n in every mode for every n ≤ 0x716ff6 (0 mismatches).

## Results (final head, shiki; labels in ~/logs/jobs.tsv there)
* lint0 (inherited head a0c354947): clippy + static rc 0. quick (d10e80d88): clippy + erf/oracle tests rc 0.
* plants (handoff/LIBM-RETHINK-ERFC-plants.txt, 5 plants): 5 CAUGHT, 0 SURVIVED, 0 WARN. The c-precision and
  round-toward-zero plants are now caught deterministically by the closest-to-boundary operands.
* gate (affected --run, d10e80d88): rc 0, 6 suites ok, 1570 passed, 0 failed, 254 ignored; peak 2.8G.
* sweeps (d10e80d88; every float, every mode, erff and erfcf, against the tiered oracle): rc 0, 1716 s.
* percall (validation/percall/libm_erf_tiny.c, instructions per call, same host; glibc / base c32adc349 /
  d10e80d88 [percall] / 102fe360e [percall3]): erfcf_tiny 35.0 / 11789.2 / 143.3 / 134.3; erff_sub
  47.0 / 13151.0 / 134.0 / 127.0; erff_typ 127.9 / 161.4 / 170.4 / 169.4; erfcf_typ 145.8 / 173.7 /
  182.7 / 178.7; f64 erf_sub 44.6 / 265.9 (unchanged); f64 erf_tiny 35.0 / 232.0 (unchanged).

## Status: INCOMPLETE, must not land as it stands
The typical rows are SLOWER than base: erff_typ +8.0, erfcf_typ +5.0 instructions per call (the predecessor's
"typical path unchanged" was never measured; the first base comparison is this run's). 102fe360e inlined the
tiny paths (they were `#[inline(never)]` and called before the accurate path's tail call): every row improved
(-1/-4 typical, -9/-7 tiny) but that explains under half; the model of the cause is incomplete. gate, plants and
sweeps ran on d10e80d88; 102fe360e changes only inline attributes and a comment (lint1 checks it).

## Next step
1. Profile erff_typ on base vs head (validation/wl_trace.sh in a git tree, `--spec
   validation/percall/libm_erf_tiny.c`) before another design. 2. Likely fix: leave `eval` exactly as on base
   (refusal = None) and answer the tiny bands in the entry's refusal arm, one `#[cold]` out-of-line function
   tail-called like base's `special::erf` (direct path, else the accurate path), so the typical path is
   base's code; then the dispatch test asserts the entry (`fast32::erf`/`erfc`) answers ±n·2^-149 by the
   direct paths, and the sweeps must run through the entry (they compare `eval` today). Re-run plants, gate,
   sweeps, percall (no row slower than base). 3. Then f64 erf for \|x\| < 2^-55 (232 / 35): round(c·m) with a
   128-bit c, exact by D-10.107's bound; open: raising exactly inexact/underflow in the in-place nearest entry.
