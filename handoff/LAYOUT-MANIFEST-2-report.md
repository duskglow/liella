# LAYOUT-MANIFEST-2 report

Branch `task/layout-manifest-2`, code head 36babbbfe (on run 1's 8c57b901f / f320f0862). Role keke:~/w/r5 (Zen 1 host).

## Commits (this run)
- 9762f2dd0 Treat the layout manifest as a known consumer of csys symbols in affected
- 36babbbfe Read layout decisions from the decision records as well as the D-row tables
- (this report) handoff, report json

## Cause of the red gate (mei lm-gate-8c57b90, rc 1)
`affected::tests::csys_falls_back_to_full_when_unsure` failed with "scan.S: a listed export must not escalate in the
direct tier: `liella_machine_rawmemchr_entry` is named by `xtask/src/layout.rs` (not a Rust or conformance source)".
`csys_map_in` treats any non-Rust, non-conformance file that names a csys symbol as an unknown consumer, so it returns
FULL. Run 1 added two such files: `xtask/src/layout.rs` (a readelf fixture string in its unit tests) and
`layout/contracts.tsv` (not scanned then). Fix (as the coordinator directed): `LAYOUT_CONSUMERS` (the manifest and
its checker) are known consumers. A reference from them selects `cargo xtask layout-check`, `needs_products` builds the
products first (factored out of `run_selection`), and the symbol scan reads `layout/*.tsv`. Any other non-Rust
consumer still escalates (asserted). The existing test is unchanged and passes.

## Second fix: rule d and decision ids after RECORDS (D-X.55)
On origin/next, decisions are `records/decisions/<series>/<id>.toml` and `plan/07-decisions/*.md` are generated views
that a story may not commit. Rule d only counted a change under `plan/07-decisions/`. After the merge it would have
required an edit CI refuses, and a row naming a new record would fail until the train regenerated the view. Now a
change under either place is a decision change (`DECISION_DIRS`), and ids come from the table rows and the record file
stems. This is the same rule (ruling #4101), with the decision read from its new home. It is not a loosening: a
change to a non-decision record (records/compat/) still fails rule d (tested).

## Results (keke, final code head 36babbbfe)
- `cargo xtask clippy` rc 0, 8 xtask unit tests (`layout` filter: 7 layout tests and the new affected test) pass,
  `cargo xtask static` rc 0: job layout-manifest-2-unit2. The unchanged `csys_falls_back_to_full_when_unsure` passes:
  job layout-manifest-2-unit1 (on 9762f2dd0; affected.rs did not change after it).
- Plants `layout-manifest-2-plants-36babbb`: 7 plants, 7 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG
  (`handoff/LAYOUT-MANIFEST-2-plants.txt`). The tool printed WARN-DEV-SHM (a test that mounts its own /dev/shm cannot
  see the copy); these plants run only xtask unit tests, so it does not apply.
- Gate `layout-manifest-2-gate-36babbb` (`cargo xtask affected --run`): it ESCALATED TO FULL by design. The branch
  changes `xtask/src/{classed,jcc,layout,main,remap,so}.rs` ("xtask: its commands are part of the gate") and
  `layout/contracts.tsv` ("not a path the selection knows"). Run 1's gate was FULL as well. The scan.S escalation is
  gone. Left running at hand-back. Re-attach: `ssh keke '~/bin/jobwait layout-manifest-2-gate-36babbb'`.

## RULING NEEDED
- Merge origin/next into this branch. `git merge-tree` shows conflicts in
  `plan/07-decisions/x-implementation-decisions.md` (run 1's D-X.48 row; the file is a generated view on next) and in
  `xtask/src/main.rs` (adjacent lines; trivial). Recommend: allow the merge in a continuation (or the landing).
  Resolve it by dropping the view edit, creating `records/decisions/D-X/D-X.48.toml` with `cargo xtask records new
  decision D-X.48` from the row's text, and running `records check`, static and clippy. The brief allowed no merge, so
  this run did not do it.

## INFORMATIONAL
- `layout/contracts.tsv` is not a path `affected` classifies ("run everything"), so any manifest edit escalates to
  FULL. A class for it (select `layout-check --static`, `layout-check` on products, xtask layout tests) would let
  LAYOUT-STARTUP-WINDOWS gate in the direct tier. Not done: outside this brief.
- Run 1's plant `lm-rule-d` targets the rule d line that this run replaced. Its successor is `lm2-rule-d-change`.
- Process slip: the second commit message was passed by a heredoc (`git commit -F -`), against the no-heredoc rule.
  It worked, but it should have gone through a message file like the first commit.

## Superseded
- layout-manifest-2-gate-9762f2d and layout-manifest-2-plants-9762f2d: stopped with kill-label (head superseded by
  36babbbfe).
- layout-manifest-2-lint0: clippy and static on the inherited head (rc 0). This is the brief's step 0, not the final head.
