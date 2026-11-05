# PERF-GETTEXT-LANGUAGE-STEADY(-5) (branch task/perf-gettext-language-steady-5, role shiki:~/w/sh1)

State: ready to land; target (glibc 466 steady / 455 found) not reached. Code head e1ae2628 (the jobs below ran on it);
the commit after it only holds this handoff and the report. Base: -4's branch (origin/next f2157504 + -3's work); origin/next
has moved on since (dc65f761), not merged (the brief allows no merge).
Commits of -5: 8577660f re-applies -4's 48310c65 (find_in_hinted split: #[inline] hint test `hinted` + #[inline(never)]
`find_in_unhinted`); e1ae2628 fixes its test (TryVec has no Clone: `ix.to_vec()`); 90f4ce34 adds the list cases to the
`key_address_accept` unit test (crates/legacy/src/intl.rs). No new unsafe, no ABI or behaviour change.
Decisions in force: D-7.5, D-7.12; saturation 14 row 6 (runs 1-6 recorded; this run's numbers are below, not yet in the register).

## Why -4's plants survived (the brief's first question), settled
1. `cargo xtask plants` builds the products once, before any plant; `catalogs_products` links `target/liella/libc.a`, so
   every mutant was tested against the unmutated library. With `cargo xtask so &&` in the cmd (plants-1) the mutants were
   in the library and still all 4 SURVIVED:
2. catalogs_diff.c never reaches the distinguishing cases. The direct test of `Resolution::matches` is the unit test
   `key_address_accept`; it covered both address conditions, and it had no list case. 90f4ce34 adds them (a list rewritten in
   place at the same address, another list of the same length, no list: no match; the same list, an equal copy: match).
   plants-2 runs the 4 on `cargo test -p liella-legacy --lib intl`: all CAUGHT.

## Jobs (shiki; all rc 0 on e1ae2628)
- perf-gettext-language-steady-5-gate: `cargo xtask affected --base origin/next --run` under ~/bin/gate: 30 of 235 targets
  + 3 unit-test libs, 2750 passed, 0 failed, peak 11G. Clippy (`cargo xtask clippy`) and `cargo xtask static` ran clean first.
- perf-gettext-language-steady-5-plants-2: ~/work/PERF-GETTEXT-LANGUAGE-STEADY-5/plants2.txt, dir
  /mnt/zram/claude/PERF-GETTEXT-LANGUAGE-STEADY-5/plants-2: 8 CAUGHT (4 matches + 4 hint plants), 0 WARN.
- perf-gettext-language-steady-5-percall: wl_percall --spec validation/percall/gettext.c --base c27f580e (WL_BASE_EXACT=1).
- Superseded: perf-gettext-language-steady-5-plants-1 (rc 1, diagnosis: catalogs_products cmd with products rebuilt).

## Counts (wl_percall, Haswell-EP 26 vCPU, instructions per call; glibc / base -4 head / new e1ae2628)
gettext_language_steady 466 / 684 / 652; gettext_found 455 / 558 / 526; ngettext_found 597 / 662 / 630;
gettext_language_alternating 2211 / 1287 / 1255. -32 on every row; no row slower. Parity by instructions: not yet.

## Open / next step (in order)
1. Add run 7 (this split, -32 per row) to saturation 14 row 6 (docs/saturation/), left out so the gated head stays the final one.
2. Profile first: -4's trace job (keke perf-gettext-language-steady-4-list-found) never ran. Run validation/wl_trace.sh
   --list for gettext_found / gettext_language_steady on e1ae2628 and write the per-phase split here.
3. Then the selector compare in `hit` (~20: `ctype_identity` match + charmap load) and translate's ~60 of key setup before
   the slot test. LANGUAGE's ~95: runs 2-4 found no safe cut without new unsafe in liella-boundary (RULING NEEDED + DO NOT LAND).
Things not to redo: the 48310c65 split is in; plants of products-linked code must rebuild the products in the cmd, or use the
unit test of the planted function.
Plan rows: saturation 14 row 6, D-7.5, D-7.12.
