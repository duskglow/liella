# HARNESS-REEXEC-IGNORED

Every test that re-executes its own binary now runs exactly the case it names, #[ignore]d or not, and fails if the child ran nothing (#2067).

## What changed
- All 60-odd `current_exe()` + `"--exact"` re-exec sites under `crates/*/tests` (core, boundary, rt; 41 files, plus `support/sync_trap.rs`) now pass
  `--include-ignored` and assert `running 1 test` in the child's stdout (libtest prints it before the case runs, so it also holds for children that
  `exit_group`). Helpers that return the child's `Output` (`run_probe`, `child`, ...) assert it before returning.
- Sites that already had their own check keep it: `s23_time_clauses.rs` `s23_7_c_3` (`--ignored` plus the child's getdate_r line, allow comment) and
  `enomem_stream_grow.rs` (RAN marker, plus the standard assertion now).
- Four spawn-and-pipe getpass harnesses (`s17_users_symbols.rs` x3, `s17_users_clauses.rs`) carry `// reexec-guard: allow(...)`: they read the child's marker lines.
- `.status()` harnesses (s04 `s4_10_f_0`, s09 `s9_2_c_0`, s14 `sym_qsort`) now use `.output()` so the stdout can be checked (s14 no longer nulls stdout/stderr).
- New `cargo xtask reexec-audit` (in `static` and `test`): flags a `current_exe()` re-exec naming a test (`"--exact"`) that lacks `--include-ignored`/`--ignored`
  or whose function never asserts `running 1 test`; exception = `// reexec-guard: allow(<why>)` on the call or up to 3 lines above. Unit tests in `policy.rs`.

## Tests that were not running before
None found among the re-exec targets: the only #[ignore]d case behind a harness was memstream (fixed in the stacked story) and `s23_7_c_3_child`
(already run with `--ignored`; the #2067 concern is satisfied). No other test named by a re-exec is `#[ignore]`d, so the flag change itself unmasks nothing.
Whether any child was silently filtered out (name typo) is answered by the gate: a failure there would name it. See the gate job.

## Jobs
- harness-reexec-ignored-plants-2 (rc 0): P1 enomem harness without --include-ignored: CAUGHT by its test and by the static check; P2 empty child run
  (filter matches nothing) in s17_users_clauses: CAUGHT; P3 assertion removed from s12_stdio_clauses: CAUGHT by the static check.
- harness-reexec-ignored-gate-2: FULL tier (the xtask change escalates it), detached; the dispatcher reads it.
- Superseded: harness-reexec-ignored-gate (killed by me, rc 143, older head), harness-reexec-ignored-plants (P1 static plant survived: the enomem harness had an allow comment).
