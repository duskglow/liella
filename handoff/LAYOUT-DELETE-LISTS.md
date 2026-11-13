# LAYOUT-DELETE-LISTS (LT-5): delete the committed hot-text lists and their machinery

State: branch `task/layout-delete-lists`, run 1 stopped at the context checkpoint (WIP commit; HEAD in
handoff/reports/LAYOUT-DELETE-LISTS.json). Base origin/next d53c0d551. **The tree does not compile yet**
(xtask mid-edit); no job was run. Plan: plan/layout-rethink.md section 4 (LT-5 paragraph), rulings
#5190-#5194, D-X.88, D-X.92, handoff/LAYOUT-TRACE-DEFAULT.md.

## Done (WIP commit)

* `git rm`: crates/libc-so/{hot-text.list, startup-text.keys, probe-text.keys, startup-data.keys, hot-text.absent,
  hot-text.unpinned}, tools/hottext/merge.py, tools/hottext/prune.py. Kept: startup-state.list (D-X.64),
  hot-text.clusters, hot-text.first-window.
* tools/hottext/order.py (new, replaces merge.py): orders ONE trace (`<new keys> <profile>`, `--startup-head`,
  `--alias`); no old-list input, no drop logic.
* tools/hottext/regen.sh rewritten (scratch generator mkregen.py, kept the trace body verbatim): only
  `LIELLA_HOT_LISTS=<dir> regen.sh <tree> --products <dir>`; LIELLA_HOT_LISTS required (no crates/libc-so
  default); every page_faults mode always traced (old --all-modes unconditional); writes fresh lists through
  order.py; no --both/--build/--allow-drop/--replace/--trace-only/--merge-only, no extra programs, no prune.
  Left: two comments still say "(--replace)" / "crates/libc-so/startup-text.keys" (lines ~125, ~131), and the
  messages still say "regen: --all-modes:" (no test greps them).
* tools/hottext/startup_data.py: no old-file union, no --replace; writes the build's own objects only.
* xtask/src/hottext.rs: removed REGEN_FLAG/regen_mode/regen_on/LIBC_ONLY_FLAG/recheck_cmd, ABSENT_LIST and the
  dead-key machinery (unmatched*, DeadKeys, dead_key_verdict, PROMOTE_ENV), repair (Repair, repair_lists,
  check_lists, repair_cmd), the pin check (fn_site, inline_never, module_source, UNPINNED_LIST, parse_unpinned,
  unpinned_reason, check_pinned, INLINE_NEVER_TIERS), may_name_regen_flag and their tests. `group()` lost its
  `profile` parameter and the regen fall-backs; the hot-text.matched file (prune.py input) is no longer written.
  New: `RETIRED` (the six file names), `retired_lists(tracked)` and `static_check` = no tracked file is a
  RETIRED list (the anti-creep check of brief step 4).

## Remaining (exact next steps)

