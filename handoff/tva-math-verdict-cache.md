# TVA-MATH-VERDICT-CACHE: math sweep verdict cache

State: branch `task/tva-math-verdict-cache-2` (continues `task/tva-math-verdict-cache`); code commit 3f71c45d0
(the pass-0 fix) on top of the merged head c9d9723f0. Owner ruling #3820 ("agree, do it"). Plan rows: D-10.57, D-10.3.

## What it does
`crates/core/tests/support/verdict.rs`. A shard prints `verdict cached: <name> <key>` and returns when its complete
input key equals the key recorded by its last green run. Otherwise it removes its record, runs as before, and
`pass()` records the key after the last assertion, but only if the key read again at that point is unchanged.
Records are kept in `target/test-verdicts/<name>` (written to a temporary file, then renamed; per tree and host).
Key (D-10.57): test binary bytes (`/proc/self/exe`), the `/proc/cpuinfo` flags line, the kernel release and the shard
name. C-oracle shards also add the shard index and count, both programs' bytes, and every shared object the
reference program loads (`LD_TRACE_LOADED_OBJECTS=1`). The fast_gamma test adds `LIELLA_GAMMA_SWEEP`.
An input that cannot be read makes the key incomplete: the shard always runs and nothing is recorded.
Wired: complex_ld_cr, complex_f128, special_ld, special_f128, complex, elem_ld and special products (each shard);
every test in special_cr and fast_gamma; the lib unit test `fast32::tests::inverse_trig_cores_are_within_their_bounds`.
Tests: `crates/math/tests/verdict_cache.rs` (5) and the fixture `crates/math/tests/oracle/verdict_probe.c`.
`LIELLA_NO_VERDICT_CACHE=1` runs every shard (documented in `xtask/src/main.rs`'s `test` row and CONTRIBUTING.md).

## -2: the CI red (pass0_only_targets_are_pure_rust)
Cause: CI went red after the merge with TVA-XTASK-RUN-ONCE. special_cr is `nextest::PASS0_ONLY`, and that check
refused any `#[path` include, because a scan of the target alone cannot see into one. The verdict cache added such
an include. The include was not harmless as written: verdict.rs held `std::process::Command` (the loader query).
Fix, option (a) with a structural split (D-10.57 records why):
- The loader query (`loaded_objects`, `Key::loaded_by`) moves to `crates/core/tests/support/verdict_programs.rs`.
  Only the C-oracle sweeps and verdict_cache include it. verdict.rs keeps a generic `Key::resolved(label, path, fn)`
  input and now runs no program. The key text is byte-identical (`loaded by\t<prog>` and its files), so existing
  records stay valid.
- `xtask/src/nextest.rs`: `PASS0_INCLUDES` (named allow-list: verdict.rs only), `PASS0_BANNED`, and
  `pass0_violations` (all `#[cfg(test)]`). A pass-0-only target may include only listed files. Each listed file is
  scanned with the target's own rules. An unlisted, unreadable or nested include is refused.
  The new unit test `pass0_only_includes_are_listed_and_scanned` covers each rule.
- Rejected: (b) on its own, because moving verdict into a crate would hide it from the scan altogether.
  Rejected: (c), because it adds a second run of a sweep that the interposed exports cannot change (#889).

## Results (-2)
- Inherited head c9d9723f0: clippy and static clean on mei s4 before any change.
- Final head: `cargo xtask clippy` and `cargo xtask static` clean; `cargo test -p xtask --bin xtask pass0`: 2 passed.
- Gate `tva-math-verdict-cache-2-gate1` (mei s4, `~/work/TVA-MATH-VERDICT-CACHE-2/gate.sh`): clippy, static,
  `cargo test -p xtask`, then `cargo xtask affected --run --base origin/next --files <every changed file outside
  xtask/ and handoff/>`. Without `--files`, the xtask change escalates `affected` to FULL, which is not a
  developer's job. Result: clippy, static and `cargo test -p xtask` (249 passed) green. In the direct tier,
  products, liella (both passes), liella-abi, liella-boundary, liella-ld and liella-legacy were ok. liella-math (the
  cold sweeps: this role had no records yet) was still running at hand-back; the dispatcher reads the job's rc.
- Plants `tva-math-verdict-cache-2-plants1` (`cargo xtask plants`, `~/work/TVA-MATH-VERDICT-CACHE-2/plants.txt`, 8
  plants). Six target the include check: unscanned include, unlisted include, unreadable include, nested include,
  unscanned target, and verdict_programs.rs on the list. Two target the moved key code: libm dropped from
  `loaded_objects`, resolved files not added. Result: rc 0, 8 CAUGHT, 0 SURVIVED, 0 WARN. Both baselines passed.
- -1's results stand for the unchanged parts: plants 12/12 CAUGHT (`tva-mvc-plants1`); timing 109 shards, 1770 s
  cold and 3 s cached on mei (`tva-mvc-timing1`).

## Open / informational
- The gain depends on CI keeping `target/` between trains on a role. A fresh target per train means no gain (still
  correct). The coordinator should confirm which CI does.
- A liella-math change can change a test binary's bytes, and with it rerun C-oracle families it does not touch (-1's
  timing). This costs runs only, never a skip.

## Nightly / release full run
`LIELLA_NO_VERDICT_CACHE=1 cargo xtask test` (the ops side schedules it; nothing in the repository sets it).

## Next step
Coordinator: land through the normal train and schedule the nightly run.
