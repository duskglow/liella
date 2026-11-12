# LIBM-RETHINK-HYPOT: f64 hypot rethink (INCOMPLETE: design and evidence, no libm change)

State: branch `task/libm-rethink-hypot` from origin/next c32adc349; this run adds `validation/percall/hypot.c` (per-call
spec, three operand distributions), this note, the continuation brief and the report json. No product code changed.
Stopped at the brief's context checkpoint (RETHINK: 180k) after the measurement, trace and design steps.

## Done (RETHINK steps 1-4)

* Baseline counted (job `libm-rethink-hypot-pc-before` on mei, `wl_percall.sh --base none --spec hypot.c`):
  typ 110.0 vs glibc 65.9 (1.67x); near 2306.9 vs 68.8 (33.5x); bits 3703.4 vs 41.5 (89x). Instructions per call.
* Traced (same job, `wl_trace.sh`): the band is the 256-bit exact path (`pow::hypot::<f64>` 3320 of 3804). The fast
  tier refuses outside `[2^-450, 2^450]`, for exponent gaps over 60, for every root within 2^-98 of a `double`
  (exact roots, huge gaps) and in every directed mode. The typical case (99 instructions) is the fast tier.
* Designed three structures from the literature (Borges 2021 FMA-residual correction; Ziv 1991 rounding test; an
  exact wrapping 128-bit integer residual for the hard cases; an integer isqrt for two subnormal operands) and
  estimated each; the chosen design, its proofs-in-outline, wiring and the done-criteria are the continuation
  brief: `handoff/LIBM-RETHINK-HYPOT/LIBM-RETHINK-HYPOT-2.md`.
* Read the current code only after the design (mathret.rs `Hypot for f64` / `elem_fast`, invtrig.rs
  `hypot_nearest` / `sqrt_dd` / `settle`, pow.rs `hypot`). Kept: `sqrt_dd`, `EPS_HYPOT`'s 2^-104 proof, the
  NaN/inf handling of `pow::hypot`. Replaced (in the design): the window, the refusals, the 256-bit fallback.

## Not done / next step

Implement LIBM-RETHINK-HYPOT-2 (the brief above): kernel in `crates/math/src/invtrig.rs` (`kernels!`, both builds),
`hot::hypot_rounded` for the slow closure, tests, plants, D-10.104 record (not created in this run), then
`wl_percall.sh --spec validation/percall/hypot.c` before/after. hypotf: not in the probe's bad list; not changed.

## Do not redo

The baseline count and the trace above; reading the tail-probe handoff (hypot row: 98% of bit patterns at
3712-3840 vs glibc 26); the design comparison.

Plan rows to read: D-10.13 (256-bit hypot), D-10.22 (envelope/DAZ/FTZ), D-10.25 (FMA build); saturation §22.
Full report: this file and handoff/reports/LIBM-RETHINK-HYPOT.json.
