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

## HARNESS-REEXEC-IGNORED-2 (port onto the new root)
Branch task/harness-reexec-ignored-2. Ported the historic branch's commits (the static-count commit f5418eae was dropped: next's own check list now has 20 entries incl. reexec-audit).
Four re-exec tests that landed on next since (search_nomem, strto_internal_group, popen_system_eintr, syslog_stream) were flagged by reexec-audit and fixed the same way.
Jobs on kanon: harness-reexec-ignored-2-plants (rc 0; 4 plants on those four files, each caught by reexec-audit), harness-reexec-ignored-2-gate (escalates to FULL, detached; dispatcher reads it).
clippy --workspace --all-targets --all-features -D warnings and cargo xtask static: clean on the final code head.
Next step: none beyond the gate result.

## HARNESS-REEXEC-IGNORED-3 (merge conflict)
Branch task/harness-reexec-ignored-3. Merged origin/next; the only conflict was xtask/src/main.rs (dispatch arms and the static check list). Kept both sides:
next's `lab-names` and `fs-order` plus this branch's `reexec-audit`; the list is now 21 entries. No other file needed a change.
Jobs on keke: harness-reexec-ignored-3-lint (clippy --workspace --all-targets --all-features -D warnings, then cargo xtask static: rc 0),
harness-reexec-ignored-3-gate (affected --base origin/next --run, detached; the dispatcher reads it). Plants were not rerun: the plant files are unchanged by the merge (4/4 caught on the -2 head).
Next step: none beyond the gate result.

## HARNESS-REEXEC-IGNORED-4 (CI precheck red on reexec-audit)
Branch task/harness-reexec-ignored-4. Merged origin/next (clean). cargo xtask static reported two problems, both for one test next added:
`s16_nameservices_symbols.rs` `sym_herror_failed_write_keeps_errno` (the /dev/full re-exec): no `--include-ignored`, no `running 1 test` assertion. Fixed both, like the other sites.
Jobs on shiki (role sh2): harness-reexec-ignored-4-lint (clippy --workspace --all-targets --all-features -D warnings + static: rc 0),
harness-reexec-ignored-4-plants (P1 flag removed, P2 assertion removed: both CAUGHT by reexec-audit with the expected message),
harness-reexec-ignored-4-gate (affected --base origin/next --run).
Superseded: harness-reexec-ignored-4-static (rc 1; the failure above, before the fix).
Next step: none beyond the gate result.
