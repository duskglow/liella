# LIBM-TABLES-PER-FAMILY: generated tables per family, freshness check, fast32 per family

State: branch `task/libm-tables-per-family`, cut from `origin/next` f8b77d70; HEAD and job results in
`handoff/reports/LIBM-TABLES-PER-FAMILY.json`. Role shiki:~/w/sh1 (Haswell-EP).

## What changed

1. **Generated tables per family.** `cargo xtask tables` (crates/mathtools/src/tables.rs, `Out::close`) now writes five
   `@generated` files in crates/math/src/coeffs/: `exp_log.rs` (35 KB), `trig.rs` (22 KB), `series.rs` (8 KB),
   `mp_const.rs` (30 KB), `erf.rs` (84 KB, from special_tables.rs). The old `tables.rs` is gone. The hand-written
   `crates/math/src/coeffs.rs` declares them and re-exports them all as `coeffs::tables`, so no kernel's
   `use crate::coeffs::tables as t` changed. The binary rewrites only the files whose bytes changed (a family's
   generator change touches and rebuilds only its file). A new family: call `o.close("<family>.rs", "<doc>")` after
   its tables in `generate()`, and add `mod <family>;` plus `pub(crate) use super::<family>::*;` in coeffs.rs.
2. **Values unchanged, byte for byte.** The five files were cut from the old tables.rs at the family boundaries by a
   one-off script (their bodies concatenate to the old body: "identical"), and then the generator's own
   `tables --check` on the role reported `tables: up to date (5 files)`: the generator writes exactly these bytes, so
   the concatenated new tables equal the old ones.
3. **Freshness check.** `tables --check` names each stale/hand-edited (`is out of date`), missing, or leftover file (a
   file still carrying the generator header that the generator no longer writes, e.g. a `tables.rs` brought back by
   a merge). `cargo xtask static` runs it as `tables-fresh` (it was only in `cargo xtask test` before). Plants
   (handoff/libm-tables-per-family-plants.txt): one hand-edited hex word in coeffs/trig.rs, and a generator header
   change not rerun.
4. **fast32 per family.** `crates/math/src/fast32.rs` (31 KB) is the shared core: `Fast` and `eval`, the shared
   constants and tables (EXP2_TAB, LOG_INV, LOG_C stay there because `crates/math/fits/fast32_tables_gen.py` reads them
   from fast32.rs, and the `include!("fast32_tables.rs")`), `round`, `eval_nearest` and the entry macros. Kernels:
   `fast32/exp_log.rs` (exp, log, log1p, expm1, cbrt, pow; 17 KB), `fast32/trig.rs` (4 KB), `fast32/inv_trig.rs`
   (10 KB), `fast32/hyperbolic.rs` (8 KB); tests `fast32/tests.rs` (41 KB, one module, not split). Pure move by script:
   only `pub(super)` on moved private items, `mod`/`use` lines (`use <family>::*` in the core, `use super::*` in each
   family, `pub use` of the public entries), a header per file, the tests' dedent. Release is fat LTO, one CGU.
5. **fast64.rs** holds one family (exp/log/pow and their nearest stages), so it is not split (brief: only if more than
   one family).

## Flow rule text for the coordinator (add to the developer flow)

Never open an `@generated` file (crates/math/src/coeffs/{exp_log,trig,series,mp_const,erf}.rs and the like). To change
a table, change its generator (crates/mathtools/src/tables.rs, special_tables.rs) and run `cargo xtask tables` as a
job; `cargo xtask static` (tables-fresh) fails on a stale file and names it. To see one constant,
`grep -n '<NAME>' crates/math/src/coeffs/*.rs`. New float-family kernels go in their own `crates/math/src/fast32/<family>.rs`;
read fast32.rs, fast64.rs and fast32/tests.rs by range (`grep -n` first).

## Merge note for in-flight libm branches

Every libm branch that touched `coeffs/tables.rs` or `fast32.rs` conflicts with this once. Resolve by merging `next`,
then: generated files are never hand-merged: take next's side of `coeffs/` (delete any conflicted `tables.rs`), keep
your generator change in crates/mathtools/, add a `o.close(...)` for a new family if you added one, and run
`cargo xtask tables` (the leftover-file check flags a stray `tables.rs`). For fast32.rs, move your kernel hunks into
the matching `fast32/<family>.rs` (new private items there need `pub(super)`), your tests into `fast32/tests.rs`.

## Not to redo / open

- Splitting `fast32/tests.rs` per family is a possible follow-up (shared helpers `accurate`, `agree`, `exhaustive`,
  `ALL`, `NAMES` would move to a `tests/common` part); it was left whole to keep this a pure move.
- No RULING NEEDED. No compat or D-row: the layout is a local detail; `coeffs.rs` stays the hand-written module file
  (the brief's "coeffs/mod.rs"; the crate uses the `coeffs.rs` + `coeffs/` layout).

## Results (code head 26b0fa10; later commits are handoff only)

- clippy (`cargo clippy --workspace --all-targets --all-features -D warnings`) and `cargo xtask static` clean on the
  role; static printed `tables: up to date (5 files)` (= the identical-values proof above).
- Plants `libm-tables-per-family-plants` (cargo xtask plants, /dev/shm/claude/libm-tables-per-family/plants-1): 2 CAUGHT,
  0 SURVIVED, no WARN lines.
- Counts `libm-tables-per-family-counts-before` (origin/next f8b77d70, in the role before the sync) and
  `-counts-after` (head), `cargo xtask bench --counters-only --only cbench_libm`, 169 rows: 25 identical, 144 differ by
  at most 0.15% (acoshf 136.4 -> 136.6), every one upward, and equally in rows this change cannot reach (x87 `acosl`
  176064 -> 176084, `acosf128`): the bench's own spread column says 1-2%. Not identical as numbers, so I did not claim
  identity from them; the deterministic check is the code comparison below.
- `libm-tables-per-family-codecmp` (SUPERSEDED): compared a RAM `xtask so` build of base with the role's bench-built
  head library; the two were built differently (5543 vs 4655 text symbols, differences in regex/printf/glob), so it
  proves nothing. `libm-tables-per-family-codecmp2` builds base (branch diff reversed with patch -R) and head the same
  way in RAM and compares .text and every cbench_libm entry's instructions (addresses and names masked); its result is
  in the report json / hand-back.
- Gate `libm-tables-per-family-gate`: FULL tier (`cargo xtask affected --tier full --run` under ~/bin/gate), left
  running at hand-back for the dispatcher.

Next step: none required; optional follow-up: split fast32/tests.rs per family.
