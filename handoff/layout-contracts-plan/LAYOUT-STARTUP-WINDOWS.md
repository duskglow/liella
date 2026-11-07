# LAYOUT-STARTUP-WINDOWS: the start-up text window contract, checked at product build

Id: LAYOUT-STARTUP-WINDOWS
Kind: code (tooling)
Size: M
Model: opus
Effort: high
Plan: handoff/layout-contracts-plan.md (finding 1-3, section 3; proposed D-X.NEW-2). Prototype:
`tools/layout/startup_windows.py` on branch `task/layout-contracts-plan-proto` (read it; do not merge that branch).

## The change

1. Port the prototype into xtask (`xtask/src/layout.rs` kind `startup-text-windows` if LAYOUT-MANIFEST has landed,
   else `xtask/src/startup_windows.rs` run at the end of `cargo xtask so`): link
   `crates/core/tests/oracle/page_faults.c` against the products exactly as `hot_text_products.rs` does (with
   `-Wl,-Map`), map the start-up keys (build-stable keys, `tools/hottext/keys.py` rules; port `key()` or call it) to
   `nm -S` ranges (assembly entries without `.size` count their entry byte), count distinct 64 KiB windows, and fail
   when the count exceeds the budget or a start-up function outside the hot group straddles a window boundary.
   The message names the contract, every function in a window outside the group, and the straddling one.
2. The start-up key set is generated, not hand-written: `tools/hottext/regen.sh` writes
   `crates/libc-so/startup-text.keys` (`@generated`) from the trace of `page_faults true` (the probe the bound
   measures). Budget per profile in the manifest row (or the keys file header until the manifest exists): set it to
   the count on origin/next for each profile (prototype: 9, test profile); release profile to be measured.
3. Second rule, STARTUP-TEXT-GROUPED (every start-up key inside the group): reported as a warning until
   HOTTEXT-STARTUP-COMPLETE has landed, then an error.
4. Evidence: the check passes on origin/next and fails on the LIBM-F64-LOG-FAMILY-WIRE-2 head cdf46a2e5 (prototype
   jobs in the plan, section 3; repeat it with the xtask version on both heads' products).
5. Plants (`cargo xtask plants`): budget minus one; a 64 KiB filler function placed before a start-up function in cold
   text (or the startup key of the group removed so the function leaves the group); the window size constant
   changed; the straddle test inverted. Unit tests on a synthetic `nm`/map text.
6. Each later `hot_text_products.rs` bound raise for a layout-only delta quotes this check's output (rule d in the
   plan). D-X.NEW-2.

Products and counters only; no timing.
