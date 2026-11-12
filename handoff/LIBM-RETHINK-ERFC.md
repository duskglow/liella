# LIBM-RETHINK-ERFC: erfcf/erff/erf at tiny and subnormal x (handoff + report)

State: branch task/libm-rethink-erfc-2 (continues task/libm-rethink-erfc; head: `git log -1`). Decision in
force: D-10.107 (records/decisions/D-10/D-10.107.toml). Last run: LIBM-RETHINK-ERFC-2, role shiki:~/w/sh1.

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
* sweeps (every float, every mode, erff and erfcf, against the tiered oracle): PENDING.
* counts (wl_percall --base none, validation/percall/libm_erf_tiny.c, instructions per call, Liella / glibc):
  erfcf_tiny 143.3 / 35.0; erff_sub 134.0 / 47.0; erff_typ 170.4 / 127.9; erfcf_typ 182.7 / 145.8;
  f64 erf_sub 265.9 / 44.6; f64 erf_tiny 232.0 / 35.0. Before (Zen 1 host traces, LIBM-RETHINK-ERFC step 1):
  erfcf_tiny 17975, erff_sub 12966. percall against the merge-base: PENDING.

## Open questions / rulings
None for the f32 bands. Target not reached for the f32 tiny rows (4.1x and 2.85x glibc by instructions; the
fast tier's fixed cost: kernel, rounding test, then the direct path).

## Next step
f64 erf for \|x\| < 2^-55 (232 / 35 above; subnormal x 266 / 45): RN(erf x) = round(c·m) with a 128-bit c,
exact by the record's bound (2^-57.36 ulp); open problem is raising exactly inexact (and underflow for
subnormal results) in the in-place nearest entry. Plan rows: D-10.107. Then, for the f32 rows, answer the
tiny bands before the kernel (an exponent test ahead of erf_core) if the typical rows stay unchanged.
