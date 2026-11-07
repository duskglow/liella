# TVA-XTASK-RUN-ONCE: the exports-interposed pass does not re-run binaries that cannot link liella-boundary

Id: TVA-XTASK-RUN-ONCE
Kind: code
Priority: 72
Size: S
Host: -
Model: opus
Effort: medium
BUDGET: 50 tool calls / 100k

**No owner ruling needed (CONSOLIDATE): every test still runs once, on the same binary.**

## Why
TEST-VALUE-AUDIT (handoff/test-value-audit.md) found that pass 2 of the FULL gate re-runs xtask's 236 unit
tests (454 test-s per train) and ends on its slowest one (`affected::tests::csys_falls_back_to_full_when_unsure`,
51-80 s). xtask has no dependencies (xtask/Cargo.toml), so the export features cannot change it; CI-GATE-DIET's
rule (xtask/src/nextest.rs `may_hold_boundary_code`) keeps it because xtask's own source contains the string
`liella_boundary` (BOUNDARY_MARK itself, main.rs, policy.rs, forkalloc.rs, hottext.rs). Estimated -0.5 to -1.3
min per train. NOT VERIFIED.

## The change
In `unchanged_binaries`, also count as run-once every binary whose package does not depend on liella-boundary,
directly or through other packages (from `cargo metadata --format-version 1`'s resolve graph, read once).
Keep the byte check for the others; keep failing safe (metadata unreadable = the byte rule alone). Print the
reason per binary (`once: xtask::bin/xtask (no dependency on liella-boundary)`). Do not narrow the byte marker
itself (a string in a dependent crate must still count). Check which other packages this frees (list them in
the PR; mathtools/spectest/abi are candidates) and that none of them builds or runs a C program against the
products in a way pass 2 relies on.

## Proof (every plant still caught)
Plants with `cargo xtask plants`: (1) treat a dependent package as independent (invert the graph test): caught
by a unit test that builds a small resolve graph where crate A depends on liella-boundary through B and expects
A not to be run-once; (2) make metadata failure return "all once": caught by a test expecting the byte rule
alone; (3) drop the byte rule: caught by the existing `a_binary_holding_boundary_code_is_never_run_once`.
Show from a pass-2 listing that `xtask::bin/xtask` is `once:` and that the pass-2 test count fell by 236.

## JOBS
gate (direct tier, `--base origin/next`), plants. Name each label on `JOBS:`.

## Hand-back first line
`COMPLETE: xtask and <n> other dependency-free binaries run once; pass 2 -<k> tests, plants <k>/<k> CAUGHT`.
