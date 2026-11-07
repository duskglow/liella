# GATE-VALIDATION-SH-TESTS: changes to validation/*.sh select their self-tests

State: branch `task/gate-validation-sh-tests` (from origin/next), HEAD: see the report json. Role keke:~/w/n2.
Decision in force: the D-NEW row (placeholder, top of `plan/07-decisions/x-implementation-decisions.md`; the brief
reserved no id: the coordinator assigns it).

## What changed

`xtask/src/affected.rs`:
- `VALIDATION_SELF_TESTS`: `validation/wl_run_test.sh`, `wl_stats_test.sh`, `wl_mtmem_test.sh` (run as `bash <path>`
  from the root; no build needed).
- `VALIDATION_COVERED` (script -> self-tests), from what each self-test sources or reads:

  | script | self-tests |
  |---|---|
  | wl_syscls.sh (the syscall verdict) | wl_run_test (sources it), wl_stats_test and wl_mtmem_test (via wl_stats.sh) |
  | wl_stats.sh | wl_stats_test, wl_mtmem_test |
  | wl_run.sh | wl_run_test (grep checks: sources the classifier, passes the thread flag) |
  | wl_count_run.sh | wl_stats_test (grep check: passes the thread flag) |
  | each self-test | itself |

- `VALIDATION_UNCOVERED`: the other 24 `validation/wl_*.sh` scripts, each with why it has no gate self-test (lab
  drivers, host sides needing built kits; `wl_percall_tunables_test.sh` is a lab count job, minutes, built products;
  `wl_stats_plants.sh` is the plants runner of the covered tests). The reason is the `--list` note for that script.
- New `Class::ValidationScript`: a covered script fills `Selection::scripts`; `run_selection` runs them after the
  other steps (direct and wide tiers; not FULL). The FULL branch of `affected` runs all three after `cargo xtask test`.
- Unit tests: `every_validation_wl_script_is_mapped` (every `validation/wl_*.sh` in exactly one table, every named path
  exists, every named self-test is in the run list and covers itself) and
  `validation_script_changes_select_their_self_tests` (wl_syscls.sh -> the three; wl_run.sh -> wl_run_test; wl_gate.sh,
  validation/README.md and a crate source -> none).

## Proof

- `cargo xtask affected --files validation/wl_syscls.sh --list` on keke: 3 of 3 self-tests, 0 test targets, 0
  conformance, no FULL. `--files validation/wl_gate.sh`: 0 self-tests (note names the reason). `--files
  crates/core/src/lib.rs`: 0 self-tests (the Rust selection unchanged by this story).
- Cost (8-thread machine): wl_run_test 0.05 s, wl_stats_test 0.2 s, wl_mtmem_test ~15 s (6 real strace runs of a
  threaded sort; prints SKIP and exits 0 without strace or `sort --parallel`).
- Plants: `handoff/GATE-VALIDATION-SH-TESTS-plants.txt` (4): syscls-futex (wl_syscls.sh stops recognising futex; caught
  by `affected --files validation/wl_syscls.sh --run`, which also proves the run step executes the scripts), map-drop,
  classify-order, missing-path (xtask unit tests). keke job `gate-validation-sh-tests-plants` on f49de9145:
  4 CAUGHT, 0 SURVIVED, 0 WARN (2383 s, mostly the tool's product build and three xtask rebuilds).
- Gate: keke job `gate-validation-sh-tests-gate` (`affected --base origin/next --run`) on f49de9145: ok, 168 s (xtask
  unit tests 140 s, static 18.5 s). Clippy (`cargo xtask clippy`) and `cargo xtask static` clean in the role.
- Later commits on the branch are handoff/ only.

## Open / next

- INFORMATIONAL: `cargo xtask test` (the landing gate, main.rs) itself still does not run the self-tests; only `affected`
  (all tiers, FULL included) does. Adding them to `xtask test` is a one-line main.rs step (+~15 s); left out because a
  main.rs change escalates the story's own gate to FULL. Recommend a follow-up if the landing gate is `xtask test`.
- The FULL-branch loop in `run_selection` has no plant (exercising it needs a FULL run).
- 24 wl_*.sh scripts have no gate self-test; most are lab drivers that need hosts or built products.