1. hottext.rs: fix the `use super::{...}` line of `mod tests` (drop removed names, `Ordering` import, top
   `AtomicBool` import); delete the `matched` set in group() (now unused); delete tests
   `regen_replace_drops_the_allow_list_lines_of_keys_it_drops`, `merge_names_every_key_it_drops`; port
   `merge_moves_startup_text_keys_to_the_head` and `a_startup_label_brings_its_section_to_the_head` to order.py
   (args: `<new> fast [--startup-head f] [--alias f]`, no "old" file; expected outputs unchanged); in
   `regen_always_traces_the_bounded_tool_and_locale_modes` drop the --allow-drop guard part (the `for cmd in
   "$@"` anchor is gone: assert the locale/tool lines precede the fork/strings modes instead); check
   `a_cold_renamed_function_reenters_the_list_by_trace` and `regen_runner_...` still match the scripts. Add a
   test `retired_lists_refuse_a_committed_list` (one RETIRED path in a fake tracked set -> error naming it; the
   real tree's `git ls-files` -> none). Plant: drop "hot-text.list" from RETIRED, msg: the new test name.
2. hotlists.rs: rewrite to the trace-only module: keep GENERATED, TRACE_ENV, ENV (only the tracer's output dir),
   MARKER, Layout {Traced, Ungrouped} (`0` -> error "removed in LT-5"; no regen arg), force_dir, marked_dir,
   follow_products, marker_text; `dir()` = the forced dir or an error ("no lists: build the products first");
   `rel()` maps a bare GENERATED name to dir(). Delete TRACKED-as-lists, OUT, STAMP, tree_hash, differing,
   Mode, resolve, train(), cmd (`hot-lists`) and their tests; fix layout_switch test; delete
   regen_builds_never_rely_on_the_default_layout.
3. so.rs: drop Layout::Committed and run_regen_libc; build_archive (bench v3 measurement build, bench.rs:801)
   -> Layout::Ungrouped (RULING-worthy: v3 bench start-up rows lose grouping; alternative: trace it too);
   `finish_archive`/`group` calls without profile; trace_layout calls `regen.sh <root> --products <tdir>`;
   tracked_files/restore_tracked can stay (guard) or go.
4. main.rs: drop `hottext-repair`, `hottext-recheck`, `hot-lists` commands and doc rows (lines 38-40); `so`
   takes no arguments (was hottext::regen_mode).
5. layout/contracts.tsv: `keys=crates/libc-so/startup-text.keys` -> `keys=startup-text.keys` (and text-keys,
   STARTUP-DATA-WINDOWS); startup_ro.rs PROBE_KEYS and reads (lines 43, 306-310) -> bare names; layout.rs
   messages at 650/720/940 (no "run tools/hottext/regen.sh"; say "rebuild: cargo xtask so traces the layout").
6. probe_text.sh / startup_data.sh: LIELLA_HOT_LISTS required, order.py instead of merge.py, no --replace.
7. crates/core/tests/hot_text_products.rs: `generated_list` reads only the marker dir (panic without one);
   messages that say "run tools/hottext/regen.sh" / "listed in crates/libc-so/hot-text.list" reworded.
8. Docs/comments: CONTRIBUTING.md:140 area, xtask/src/affected.rs, coldbss.rs, iconv/mod.rs, cpuid.S, fold.S,
   hot-text.clusters:9 (merge.py --alias -> order.py --alias), keys.py:282, perf-ledger, SR-83 (record: leave),
   plan/layout-rethink.md (leave: history).
9. Pending record records/pending/LAYOUT-DELETE-LISTS-1.toml (`cargo xtask records new decision D-NEW --series
   D-X --story LAYOUT-DELETE-LISTS`), amends D-X.92 / D-X.56 / D-X.53.
10. Step 2 (pins): 49 Rust keys of the deleted list were required pinned (check_pinned); 15 carry an explicit
   key-only comment (LAYOUT-HOTKEY-INLINE-STABLE{,-2,-3}; scratch pins.tsv): cpu::init_with, heap map,
   process::exit, runtime::{run_fini,start}, stdio::new_slot, sys::vdso_fn, thread::each_block,
   catalog::variants::each, kernel vdso lookup_many, exit_handlers::{register,run_all}, intl::record_for,
   nscache::real_secs, stdio_compat::each_stream. Recommend: unpin those 15 in one commit, one wl_percall over
   the start-up rows against next (fast and release), keep each unpin whose rows are equal or faster; the other
   34 pinned keys look like slow-path outlining (`*_slow`, `*_general`, `move_large`): check each comment.
11. Proof: clippy, static, affected gate (--base origin/next; escalates to FULL: xtask change), plants, and
   default `so` + layout-check + hot_text_products both profiles on mei and keke.

## For the coordinator (brief step 5)

The ops side to retire in the same landing: the train's `hot-lists` step (the one that sets
ci_train_hot_lists and runs `cargo xtask hot-lists --in-place`) and the hot-lists lane (the one-at-a-time lane
for list-touching stories). The Liella side removes the `hot-lists` xtask command they call.

## Not to redo

The inventory above (files and items to delete) is complete for xtask/src/hottext.rs; `git grep` of
`hot-text\.list|startup-text\.keys|probe-text\.keys|startup-data\.keys|hot-text\.absent|hot-text\.unpinned|check_pinned|hottext-regen|hottext-repair|hottext-recheck|prune\.py|LIELLA_LAYOUT_TRACE`
outside handoff/, plan/07-decisions, records/decisions and docs/saturation gives the remaining 27 files.
