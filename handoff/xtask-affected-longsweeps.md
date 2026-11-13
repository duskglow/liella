# XTASK-AFFECTED-LONGSWEEPS
Branch task/xtask-affected-longsweeps. Change: `longsweeps` and `float_casts` added to XTASK_TOOLS (xtask/src/affected.rs).
A diff touching only xtask/src/longsweeps.rs now selects the xtask unit tests (the table's own tests), xtask clippy and
`cargo xtask static` (its long-sweeps check against all ignored/env-scaled tests), not FULL. Covers it because the registration is
checked by those two; the sweeps themselves run only in the nightly/release run (`LIELLA_NO_VERDICT_CACHE=1`), not the ordinary gate.
Unit test `tool_modules_stay_off_the_product_path` accepts it: main.rs's static_checks names both.
Not mapped (stay FULL): labnames (used by hottext), partial_write (so.rs), jcc/align/classed/remap (layout/so), copies, placeholders,
compat_objects (so/compat), spec, steptime/nextest (affected/plants own). 
Test: a_long_sweep_registration_selects_the_xtask_checks_not_full. Plants: handoff/xtask-affected-longsweeps.plants, 2 CAUGHT.
Jobs on keke n1: xla-clippy (clippy+static, rc 0), xla-gate (376 passed), xla-plants (2 CAUGHT; WARN-DEV-SHM only).
