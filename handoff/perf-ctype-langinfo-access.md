# PERF-CTYPE-LANGINFO-ACCESS handoff (-2 run: verification only, no new code)

Branch task/perf-ctype-langinfo-access-2. Code commit 94ea6a7 (crates/boundary/src/locale.rs): ctype_slot tail-calls
ctype_fill; byte_class indexes from the stored word. Host keke, role n3. clippy and static passed earlier (pcla-lint rc 0).

## This run
- Synced role n3 to the head; started detached jobs on keke: perf-ctype-langinfo-access-2-gate (clippy, static, affected
  --base origin/next --run) and perf-ctype-langinfo-access-2-plants (fill-skipped, range-test-dropped, wrong-slot-offset;
  script ~/work/perf-ctype-langinfo-access-2/plants.sh, fresh RAM copy). Both sat in the keke slot queue for 20+ min.
- Old-run jobs perf-ctype-langinfo-access-gate / -percall-a / -plants (role n4, old story name) were still queued: not named.
- NOT VERIFIED: gate, plants results; no before/after counts (percall not started: it would share role n3 with the gate).

## Next step
1. Read the two jobs' logs in keke ~/logs. 2. Run percall (validation/percall: ctype_b_loc, wcwidth_2byte, toupper_isalpha)
before/after after the gate. 3. Then items 2-5 of the earlier list: nl_langinfo flat per-category array (core view.rs),
isalpha masked class, wcwidth early narrow answer, saturation-register notes (no lab host names outside handoff/).
