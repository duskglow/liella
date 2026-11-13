# CONF-DYN-HELPERS (DL-16H): a dynamic mode for the conformance harness

**State.** Branch `task/conf-dyn-helpers-2` (continues `task/conf-dyn-helpers`), **based on
`origin/task/dl-libc-start-10`** (merged again at 84fb8d645 on 11-13): on next a dynamically linked program cannot
start under Liella's ld.so (no run-path search, no dynamic `__libc_start_main`). My commits are separate from
DL-LIBC-START's; CI must wait until DL-LIBC-START-10 lands. Role shiki:~/w/sh1. Head: handoff/reports/CONF-DYN-HELPERS-2.json.
Status: COMPLETE (gate, plants, lint green on c4418a65d; the commit after it adds only handoff/).

**Decisions in force.** Pending record `records/pending/CONF-DYN-HELPERS-2-1.toml` (series D-27, id to be assigned):
two builds judged by the same EXPECT-* directives, by-path libc until the run-path search, PENDING-LIELLA semantics,
helper selection in `affected`.

**Done.**
- CONF-DYN-HELPERS (two WIP commits): `conformance/run.sh` dynamic-link mode (directives `LINK-HELPERS`,
  `DLOPEN-HELPERS`, `HELPER-FLAGS`, `LINK-EXPORT-DYNAMIC`, `PENDING-LIELLA`), programs `s27_dlsym_handles`,
  `s27_dlopen_failures` (pending), `s27_dynlink_helper_dlsym` (now pending DL-16b), helpers, `affected.rs` selection
  of `conformance/c/helpers/`, conformance/README.md.
- CONF-DYN-HELPERS-2: clause tests `s27_1_f_0` (s27_dlopen_failures, PENDING until DL-16a) and `s27_1_c_3`
  (s27_dlsym_handles, PENDING until DL-16b) in crates/core/tests/s27_dynlink_clauses.rs (now a specified file; the
  other 17 clauses stay stubs); each asserts its program's PENDING line by name. Plants in
  handoff/CONF-DYN-HELPERS-plants.txt (helper unbuilt in either set, the existence check (equivalent, reason in the
  file), both arms of the pending verdict). Decision record above.

- CONF-DYN-HELPERS-2 also fixed `cargo xtask affected`'s failure report (D-X.27) for the inherited run.sh refactor:
  `verdict()` labels misses `<name> (reference|Liella)` and each run has its own stderr file; `conformance_failures`
  now prefixes the run and tails the files the FAIL line names; format test updated, new test
  `a_failed_dynamic_link_conformance_program_reports_the_failing_run` (the first gate failed on the format test).

**Jobs (shiki), final code head c4418a65d.** `conf-dyn-helpers-2-lint3` (clippy, static, records check: rc 0),
`conf-dyn-helpers-2-gate2` (`affected --run --base origin/next`, which also selects DL-LIBC-START-10's files: 32
suites ok, 11697 passed, 0 failed; conformance 132 passed, 0 failed, 3 pending on Liella; peak 14G),
`conf-dyn-helpers-2-plants2` (7 plants: 6 CAUGHT, 1 EQUIVALENT, no WARN). Superseded: `conf-dyn-helpers-2-gate`
(rc 1: the xtask format test above, plus one fstree_equals_the_reference miss that passed on rerun, see the report),
`-plants` (older head), `-lint0..2`, `-xt1/2` (targeted checks).

**Open questions.** When ld.so's run path gains the library search, drop the by-path libc copy (`dyn_setup` in
run.sh) and link Liella's builds against libc.so.6 by soname (a dlopen'd helper's DT_NEEDED `libc.so.6` must then
match the loaded libc). Static TLS exhaustion (part of s27_1_f_0's text) has no program yet.

**Next step.** When DL-16a / DL-16b land: their run fails on the PENDING-LIELLA FAIL line; remove the directive from
the program and replace `assert_pending` in the clause test by a plain run (`conformance(&[...])`).

**Do not redo.** The base decision and its probe; the reference texts (all match); the clause tests and plants.
