# XTASK-AFFECTED-XTASK report

Commits (task/xtask-affected-xtask): a7331e5ed (the rule, tests, plants file), 4fa7dac05 (the use table compiled for
its test only: clippy flagged it unused), then this handoff and report. Code head for the jobs: 4fa7dac05; later
commits touch handoff/ only.

## Jobs (all on shiki, role sh1, synced from the worktree)
- xtask-affected-xtask-clippy2: `cargo xtask clippy && cargo xtask static`, rc 0 (84 s). Superseded:
  xtask-affected-xtask-clippy (rc 1, XTASK_TOOL_USES unused outside tests; fixed in 4fa7dac05).
- xtask-affected-xtask-gate: `cargo xtask affected --base origin/next --run` (direct tier), rc 0, "affected: ok":
  xtask unit tests ok 115.0 s, `cargo xtask static` ok 15.0 s; job 136 s, peak RSS 1.37 GB.
- xtask-affected-xtask-plants: `cargo xtask plants handoff/XTASK-AFFECTED-XTASK-plants.txt`, rc 0: 6 plants, 6 CAUGHT,
  0 SURVIVED, 0 EQUIVALENT, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (results in the plants file).
- xtask-affected-xtask-after: the gate of a tooling-only change, `cargo xtask affected --run --files
  xtask/src/records/mod.rs`, rc 0: clippy (xtask) 0.5 s, xtask unit tests 103.5 s, static 14.8 s; job 124 s.
- xtask-affected-xtask-ut1 (development run of `affected::` unit tests, rc 0, 83 passed): evidence, not a final-head job.

## Gate time of one tooling-only change, before and after (same host class, shiki)
- Before: every xtask/src module outside affected.rs and the audit table was FULL (`cargo xtask test`). Reference:
  HOTTEXT-DEAD-KEY-REPAIR's FULL gate on this host took 2538 s (hottext-dead-key-repair-gate). A records.rs-only change
  got the same FULL selection; its own FULL time was not measured (not worth a 42-minute run).
- After: a records.rs-only change selects the xtask unit tests, xtask clippy and `cargo xtask static`: 124 s.

## Not narrowed, on purpose
hottext.rs (so.rs calls `hottext::group`), labnames.rs (hottext names it), trace.rs (spec's stubs call it), steptime.rs
(only the full gate's steps), the generators, so/coldbss/layout/csys, nextest/native_examples/rust_dylib, main.rs, any
module not in XTASK_TOOLS (default FULL). So #4684's exact case (hottext's repair command) is still FULL.

## RULING NEEDED / INFORMATIONAL
- INFORMATIONAL: next story recommended: split hottext's repair command into its own module and list it in XTASK_TOOLS.
- INFORMATIONAL: no decision record written (the brief reserved no D-id); say if one is wanted (`records new decision`).
- Process: one doc-comment edit of affected.rs was made with a python heredoc instead of the Edit tool (content reviewed,
  gated). No refusals.
