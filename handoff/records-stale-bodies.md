# RECORDS-STALE-BODIES handoff

State: branch task/records-stale-bodies, record commit 70a61c45a (on origin/next 28a903a01). Records only, no code.

## Done
- D-12.26 (records/decisions/D-12/D-12.26.toml): the body said "No compatibility option". It now names COMPAT-21
  (ruling #4661, COMPAT-SCANF-ERANGE) as registered, not implemented, with the per-scan flag test as the recorded
  cost. Checked against next first: records/compat/COMPAT-21.toml (status "registered", cost text) and the
  differences row "scanf floating range error, then end of input" both name COMPAT-21.

## Waiting (item 2)
- D-10.5 is unchanged. On origin/next 28a903a01, crates/boundary/tests/s10_math_symbols.rs (around line 1413) still
  reports the dynamic fpu_control_default/fpu_control_own run on Liella as a #814 skip. DL-LIBC-START is red and held,
  continued as DL-LIBC-START-2, which was still running when this story started. The record would be wrong if it
  were corrected now.
- Next step: once DL-LIBC-START-2 lands and the skip is gone from next, change the last two sentences of D-10.5's
  body so they say the dynamic hook runs on Liella and name the test and the plant that cover it, then run
  `cargo xtask records check`. That is a one-line follow-up story.

## Jobs
- shiki records-stale-bodies-static: `cargo xtask records check && cargo xtask static`, rc 0
  ("records: ok: 507 decision, 21 compat, 87 saturation records (0 pending)").

## Not to redo
- Do not edit plan/07-decisions/*.md: the train regenerates the view from the record.
