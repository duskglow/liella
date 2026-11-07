# HOTTEXT-STARTUP-COMPLETE: every start-up function in the hot group, and a stale key fails the build

Id: HOTTEXT-STARTUP-COMPLETE
Kind: code
Size: M
Model: opus
Effort: high
Plan: handoff/layout-contracts-plan.md (finding 1, section 2c C1, proposed D-X.NEW-3)

## The change

On origin/next 9643c81b0 `liella_boundary::runtime::start::<false>` (the first Rust function of start-up, 5.1 KiB)
and `runtime::apply_irelative` are outside the hot group: `crates/libc-so/hot-text.list` line 4 still holds the key
of the non-generic `runtime::start`, which matches no section since the function became generic, and
`xtask/src/hottext.rs` drops an unmatched key silently. In cold `.text` the function moves with any growth before it
(LIBM-F64-LOG-FAMILY-WIRE-2: it crossed 0x6f0000, startup faults +1, CI red).

1. Regenerate the list (`tools/hottext/regen.sh`, both profiles, union) on origin/next. Check that both functions
   are grouped (`readelf -SW libc.a`: `.text.sorted.h*`); add them to `init_cold_startup_functions_are_grouped`.
2. Give each key its tier (startup, fork, stdio, strings, locale, tool, extra) and the profiles it was traced in
   (fast, release), written by regen (`merge.py`), e.g. `<key>\t<tier>\t<profiles>`; keep the order (HOT-ORDER).
   Update `hottext.rs` parsing and its unit tests.
3. In `hottext::group`: a startup- or fork-tier key traced in the profile being built that matches no function
   section is an error naming the key and saying "renamed, now generic or inlined: run tools/hottext/regen.sh".
   Other tiers: print the count of unmatched keys.
4. Measure before/after with counters only: the `hot_text_products.rs` counts (startup, setlocale, tool, fork) on
   both profiles, and the per-section page-fault addresses (`perf record -e page-faults -d` of the probe, method in
   the plan section 3). Expected: one or two fewer start-up text faults. Lower no bound in this story unless the
   counts are deterministic on both hosts (rule in the test's comments); record the counts in the D-row.
5. Plants (`cargo xtask plants`): the key of `runtime::start::<false>` removed from the list (must fail `so` with the
   new message); a startup-tier key renamed to a non-existent one; the guard's comparison inverted.
6. D-X.NEW-3 row (coordinator numbers it).

Rules: products and counters only; no timing claims. Clean room as always (layout tooling, no libc source).
