# XTASK-AFFECTED-RECORDS: `cargo xtask affected` classifies records/ and layout/contracts.tsv

State: branch `task/xtask-affected-records` (from origin/next), code commit `bf4b6da8f`. Decision in force: D-X.57
(`records/decisions/D-X/D-X.57.toml`).

## What changed (xtask/src/affected.rs)

- `Class::Records`: every path under `records/` (decision, compat and saturation records, `records/views/*.head.md`
  and `*.tail.md`, `records/views.toml`) selects the xtask unit tests, `cargo xtask static`, and any target, crate
  or xtask source that names the file as a path (`path_readers`, factored out of `docs_readers` and shared with it). No
  products, no conformance programs. It was FULL ("not a path the selection knows").
- `Class::LayoutManifest`: `layout/contracts.tsv` selects `cargo xtask layout-check` (so `needs_products`: fresh
  `cargo xtask so`, which also runs `layout::check_products`), `cargo xtask static` and the xtask unit tests. It was FULL.
- Unit tests `records_select_the_xtask_tests_and_static_not_full` and `the_layout_manifest_selects_layout_check_not_full`
  (Direct and Wide tiers; no spec clause).

## Readers of the records (why `static` + `-p xtask` covers them)

`grep -rn records:: xtask/src crates`, plus a search for code that names `records/` as a path:
- `records::check` (static step `records`), `records::fresh_check` (static step `records-gen`; outside the train it
  only counts stale views, but a malformed `views.toml` or record still fails it), `records::cmd` / `gen_cmd`
  (`cargo xtask records`, `records-gen`).
- `register.rs` (`records::saturation_rows`, `GENERATED`): static step `register`.
- `trace.rs` (`records::ids(Decision)`): static step `trace`.
- `compat_ids.rs` (`records::ids(Compat)`): static step `compat-ids`.
- `layout.rs` (`DECISION_DIRS`: decision ids from `records/decisions/`, rule d): static step `layout`.
- `hottext.rs` only treats `records/*.toml` as prose that may name the regen flag (no read of the ids).
- No crate under `crates/` reads `records/` (`verdict_cache.rs` uses the word "records" for something else).
All unit tests of these modules are in `-p xtask` (bin unit tests).

## Results (shiki, role sh1)

- `xar-check`: `cargo xtask clippy` OK, `cargo xtask static` OK; before/after listing below.
- Before/after `cargo xtask affected --list --base origin/next --files <the files of task/hottext-isolate-64k-2>`
  (2 handoff reports, 1 handoff note, `records/decisions/D-X/D-X.51.toml`, `records/saturation/SR-86.toml`):
  BEFORE: tier Direct, **FULL (escalated)**, both records "not a path the selection knows". AFTER: tier Direct, no
  escalation; 1 test step (`cargo test -p xtask`) and `cargo xtask static`.
  `layout/contracts.tsv`: BEFORE FULL; AFTER `-p xtask`, `cargo xtask layout-check` (products), `cargo xtask static`.
  The before run used this branch's tree with this story's affected.rs diff reversed (an origin/next affected.rs).
- `xar-gate`: `cargo xtask affected --run --base origin/next`; `xar-plants`: `handoff/XTASK-AFFECTED-RECORDS-plants.txt`
  (3 plants: records class selecting nothing; layout arm without layout-check; a bad record id run through the
  selected set).
- `xar-gate` (rc 0): suites 2 ok, 312 passed, 0 failed, affected: ok; peak 1.3G, oom_kill=0.
- `xar-plants` (rc 0): 3 plants, 2 CAUGHT (`xar-records-select-nothing`, `xar-layout-no-check`, each by its unit
  test), 0 SURVIVED, 1 **WARN-NO-MSG** (`xar-bad-record-id`): the selected set DID fail it (`FAIL cargo xtask static`,
  `affected: FAILED`, rc 1), but the `msg:` regex named the records-check line, which goes to the console and is not
  in that log. Next step: change its `msg:` to `FAIL +cargo xtask static` and re-run the plants file (one run, ~16 min).
  Also WARN-DEV-SHM (the copy was under /dev/shm; no test of this story mounts /dev).

## Open questions / not to redo

- None needing a ruling. Selecting all `-p xtask` unit tests for a record is wider than the records tests alone,
  but the readers' tests (trace, register, compat-ids, layout) also read the live tree; one `cargo test -p xtask`
  covers all of them.
- Next step: none beyond landing. A possible follow-up: map `handoff/reports/*.json` explicitly (they are docs today,
  correctly).
