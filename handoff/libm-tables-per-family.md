# LIBM-TABLES-PER-FAMILY(-2): generated tables per family, freshness check, fast32 per family

State: branch `task/libm-tables-per-family-2` (continues `task/libm-tables-per-family`), origin/next f2157504 merged
in (merge 1726199a); HEAD and job results in `handoff/reports/LIBM-TABLES-PER-FAMILY-2.json` (the first run's in
`LIBM-TABLES-PER-FAMILY.json`). Role kanon:~/w/kn1 (Broadwell-EP). Stacked on it: LIBM-F32-SINF-SHRINK-4,
LIBM-SPECIAL-FAST-TIER-5.

## What changed

1. **Generated tables per family.** `cargo xtask tables` (crates/mathtools/src/tables.rs, `Out::close`) writes five
   `@generated` files in crates/math/src/coeffs/: `exp_log.rs`, `trig.rs`, `series.rs`, `mp_const.rs`, `erf.rs`
   (from special_tables.rs). The old `tables.rs` is gone. The hand-written `crates/math/src/coeffs.rs` declares them
   and re-exports them all as `coeffs::tables`, so no kernel's `use crate::coeffs::tables as t` changed. The binary
   rewrites only files whose bytes changed. A new family: `o.close("<family>.rs", "<doc>")` after its tables in
   `generate()`, plus `mod <family>;` and `pub(crate) use super::<family>::*;` in coeffs.rs.
2. **Values unchanged, byte for byte** (first run: the five files concatenate to the old body; `tables --check` up to
   date). Second run: `cargo xtask tables` rerun on the merged head left every coeffs file byte-identical (sha256
   before/after), and static's tables-fresh printed `tables: up to date (5 files)`.
3. **Freshness check.** `tables --check` names each stale/hand-edited (`is out of date`), missing, or leftover file
   (generator header but no longer written, e.g. a `tables.rs` brought back by a merge). `cargo xtask static` runs
   it as `tables-fresh`. Plants: handoff/libm-tables-per-family-plants.txt.
4. **fast32 per family.** `crates/math/src/fast32.rs` is the shared core (`Fast`, `eval`, shared constants and
   tables, `round`, `eval_nearest`, the entry macros); kernels in `fast32/exp_log.rs`, `fast32/trig.rs`,
   `fast32/inv_trig.rs`, `fast32/hyperbolic.rs`; tests in `fast32/tests.rs` (one module). Pure move by script
   (only `pub(super)` on moved private items, `mod`/`use`/`pub use` lines, a header per file, the tests' dedent).
   Run 2: next's LIBM-F32-HYPERBOLIC-4 code (`asinh_core_with`, `acosh_core_with`, public `asinh_nearest_sqrt`,
   `acosh_nearest_sqrt`) was put in by rerunning the script on next's fast32.rs, extended so the hyperbolic entries
   after the last hyperbolic `nearest_fn!` go to `hyperbolic.rs` too (core: `pub use hyperbolic::{...}`). Proofs:
   the extended script reproduces run 1's committed split byte for byte from f8b77d70; a line-multiset check of
   next's fast32.rs against the six output files shows only header/mod/use lines added.
5. **fast64.rs** holds one family, so it is not split.

## Flow rule text for the coordinator (add to the developer flow)

Never open an `@generated` file (crates/math/src/coeffs/{exp_log,trig,series,mp_const,erf}.rs and the like). To change
a table, change its generator (crates/mathtools/src/tables.rs, special_tables.rs) and run `cargo xtask tables` as a
job; `cargo xtask static` (tables-fresh) fails on a stale file and names it. To see one constant,
`grep -n '<NAME>' crates/math/src/coeffs/*.rs`. New float-family kernels go in their own `crates/math/src/fast32/<family>.rs`
(entries too: never add a family's kernel or entry to the core fast32.rs); read fast32.rs, fast64.rs and
fast32/tests.rs by range (`grep -n` first).

## Merge note for in-flight libm branches

Resolve by merging `next`, then: generated files are never hand-merged: take next's side of `coeffs/` (delete any
conflicted `tables.rs`), keep your generator change in crates/mathtools/, add `o.close(...)` for a new family, and
run `cargo xtask tables` (the leftover-file check flags a stray `tables.rs`). For fast32.rs, move your kernel hunks
into the matching `fast32/<family>.rs` (new private items there need `pub(super)`, new public entries a `pub use` in
the core), your tests into `fast32/tests.rs`.

## Results, run 2 (code head 1726199a; later commits are handoff only)

- `libm-tables-per-family-2-check`: rc 0: `cargo xtask tables` (byte-identical), `cargo clippy --workspace
  --all-targets --all-features -- -D warnings` clean, `cargo xtask static` clean.
- `libm-tables-per-family-2-plants` (cargo xtask plants, /dev/shm/claude/libm-tables-per-family-2/plants-1): 2 CAUGHT,
  0 SURVIVED, 0 WARN-RC-ONLY/NO-MSG (one WARN-DEV-SHM note: irrelevant, the plant command is `cargo xtask static`).
- `libm-tables-per-family-2-codecmp`: base (origin/next f2157504: branch diff reversed) and head built the same way
  (`cargo xtask so` in RAM): libc.so.6 .text IDENTICAL (3088069 bytes, sha256 28075c7ec313b086 both), so every libm
  entry, float ones included, runs the same instructions. Then `cargo xtask bench --counters-only --only
  cbench_libm` in both copies (169 rows, Liella instructions/call): 44 identical, 125 differ by -0.11%..+0.06%, both
  directions, x87 rows included (acoshf 94.4 -> 94.3, acosl 176063 -> 176067; asinhf 106.0, expf 70.1, sinf 167.2
  equal): measurement spread over identical code, not a change; the identical .text is the proof.
- `libm-tables-per-family-2-gate`: FULL tier (`cargo xtask affected --tier full --run` under ~/bin/gate), left
  running at hand-back for the dispatcher.

## Not to redo / open

- Splitting `fast32/tests.rs` per family is a possible follow-up (shared helpers would move to a common part).
- No RULING NEEDED, no compat or D-row (layout is a local detail).

Next step: none required; optional follow-up: split fast32/tests.rs per family.
