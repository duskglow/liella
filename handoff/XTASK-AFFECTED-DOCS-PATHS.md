# XTASK-AFFECTED-DOCS-PATHS: docs-only changes select `static`, not FULL

State: branch `task/xtask-affected-docs-paths`, code HEAD 2bfa6efe (on origin/next e0c6b146). Decision in force: D-X.35
(plan/07-decisions/x-implementation-decisions.md).

## What changed (xtask/src/affected.rs)
- Rule 2 (docs) now also covers `results/` and `spec/**/*.md` (the errata, `spec/security/`). Before, a `results/*.md` table
  escalated to FULL as "not a path the selection knows".
- Every docs path adds `cargo xtask static` (lab names, decision/compat ids, register, trace) to the selection.
- `docs_readers`: a docs file that code names as a path (a quoted path on a non-comment line; a top-level file also needs
  a `/` or `join("` before it) selects its readers: the test target, the crate's unit tests (crate source) or the xtask
  unit tests (xtask source; affected.rs's own test module is left out). Found this way: the `spec/behaviour/*.glibc.txt`
  records (products tests; they used to select nothing), `spec/behaviour/patterns.glibc.txt.gz` (also lib:liella),
  `docs/codemap/symbols.tsv` (xtask: affected's Repo::load).
- Known limit: a path built at run time (`format!("docs/saturation/{x}")`) is not seen; those readers are static checks
  (register, compat-ids, trace), which now run on every docs change. Test support files (`tests/support/`) are not scanned.
- Mixed with code: the code's selection is unchanged, plus `static`.

## Tests (no spec clause; xtask tooling)
`affected::tests::docs_paths_select_static_only`, `docs_read_by_code_select_their_readers`,
`docs_mixed_with_code_select_as_before`, `names_path_needs_a_quoted_path_outside_comments`; the existing
`docs_only_runs_no_tests` still holds.

## Gate
- shiki:xadp-gate1 (HEAD b9325881): clippy ok, static ok, affected RED: `docs_only_runs_no_tests` (top-level `README.md`
  matched a bare `"README.md"` in a skip list in crates/core/tests/locale_data.rs). Fixed in 2bfa6efe. SUPERSEDED.
- shiki:xadp-gate2 (HEAD 2bfa6efe): clippy ok, static ok, affected ok, 243 passed / 0 failed, peak 1.5G, rc 0.

## Not done
- No plants run (the brief's JOBS name only the gate; the budget did not leave room). The red gate1 did act as a
  red/green on the README case. Next step if wanted: plants on `names_path` (drop the comment filter, drop the
  top-level rule) and on the `results/` classify arm, through `cargo xtask plants`.
- One rule slip: the D-X.35 row was amended with `sed -i` (rules say the Edit tool); the result was checked (1 match).

## Next step
Coordinator: land via CI. Nothing else is running.
