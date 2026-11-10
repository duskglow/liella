# LAYOUT-STARTUP-WINDOWS report (runs 1 and 2)

Run 1 (LAYOUT-STARTUP-WINDOWS) built the contract. Its evidence is in D-X.49 and in the run-1 plants file
`handoff/LAYOUT-STARTUP-WINDOWS-plants.txt`. Run 1 ended red at handback-check because the branch carried decision
D-X.48 as a row. This file covers run 2.

## Run 2 (LAYOUT-STARTUP-WINDOWS-2), all jobs on shiki role sh2 (Skylake-SP)

Commits:
- 8e5a92e87 merges origin/next 03c9a4972. Conflicts: (1) `plan/07-decisions/x-implementation-decisions.md`: took next's
  file. (2) `xtask/src/main.rs`: kept next's static-check list and added `layout`, giving 31 checks. The D-X.49 and
  D-X.48 rows became `records/decisions/D-X/D-X.49.toml` and `D-X.48.toml`. These follow the `records new` skeleton
  for the x view (id, section, status, story, date, tests, body), with the evidence column joined to the body by `\|`.
  `records check` and `records fmt --check` pass. This branch changes no view (`git diff origin/next -- plan/` is empty).
- dbb3b1a9d adds the merged-head counts to the D-X.49 evidence, plus the new plants file
  `handoff/LAYOUT-STARTUP-WINDOWS-2-plants.txt`.

Jobs:
- `lsw2-lint-inherited`: clippy and static on the inherited head 761d932dc. rc 0.
- `lsw2-measure-8e5a92e87`: records check and fmt, clippy, static, `layout::` tests (10 pass). Release products: 4
  windows (budget 4, 101 of 101 keys). Fast products: 5 windows (budget 5, 107 of 107). All 11 contracts hold. rc 0.
  This settles #4144 (release budget on the merged head).
- `lsw2-plants-dbb3b1a9d`: 7 plants, 7 CAUGHT, 0 SURVIVED, 0 WARN. This settles #4143. lsw-key-removed, moved to tier
  `extra` with profile `-`, now reaches the contract on a real rebuild: `STARTUP-TEXT-WINDOWS: 6 start-up text windows
  (budget-fast 5) ... window 0x660000 (outside the hot group): runtime::start::<false>`. hottext's guard does not stop it
  first.
- `lsw2-gate-dbb3b1a9d`: `cargo xtask affected --run --base origin/next`. It escalated to FULL because xtask sources
  changed, and was left running at hand-back. Re-attach with `ssh shiki '~/bin/jobwait lsw2-gate-dbb3b1a9d'`.
- `lsw2-static-dbb3b1a9d`: `cargo xtask static` and `records check` on the final code head. rc 0.
- `ops handback-check` on dbb3b1a9d: rc 0. Its static/clippy step hit the timeout cap, which only gives a NOTE.

## Decision ids
- D-X.49 is this chain's reservation.
- D-X.48 is LAYOUT-MANIFEST's reservation; that story is stacked underneath and not landed. Its record is on this branch
  with story `LAYOUT-MANIFEST`. Reason: the manifest rows STARTUP-NAMES-WINDOW, GETTEXT-SLOTS-PAGE and PRODUCT-PATH-REMAP
  name D-X.48, and the `layout` static check fails if that decision does not exist. Once the view came from next, the
  record was the only place left for it. handback-check accepts it.
- INFORMATIONAL: if LAYOUT-MANIFEST's own continuation also writes `D-X.48.toml`, the two files must match, or one
  branch must take the other's.

## Rule notes
- In run 2, one command carried an empty heredoc by mistake (`python3 - <<'X'` with an empty body, before the
  `python3 -c` that did the work). It had no effect.
- No `sed -i` was used (#4145).
- Nothing was refused.
