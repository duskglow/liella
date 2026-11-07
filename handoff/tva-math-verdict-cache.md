# TVA-MATH-VERDICT-CACHE: math sweep verdict cache

State: branch `task/tva-math-verdict-cache` (from `origin/next` a09927ac); code commit 00c4d11 plus this handoff.
Owner ruling #3820 ("agree, do it").

## What it does
`crates/core/tests/support/verdict.rs`. A shard prints `verdict cached: <name> <key>` and returns when its complete
input key equals the key recorded by its last green run. Otherwise it removes its record, runs as before, and
`pass()` records the key after the last assertion, but only if the key read again at that point is unchanged.
Records are kept in `target/test-verdicts/<name>` (written to a temporary file, then renamed; per tree and host).
Key (D-10.51):
- test binary bytes (`/proc/self/exe`)
- `/proc/cpuinfo` flags line
- kernel release
- shard name
- C-oracle shards also: shard index and count, both programs' bytes, and every shared object the reference program
  loads (resolved by the dynamic loader with `LD_TRACE_LOADED_OBJECTS=1`)
- `LIELLA_GAMMA_SWEEP` for the fast_gamma test it sizes

An input that cannot be read makes the key incomplete: the shard always runs and nothing is recorded.
Wired: complex_ld_cr, complex_f128, special_ld, special_f128, complex, elem_ld and special products (each shard);
every test in special_cr and fast_gamma; the lib unit test `fast32::tests::inverse_trig_cores_are_within_their_bounds`.
Tests: `crates/math/tests/verdict_cache.rs` (5) and the fixture `crates/math/tests/oracle/verdict_probe.c`.
`LIELLA_NO_VERDICT_CACHE=1` runs every shard. This is documented in the `cargo xtask test` row of
`xtask/src/main.rs` and in CONTRIBUTING.md.

## Nightly / release full run (requirement 4)
`LIELLA_NO_VERDICT_CACHE=1 cargo xtask test`. The variable is inherited by every test process, so every
shard runs and green shards record again. The coordinator schedules this on the ops side. Nothing in the repository
sets the variable.

## Results
- Plants `tva-mvc-plants1` (mei, `cargo xtask plants`): 12/12 CAUGHT, 0 WARN. Each of the three libm faults ran after
  the baseline recorded a green verdict for the same command, and its shard FAILED:
  - csqrt `sqrt_general` (complex_ld_cr s0)
  - `j0_accurate` (special_cr)
  - the fast_gamma Taylor argument

  Key plants (each caught by a unit test): drop libm from the key, drop the test binary, CPU flags, kernel release,
  program bytes, shard; record at start; override ignored; re-read ignored.
- Gate `tva-mvc-gate1` (direct tier, `--base origin/next`): GATE_RESULT
- Timing `tva-mvc-timing1` (RAM copy on mei, `~/work/TVA-MATH-VERDICT-CACHE/timing.sh`): TIMING_RESULT

## Open / informational
- The gain depends on CI keeping `target/` between trains on a role. If the gate tree's target is fresh for every
  train, no record survives and every shard runs (correct, but there is no gain). The coordinator should confirm this.
- A change anywhere in liella-math can change a test binary's bytes even where the sweep does not call the changed
  code. That only makes shards run, never skip. See the timing's "touched" phase for what reran.
- Pass 2 (GATE-PASS2-MATH-SHARD) and the remez check cache are out of scope. Other slow lib unit tests are not wired;
  add `crate::verdict::Key::new(..).begin()` / `pass()` to them when they are named.

## Next step
Coordinator: land through the normal train and schedule the nightly `LIELLA_NO_VERDICT_CACHE=1 cargo xtask test`.
Plan rows: D-10.51, D-10.3.
