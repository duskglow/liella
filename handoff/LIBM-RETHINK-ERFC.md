# LIBM-RETHINK-ERFC: erfcf/erff/erf at tiny and subnormal x (handoff + report)

State: branch task/libm-rethink-erfc-3 (continues -2 and -1; head: `git log -1`; code head 7285df2cd, later commits
are the record, handoff and report). Decision in force: D-10.107 (records/decisions/D-10/D-10.107.toml). Last run:
LIBM-RETHINK-ERFC-3, role mei:~/w/s2. Plants file: handoff/LIBM-RETHINK-ERFC-plants.txt (with the tool's results).

## What is done (do not redo)
* -1/-2: exact direct paths `erf_tiny` (subnormal erff results, \|x\| ≤ ERF_TINY_MAX = 0x716fe1) and `erfc_tiny`
  (erfcf, 0 < \|x\| < 2^-26), proved in D-10.107; -2 fixed ERF_TINY_MAX (0x716ff6 raised underflow on a normal
  result) and the stale dispatch expectation.
* -3 took the bands off the typical path. `eval` is back to its base form (a refusal is `None`); the entries
  `fast32::erf`/`erfc` (fast32.rs, hand-written in place of `fast_fn!`) call `erf_refused`/`erfc_refused`
  (erf.rs, `#[inline(never)]`: direct path inside its band, else the accurate path) from the refusal arm where
  they called `special::erf`/`erfc`. Disassembly of the entries (validation products, static libm.a, base
  c32adc349 vs 7285df2cd): 183 and 264 instructions as base, identical except the refusal arm's callee.
* Tried and dropped: `#[cold]` on the refusal functions: LLVM stopped duplicating the rounding block into one erfc
  piece (264 -> 265 instructions, erfcf_typ +0.2/call). The band inside `eval` (-2): +8.0 / +5.0.
* Tests: `erf_fast_path_agrees_with_the_accurate_path` checks the entry on the hard operands and erf_tiny's
  band-end edges: entry = accurate path, and inside a band `eval` refuses and the entry's `Status::hw` equals the
  direct path's (the accurate path leaves HW_ANY), so a skipped direct path is caught. The exhaustive sweeps use
  `exhaustive_oracle_by` (fast32/tests.rs) with `eval` else the direct path: what the entry answers before the
  accurate path.

## Results (code head 7285df2cd, mei; labels in ~/logs/jobs.tsv there)
* lint0 (inherited head 2e376df00): clippy + static rc 0. quick2 (3bf382a5c, same code): clippy, static, erf/oracle
  tests rc 0.
* plants (8): 8 CAUGHT, 0 SURVIVED, 0 WARN (3 new: each entry skipping the direct path, erf_refused's fallback).
* sweeps: every float in every mode, erff and erfcf, against the tiered oracle: rc 0, 1771 s (erff 17112756528 and
  erfcf 12923433596 of 17179869184 calls answered before the accurate path).
* percall3 (validation/percall/libm_erf_tiny.c, instructions per call, glibc / base c32adc349 / head): erff_typ
  127.9 / 161.4 / 161.4; erfcf_typ 145.8 / 173.7 / 173.7; erfcf_tiny 35.0 / 11789.2 / 129.3; erff_sub 47.0 /
  13151.0 / 120.0; f64 erf_sub 44.6 / 265.9 / 265.9 and erf_tiny 35.0 / 232.0 / 232.0 (unchanged, not in scope).
* gate (affected --run on 7285df2cd): rc 0, 6 suites ok, 1570 passed, 0 failed, 254 ignored; peak 3.2G.
* Superseded: percall (7fd476e75, `#[cold]`: erfcf_typ 173.9), percall2 (rc 2: tree synced dirty, the plants file
  was uncommitted), quick (7fd476e75: the test asserted a refusal past the band end, where `eval` answers).

## Open / next step
1. f64 `erf` for \|x\| < 2^-55 (232 against 35) and subnormal x (265.9 against 44.6): round(c·m) with a 128-bit c,
   exact by D-10.107's bound; open: raising exactly inexact/underflow in the in-place nearest entry. A new story.
2. The typical rows stay above the reference (161.4 vs 127.9, 173.7 vs 145.8): the cores, not the bands; a
   separate story if wanted.
