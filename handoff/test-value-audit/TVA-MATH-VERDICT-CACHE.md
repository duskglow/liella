# TVA-MATH-VERDICT-CACHE: a math accuracy sweep shard runs only when its inputs changed since a green verdict

Id: TVA-MATH-VERDICT-CACHE
Kind: code
Priority: 70
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 90 tool calls / 180k

**Runs only after the owner rules (RULING NEEDED in TEST-VALUE-AUDIT's hand-back): this changes when gate
coverage runs.**

## Why
TEST-VALUE-AUDIT (handoff/test-value-audit.md) measured that pass 1 of every FULL CI gate spends ~9,000 of its
~12,000 test-s in liella-math accuracy sweeps: the C-oracle sweeps complex_ld_cr_products, complex_f128_products,
special_ld_products, special_f128_products, complex_products, elem_ld_products, special_products, and the
pure-Rust sweeps special_cr, fast_gamma and the slow liella-math lib sweeps (fast32 inverse_trig 184 s). In 328
CI gates none of them failed; in developer logs they failed only in the stories that wrote their functions.
They prove correct rounding (D-10.3) and are worth running whenever what they check changes, which most trains
do not touch. Setup is 1-4% of a shard (measured: complex_ld_cr shard 0 2.8 s setup / 75.6 s check), so the
gain is in skipping the check, not in sharing setup. Estimated -4 to -6 min per train that changes no math
input (pass 1 falls toward its ~400 s privileged chain from ~742 s), 0 on trains that do. NOT VERIFIED.
Out of scope: pass 2 (GATE-PASS2-MATH-SHARD), the remez check (GATE-REMEZ-CHECK-CACHE; reuse its cache layout
if it has landed).

## The change
1. A support module (crates/core/tests/support/, next to oracle.rs) gives `verdict::cached(name, key) -> bool`
   and `verdict::record(name, key)`, a file per shard under target/test-verdicts/, written only after the shard
   passed (written to a temp file and renamed). The key is the hash of:
   - C-oracle sweeps: the bytes of both built programs (the static Liella program holds every Liella byte it
     runs), the host's libm.so.6 and libc.so.6 the reference program loads (resolved paths), the test binary's own
     bytes (`std::env::current_exe`: the checker, mathtools oracle included), the shard index and count;
   - pure-Rust sweeps: the test binary's bytes and the shard;
   - always: the CPU feature flags the dispatch reads (`/proc/cpuinfo` flags line) and the kernel release.
2. A shard whose key matches prints `verdict cached: <name> <key>` (so the log says it did not run) and returns;
   otherwise it runs as today and records on success. `LIELLA_NO_VERDICT_CACHE=1` runs everything (the owner's
   override, and the nightly / release mode: say in the PR where it is set).
3. A failing shard removes its record. A record is never shared between hosts (target/ is per tree).
4. A decision row (plan/07-decisions/, D-10.x next free) states the key and why it is complete.

## Proof that coverage is unchanged (every plant still caught)
Plants through `cargo xtask plants` (docs/plants.md), each with `msg:`:
- a libm fault reached by complex_ld_cr (e.g. one coefficient of a long double complex kernel), run after a
  green cached run: the key misses and the shard FAILS (`not correctly rounded` / `special value`);
- the same for one pure-Rust sweep (special_cr j0) and fast_gamma;
- key plants: drop the host libm from the key, drop current_exe, drop the CPU flags: each caught by a unit test
  of the key builder that changes that input and expects a different key;
- record on failure (record before the check): caught by a test that fails a shard and expects no record.
Also run every listed sweep twice on an unchanged tree: the second run prints `verdict cached` for every shard
and takes seconds; then touch nothing but a libm source used by one family and show only that family reruns.
Optional later step (not this story): libm mutation plants against the full sweeps, to see whether a smaller
grid would catch the same faults (the evidence a SHRINK needs; none exists today).

## JOBS
gate (direct tier, `cargo xtask affected --run --base origin/next`), plants, and one timing job of the listed
sweeps run twice (cold, cached). Name each label on `JOBS:`.

## Hand-back first line
`COMPLETE: math sweep verdict cache, <n> shards cached on an unchanged tree, plants <k>/<k> CAUGHT` (or
INCOMPLETE: / DO NOT LAND:).
