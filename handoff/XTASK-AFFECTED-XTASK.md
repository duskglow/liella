# XTASK-AFFECTED-XTASK (#4684): an xtask tool-module change does not escalate the gate to FULL

State: branch task/xtask-affected-xtask, base origin/next. Code commits a7331e5ed, 4fa7dac05 (+ this handoff/report).
No decision record (the brief reserved no D-id; the rule is in the module doc of xtask/src/affected.rs, rule 1 and 7).

## Why it was FULL
`classify` sent every `xtask/` path except affected.rs, the XTASK_AUDITS leaves and test-only diffs (D-X.41) to
FULL ("xtask (its commands are part of the gate)"): the conservative reading that any xtask module may run inside the
product build or a gate step.

## What changed (xtask/src/affected.rs)
- `XTASK_TOOLS`: the modules off the product path (a file `xtask/src/<m>.rs` or a directory `xtask/src/<m>/`):
  records, kernel_index, register, compat_ids, codemap, compat_plan, compatprobe, privpaths, locks, fuzz, arch
  (each covered by a check of `cargo xtask static`), and bench, benchrows, benchstage, abbench, sweep, repro, symcov
  (commands no gate step runs). A change selects the xtask unit tests, the xtask clippy and `cargo xtask static`.
- Still FULL: so, hottext, coldbss, layout, csys (+ csys/), the generators (abi, unicode, langinfo, native, compat,
  math), nextest, native_examples, rust_dylib, spec, trace, labnames (used by hottext), steptime, main.rs, any module
  not in the table (a new module), xtask/Cargo.toml. hottext.rs/coldbss.rs/so.rs still select the start-up layout tests
  (IMAGE_TOOLS unchanged); XTASK-AFFECTED-LAYOUT and -LOADER rules untouched.
- Guard: unit test `tool_modules_stay_off_the_product_path`: a tool module may be named (`m::` after `crate::`/
  `super::`/a non-path char, or in a `crate::{...}` import) only by main.rs, affected.rs, other tool or audit modules,
  or an `XTASK_TOOL_USES` row (math -> kernel_index, math -> records: `math::run_tables`, static's tables-fresh; trace ->
  records: static's trace); the full gate's `test_steps` may call a tool module only if `static_checks` calls it too;
  every table row names a real module and every use row a real use.
- Tests: a_records_only_change_selects_the_xtask_checks_not_full, a_product_build_path_change_still_selects_full,
  a_hottext_change_selects_the_layout_tests, tool_modules_stay_off_the_product_path.
- Plants: handoff/XTASK-AFFECTED-XTASK-plants.txt (6: narrowing removed, directory module lost, static dropped, so as a
  tool, an unlisted use, hottext out of IMAGE_TOOLS).

## Results
See handoff/XTASK-AFFECTED-XTASK-report.md.

## Open / next step
- #4684's own case, a change to hottext.rs's repair command, stays FULL: hottext.rs is one module and so.rs calls its
  `group` (the product build), so module granularity cannot tell the repair command from the hot-text grouping.
  Recommended next story: move the repair command (`hottext::repair_cmd` and what only it uses) into its own module,
  e.g. `xtask/src/hottext_repair.rs`, and add it to XTASK_TOOLS (its test then proves the split). That move's own gate
  is FULL once (it touches hottext.rs).
- A finer, function-level rule (hunks inside functions that only tool commands reach) would generalise this; not done
  (S-size story).

Plan rows to read: D-X.41 (xtask test code), XTASK-AFFECTED-LAYOUT / -LOADER handoffs.
