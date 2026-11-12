# LAYOUT-SINGLE-FAULT-TEST (#4869): catch a one-fault start-up loss by object

## State
Branch `task/layout-single-fault-test` (cut from origin/next e75629121). Code commits 84dfabd83 (check + layout fix),
25957c0c7 (mode guard fix, pending record, plants), 42bae48c0 (record series). Report: this file;
`handoff/reports/LAYOUT-SINGLE-FAULT-TEST.json`. Plants: `handoff/LAYOUT-SINGLE-FAULT-TEST-plants.txt`.

## What was found
The brief's premise (the check does not trace `locale`/`tool`) was already out of date: LAYOUT-STARTUP-STATE-LOCALE
(#4915) made `init_startup_data_accesses_are_in_the_data1_cluster` trace every mode in `STARTUP_PROBE_MODES`, and
an object such as `_nl_msg_cat_cntr` in a `.bss` section of its own was already named by it. LAYOUT-DATA-CLUSTERS-11's
ldc11-msg-cat-cntr survived because its plant command ran only `init_short_programs` (a count test), not this check.
A first trace of next (fast products, mei) showed the check's real blind spots:
* size cut-off 4 KiB, while `startup-state.list` declares up to 8 KiB (`STATE_DECLARED_MAX`): heap::HEAP (5376
  bytes, every start writes it) could leave the cluster unseen;
* a member's shared `.data`/`.bss` (C or assembly object without a section of its own) was never flagged: the
  `rebind` probe's first close read `liella_machine_aio_used` (1 byte, csys points.S, shared `.data`), a page
  whose place followed emission order;
* the start object (`liella-crt1.o`) was out of scope; a new probe mode could be added without a trace.

## Change
* `crates/core/tests/hot_text_products.rs`: the check names every library access (libc.a and the start object)
  outside `.data1` and, for first calls, the `.lbss` cluster (D-X.74): own sections up to 8 KiB, shared sections by
  the symbol hit (nm), each line ending "outside .data1". It fails if a `page_faults.c` mode is not in
  `STARTUP_PROBE_MODES` (`true` is the default mode, selected by no strcmp).
* `csys/src/machine/points.S`: `liella_machine_aio_used` in `.data.liella_machine_aio_used`;
  `crates/libc-so/startup-state.list`: `symbol liella_machine_aio_used` after `cluster first-call` (an assembly
  member's, so it stays in `.data1`, after the every-process tier).
* `records/pending/LAYOUT-SINGLE-FAULT-TEST-1.toml` (D-NEW, series D-X; the brief said D-10, which is libm).

## Counts (mei, fast products, this branch; every run identical)
startup only 11 (bound 17), setlocale + printf 49 (51), tool 52 (56), fork child 5 (6), localtime and
localtime + mktime 19, float 4, mktime / strings / rebind 0. Bounds unchanged. NOT VERIFIED against next on the same
host (no next count was run); the moved byte is read only by the rebind probe's close, outside its counted window.

## Jobs (mei)
layout-single-fault-test-diag1 / -diag2 (diagnostics: trace of next's tree; first branch run, which found the
mode-guard bug), -lint (records columns for D-10, superseded), -lint2 (workspace clippy, xtask clippy, static,
records check: rc 0), -gate (direct tier, --base origin/next; still running at hand-back), -plants (rc 0: 3 CAUGHT,
lsft-heap-over-4k WARN-NO-MSG: the object was named as `.lbss._RNv...4heap4HEAP (5376 bytes ...) outside .data1`, the
regex expected `.bss.`; superseded), -plants2 (same plants, fixed regex; from ~/work because the role could not be
re-synced during the gate; the code tree is head 42bae48c0's, later commits are handoff-only).

## Plants (plants-1)
ldc11-msg-cat-cntr CAUGHT (`locale: .bss._nl_msg_cat_cntr ... outside .data1`), lsft-locale-name-count CAUGHT,
lsft-aio-used-shared CAUGHT (`rebind: liella_machine_aio_used (1 bytes) in shared .data ...`), lsft-heap-over-4k
WARN-NO-MSG (see above).

## Next step
None required. Optional: bounds could tighten if next on the same host reads below them (coordinator's call).
