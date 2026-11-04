# TEST-SILENT-SKIPS

A test may skip only when its tool is truly absent, and says so. Trace: no spec clause (test-harness behaviour).

- `oracle::locale`, `oracle::build` (core tests) and the localedef builders in boundary `reload.rs` / `s23_time_symbols.rs`: a tool that cannot be started returns false (the callers skip); a tool that starts and fails now panics. The src-side localedef helpers already asserted.
- Skip lines are now `SKIPPED <test binary>: <reason>` (skip_note; nextest report filter updated).
- `cargo xtask affected` prints `SKIPPED lines: N` in its summary (`count_skips`, unit-tested).
- New test `crates/core/tests/skip_precondition_fails.rs` (fake failing localedef; non-compiling C probe). Plants: localedef failure and cc failure turned back into a skip: both CAUGHT (job test-silent-skips-plants, rc 0).
- Not changed (already correct, absence-only): user-namespace and uidmap skips in appendix_a_primitives.rs.
- Audit not exhaustive: other ad-hoc early returns not routed through oracle/skip_note were not found by grep for `skip`/localedef/cc; remaining ones, if any, are unknown.

## TEST-SILENT-SKIPS-2 (port onto next, 11-04)
- The three TEST-SILENT-SKIPS commits cherry-picked cleanly onto next (no conflicts; nothing of next's replaced). The earlier five commits on the old branch are HARNESS-REEXEC-IGNORED, a separate story, not ported here.
- Gate: clippy -D warnings, `cargo xtask static`, `xtask so`, tests skip_precondition_fails, skip_note_visible, boundary reload and s23_time_symbols, and the xtask unit tests: all green (jobs test-silent-skips-2-gate2, test-silent-skips-2-plants2).
- Plants: localedef failure and cc failure turned back into a skip: both CAUGHT.
- Audit remains non-exhaustive (see above). The role tree has no .git, so `affected --base` escalates to FULL there; tests were named explicitly.
