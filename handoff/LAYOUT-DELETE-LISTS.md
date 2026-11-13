# LAYOUT-DELETE-LISTS (LT-5): delete the committed hot-text lists and their machinery

State: run 2 (LAYOUT-DELETE-LISTS-2), branch `task/layout-delete-lists-2` (stacked on run 1's
`task/layout-delete-lists`), base origin/next d53c0d551. HEAD: see handoff/reports/LAYOUT-DELETE-LISTS-2.json.
Plan: plan/layout-rethink.md section 4 (LT-5), rulings #5190-#5194, #5463; D-X.88, D-X.92; pending record
records/pending/LAYOUT-DELETE-LISTS-2-1.toml (D-NEW, series D-X; amends D-X.92/.56/.53/.82/.86/.87).

## Done (brief steps 1, 3, 5; ruling #5463)

* Run 1 (inherited, did not compile): `git rm` of the six lists + merge.py/prune.py, order.py, regen.sh
  trace-only rewrite, startup_data.py, hottext.rs machinery removal. Step 0 of this run: clippy and static on
  that head failed (rc 101, did not compile: known WIP state; jobs layout-delete-lists-2-clippy0/-static0).
* hottext.rs: tests ported (merge.py -> order.py: `order_moves_startup_text_keys_to_the_head`,
  `a_startup_label_brings_its_section_to_the_head`), list-reading/drop/prune tests deleted, dead V0
  `name`/`owner` removed; new `retired_lists_refuse_a_committed_list` and
  `static_refuses_a_readded_committed_list` (git scratch repo, end to end through `static_check`).
* hotlists.rs rewritten trace-only: `Layout {Traced, Ungrouped}` (`0` -> error naming LT-5), `force_dir`
  re-settable per build (a Mutex, so bench's v3 build after the products sets its own lists dir), `dir()` =
  forced dir or error, `rel()` maps bare list names to it and refuses `crates/libc-so/<list>` by name.
  `hot-lists` command, STAMP/OUT/tree_hash/resolve/train gone.
* so.rs: Layout::Committed, run_regen_libc gone; pre-group archive always saved; marker always written;
  regen.sh called as `--products <dir>`. main.rs: `hottext-repair`, `hottext-recheck`, `hot-lists` gone; `so`
  takes no argument.
* Ruling #5463: bench.rs `--v3` build is `Layout::Traced`; `Ungrouped` only when the host lacks x86-64-v3
  (`is_x86_feature_detected!` of the v3 set); then `liella_v3 layout: UNGROUPED: ...` in the bench output
  (also `target/liella-v3/layout.txt`). Test `v3_measurement_build_is_traced_unless_the_host_cannot_run_it`.
* contracts.tsv `keys=startup-text.keys` etc. (bare); startup_ro.rs bare names; layout.rs messages say
  "rebuild: cargo xtask so traces the layout"; hot_text_products.rs `generated_list` reads only the marker dir.
* probe_text.sh / startup_data.sh: LIELLA_HOT_LISTS required, order.py, no --replace.
* Docs/comments: CONTRIBUTING.md (lists paragraph rewritten), iconv/mod.rs, cpuid.S, fold.S,
  hot-text.clusters, keys.py, coldbss.rs. Left as history: bench/perf-ledger.tsv, SR-83, plan/.
* Records D-X.53/.82/.86/.87: references to the deleted tests removed (static's records check).

## Results (shiki, Skylake-SP class role sh1)

* clippy (cargo xtask clippy): green (layout-delete-lists-2-clippy2). static: green (layout-delete-lists-2-static3).
* xtask unit tests hottext/hotlists/bench v3/layout/startup_ro: 41 passed (layout-delete-lists-2-xtests).
* Gate: `affected --run --base origin/next` escalates to FULL (xtask + crates/libc-so + cpuid.S):
  layout-delete-lists-2-gate (see the report json for its verdict).
* Plants (handoff/LAYOUT-DELETE-LISTS-2-plants.txt, 7 plants, xtask unit tests): plants-1 REFUSED (a `lit:`
  plant held " | "; fixed as perl:), plants2 BASELINE FAILED: the copy's fresh `cargo xtask so` ran past the
  600 s limit while the FULL gate built in parallel on the same host (no plant applied). Rerun after the gate.

## Not done (exact next steps)

1. Brief step 2 (pins): the 45-49 `#[inline(never)]` pins kept only for list keys are unchanged (no check needs
   them now). Candidate list in run 1's notes: the 15 with an explicit key-only comment (cpu::init_with, heap
   map, process::exit, runtime::{run_fini,start}, stdio::new_slot, sys::vdso_fn, thread::each_block,
   catalog::variants::each, kernel vdso lookup_many, exit_handlers::{register,run_all}, intl::record_for,
   nscache::real_secs, stdio_compat::each_stream). Unpin in one commit, one wl_percall of the start-up rows
   against next (fast and release), keep each unpin whose rows are equal or faster and whose fault/window bounds
   hold; report the table. A separate story is fine: this branch changes no pin.
2. Brief step 4 remainder: release profile (`LIELLA_FULL_PROFILE=1 cargo xtask so` + layout-check +
   hot_text_products) and the keke run; this run proved the fast profile on shiki only (the FULL gate).

## For the coordinator (brief step 5)

Retire in the same landing: the train's `hot-lists` step (sets `ci_train_hot_lists` and runs
`cargo xtask hot-lists --in-place`; the command no longer exists after this branch) and the hot-lists lane
(one-at-a-time lane for list-touching stories). Nothing in the Liella tree calls either any more.

## Not to redo

`git grep -E 'libc-so/(hot-text\.list|startup-text|probe-text|startup-data)|merge\.py|prune\.py|hot-lists|LAYOUT_TRACE=0'`
outside handoff/, plan/, records/decisions, docs/saturation, bench/perf-ledger.tsv and SR-83 is clean apart from
the refusal messages and their tests.
