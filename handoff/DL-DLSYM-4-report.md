# DL-DLSYM-4 report

Branch `task/dl-dlsym-4`; build role shiki:~/w/sh2. Brief: continue DL-DLSYM-3 once LAYOUT-TRACE-DEFAULT has landed
(coordinator note 11-10: merge origin/next, rerun `init_data_clusters_hold_at_every_text_phase` in fast and release
and the gate on that head, nothing else).

## Commits
- `4a827a08c` merge of origin/next d53c0d551 (LAYOUT-TRACE-DEFAULT-4 and DL-16d's static-TLS placement among it).
  One conflict, `crates/boundary/src/ld/dlopen.rs`: next added the TLS surplus (`tls_mark`, `rollback`,
  `place_tls`, `thread_walk`) where this branch added the unique table (`unique_mark`, `truncate`, `pin_unique`).
  Both are kept: `unwind` truncates the unique table and rolls the surplus back, `load_set` sets both marks.
- `1a8066d41` next's `thread_walk` called the `Loader::syms_upto` method that this branch replaced with the free
  function `syms_of` (dlsym borrows the symbol tables beside the unique table); it now calls `syms_of`, which returns
  the same slice.
- `c57cdd977` drops `PENDING-LIELLA: DL-16b` from `conformance/c/s27_dynlink_helper_dlsym.c` (it came with next,
  pending on this story; with dlsym in ld.so Liella's run passes, which run.sh reports as a FAIL until the directive
  goes, conformance/README.md). No clause test carries a pending assertion for it.
- The handoff, this report and the json are committed on top. No dlsym code changed in this run.

## Jobs (shiki, role sh2)
- `dl-dlsym-4-lint`: clippy on the bare merge, rc 1 (E0599 `syms_upto`, fixed by `1a8066d41`). Superseded.
- `dl-dlsym-4-lint2`: `cargo xtask clippy && cargo xtask static` on `1a8066d41`, rc 0.
- `dl-dlsym-4-layout-release`: `LIELLA_FULL_PROFILE=1 cargo xtask so`, then the layout test alone: rc 0. `tool` 49
  at every `cold`/`lbss`/`data` pad (run 3: 55), `true` 10, `fork` 5.
- `dl-dlsym-4-layout-fast`: same in the fast profile: rc 0. `tool` 50 at every pad (run 3: 54 at `cold 0x2000`, 53
  elsewhere), `true` 11, `fork` 5. The run-3 red came from next's stale tracked hot list, and trace-at-build removed
  it, as run 3 expected.
- `dl-dlsym-4-gate`: `cargo xtask affected --run --base origin/next` (escalates to FULL for
  `csys/src/machine/dl.S`, `compat_aliases.S`, `xtask/src/compat.rs`, as in runs 2 and 3) on `1a8066d41`: rc 1,
  61 min, peak 16G (page cache, oom_kill 0). Suites 30/30 ok, 11734 passed, 0 failed, 1271 ignored; conformance
  133 passed, 1 failed: `s27_dynlink_helper_dlsym` ("PENDING-LIELLA: DL-16b, but Liella's run meets the
  expectations now"). This is the expected result of this story landing. Fixed by `c57cdd977`. Superseded.
- `dl-dlsym-4-conformance`: `cargo xtask so && sh conformance/run.sh` on `c57cdd977` (the failing suite only, per
  the RED-gate rule): rc 0, 134 passed, 0 failed.
- `dl-dlsym-4-lint3`: clippy and static on `c57cdd977`.

## Plants
Not rerun (coordinator note: code, plants and probes done in -2/-3). `handoff/DL-DLSYM-2-plants.txt`: 12/12 CAUGHT in
`dl-dlsym-3-plants`. This run's two code changes are merge glue in `dlopen.rs` (the unique-table and surplus marks
side by side, one call renamed); the existing dlopen/dlsym direct tests run in the gate.

## Rulings, informational
- None new. Ruling #5350 is closed: the layout test passes in both profiles on the merged head.
- INFORMATIONAL: the gate escalates to FULL every run because the branch touches `dl.S`, `compat_aliases.S` and
  `xtask/src/compat.rs` (the dlsym/dlvsym exports); a per-fix rerun on this branch costs a full gate.
