# GATE-REMEZ-CHECK-CACHE: parallel `remez --check` with a content-keyed verdict cache

State: branch `task/gate-remez-check-cache` (from origin/next acc8cfcd5); head: see `git log -1` / the report json.
Decision in force: **D-10.54** (plan/07-decisions/s10-mathematics.md), on top of D-10.12 / D-10.3.

## What changed
- `crates/mathtools/src/bin/remez.rs`: fits run on up to `available_parallelism()` threads (`std::thread::scope`,
  shared index); each fit's stdout/stderr is buffered and printed in command-line order; exit 1 if any fit fails or
  a fit goes unreported. Parallelism lives in the binary (one process, the process image hashed once), not xtask.
- `crates/mathtools/src/check_cache.rs`: std-only SHA-256 (FIPS 180-4), `check_key` over tagged, length-prefixed
  fields (process image = `current_exe` + every file in `/proc/self/maps` with its digest; mode; fit argument;
  display path; fit bytes; output path; coefficient bytes or "absent"), and the stamp store
  `<target>/remez-check/<key>` (stamp = header line + the fit's report; temp name + rename). Only `--check` uses it;
  a failing fit stores nothing; `LIELLA_REMEZ_NO_CACHE=1` turns it off; `LIELLA_REMEZ_CACHE_DIR` moves it (tests).
- `crates/mathtools/tests/check_cache.rs`: FIPS vectors, key-per-input test, the binary on three small fits
  (order, cold/warm/uncached equality, failing last and first fit, no stamp for a failure, generate never stamps).
- `xtask` unchanged: `math::run_remez` still runs one `remez --check <all fits>`.

## Measurements (keke, 7-CPU queue slot, host load 22-35; `/usr/bin/time -v cargo xtask remez --check crates/math/fits/*.fit`)
| run | wall | CPU | job |
|---|---|---|---|
| serial (origin/next remez, git-archive export) | 1261 s (21:01) | 107% | grcc-serial |
| parallel, cold cache | 488 s (8:08) | 293-297% | grcc-after-1 (and grcc-before, which raced and also ran the new code) |
| parallel, uncached (`LIELLA_REMEZ_NO_CACHE=1`, under strace) | 444 s | | grcc-after-1 |
| warm cache | 0.19 s | | grcc-after-1 |
| `tables --check` | 5.7 s | 100% | grcc-before, grcc-after-1 (under ~10 s: not cached) |

The cold bound is the longest fit (fast32_tgamma); 22/22 up to date in every run; cold, warm and uncached printed
byte-identical reports and verdict lines (`cmp` after removing the `(cached <key>)` suffix).
strace -f -e trace=openat (uncached, all fits) opened: the 22 fits, the 22 coefficient files, /etc/ld.so.cache,
libc.so.6, libgcc_s.so.1, /proc/self/maps, /proc/self/cgroup + cgroup cpu.max (worker count), /proc/sys/vm/overcommit_memory.

## Proofs
- Plants: handoff/GATE-REMEZ-CHECK-CACHE-plants.txt (a coefficient digit, a fit number, remez iteration step, key
  without coefficient bytes, parallel driver ignoring the last fit's rc). a-c use `cargo xtask remez --check
  crates/math/fits/fast32_y0_reg.fit`, whose unmutated baseline run stamps the copy's cache, so they run warm.
- `cargo xtask affected --list --files crates/math/fits/fast32_y0_reg.fit` selects
  `xtask: cargo xtask remez --check crates/math/fits/fast32_y0_reg.fit` and `tables --check` both on origin/next and
  on this branch (xtask/affected.rs unchanged).
- Gate (keke n1, direct tier, `--base origin/next`, job grcc-gate, code head f1021777e): 10 suites ok, 7689 passed,
  0 failed, 680 ignored; conformance 125 passed; peak 12G, oom_kill=0, rc 0. (lib.rs is a crate root, so the direct
  tier went wide: 240 of 241 targets.)
- Plants (job grcc-plants-2, `cargo xtask plants`, dir /dev/shm/claude/GATE-REMEZ-CHECK-CACHE/plants-2): 5 CAUGHT,
  0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (WARN-DEV-SHM only). Logs show stamp d84c45fb... present before a, b, c
  and each re-fitted (key changed). grcc-plants (run 1) was REFUSED by the tool: it copied the role while the gate
  wrote conformance/build/ there; run 2 copies a RAM snapshot without target/ and conformance/build/.

## Open / informational
- `cargo xtask affected` does not select `remez --check` for a change under `crates/mathtools/src/` (this branch's own
  list has only fuzz-check and static as xtask commands); the brief assumed it did. This branch's full check was run
  by grcc-after-1 instead (22/22 up to date, cold, warm and uncached). Such a mapping costs a full parallel re-fit
  (~8 min on a 7-CPU slot) for every change to the remez binary, because the key covers the executable.
  Recommendation: in a follow-up story, map `crates/mathtools/src/{remez.rs,bin/remez.rs,check_cache.rs,mp/}` to
  `remez --check <all fits>`.
- Next step: none required; the FULL gate's `cargo xtask test` picks this up unchanged.
