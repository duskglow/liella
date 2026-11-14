# DL-LIBC-START-9 report

Branch `task/dl-libc-start-9`, stacked on DL-LIBC-START-8's head e55b7c412. Role shiki:~/w/sh2.

## Cause of -8's red gate
kanon dl-libc-start-8-gate failed only hot_text_products `init_written_startup_objects_are_in_the_data1_cluster`.
Rerun on the inherited head (job dl-libc-start-9-check: clippy rc 0, static rc 0, test rc 101) named the object:
`_RNvNtCs..._15liella_boundary7runtime10EARLY_DONE (1 bytes) in .bss._RNv...EARLY_DONE`. EARLY_DONE is this branch's
dynamic-path flag (`early_init` sets it under Liella's ld.so, `start` reads it; D-27.24). -8's regen added it to
startup-data.keys (rw, release), but crates/libc-so/startup-state.list (hand-written, D-X.64) did not declare it,
so `move_startup_state` left it out of `.data1`. The fix declares it in the every-start tier (no test loosened).

## Commits
- a45de8a07 Declare start's dynamic-path flag as start-up state (startup-state.list + plants file).
- (this commit) handoff, report, json.

## Results (shiki)
- dl-libc-start-9-check (inherited head): clippy 0, static 0, the failing test 101 (cause above). Superseded.
- dl-libc-start-9-t2 (a45de8a07): `cargo xtask so` + all of hot_text_products: 23 passed; 74 written start-up
  objects in .data1 (73 before); STARTUP-DATA-WINDOWS ok, 13 written start-up data pages (budget 20).
- dl-libc-start-9-plants: 1 plant, 1 CAUGHT (early-done-undeclared, by the test above), 0 WARN.
- dl-libc-start-9-gate: `cargo xtask affected --run --base origin/next`, escalated to FULL (product linker inputs
  and xtask changed on the branch); started detached on a45de8a07, left for the dispatcher.

## Ruling #5171
Applied: the five refused keys (find_cached, attach_charmap, TscGate::fresh, variants::each, record_for) are not in
hot-text.list or hot-text.unpinned (-8 had already kept them out; checked by grep). No pinning here.

## INFORMATIONAL
- At hand-back the branch conflicts with next in crates/libc-so/hot-text.list and hot-text.unpinned only (both sides
  edited the hot lists; no merge was allowed in this brief). The hot-lists train regenerates them; otherwise a
  continuation merges next and keeps -8's three dead keys dropped and ruling #5171's five keys out.
- `runtime::PROGRAM_INIT` (24 bytes) is written by every start under Liella's ld.so; the start-up trace is of a
  static program, so it is not declared. Declare it once a dynamic start-up fault trace exists (handoff next step 6).
- The local handoff edit used a python heredoc (scratch-side, not on a host); noted for the record.
