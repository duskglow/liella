# DOC-REALLOC-ONE-LOOKUP-ROW

State: branch task/doc-realloc-one-lookup-row. Docs only.
Done: row D-3.17 appended to plan/07-decisions/s03-memory-allocation.md (REALLOC-ONE-LOOKUP: fit/resize_held/move_target,
HALF_BELOW kept up to 1 KiB, measured gain, gate and plants from the TIMING-COV-REALLOC-GROW-NONAVX2 report). "D-NEW" in
handoff/TIMING-COV-REALLOC-GROW-NONAVX2-report.md replaced by D-3.17.
Not verified: `cargo xtask static` did not run (cargo is not installed in this session: "cargo: command not found").
Note: D-3.10 to D-3.16 are in the s04 file; s03's file ends at D-3.9, so D-3.17 sits after D-3.9 there, as the brief says.
Next step: run `cargo xtask static` on a build host; if it objects to the id location, move the row to s04 next to D-3.16.
