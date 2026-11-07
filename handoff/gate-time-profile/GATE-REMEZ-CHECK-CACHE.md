# GATE-REMEZ-CHECK-CACHE: the remez check runs its fits in parallel and remembers verdicts by content hash

Id: GATE-REMEZ-CHECK-CACHE
Kind: code
Priority: 75
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 tool calls / 160k

## Why
GATE-TIME-PROFILE (handoff/gate-time-profile.md) found that `cargo xtask test` spends ~770 s of every FULL CI gate
in `math::run_remez(root, ["--check", <every crates/math/fits/*.fit>])` (xtask/src/main.rs test_all, D-10.12):
one single-threaded `remez` process checks the 22 fit files one after another on a 12-CPU slot, before the
products and both nextest passes can start. It was ~100 s on 11-05 (2 fits) and grows with every new fit.
Measured per fit (release, one core): fast32_tgamma 241.7 s, fast32_lgamma 167.2, fast32_erfcx2 74.4, fast32_y1
58.2, fast32_y0 57.6, fast32_erfcx1 47.2, fast32_erfcx3 37.1, the rest <= 19 s; sum 772 s.

## The change
1. Parallel: `remez --check` (and the generating mode) handles its fit files concurrently, at most
   `std::thread::available_parallelism()` at once (the affinity mask of the gate's slot); the bound is then the
   longest fit (~242 s), so no scheduling beyond that is needed. Each fit's output lines are buffered and printed in the
   command-line order, so the log reads as today. The exit status is still the failure of any fit. Whether this
   lives in the `remez` binary (std::thread::scope) or in xtask (one process per fit) is yours to decide; record it.
2. Verdict cache: before fitting, `remez --check` computes a content key over EVERYTHING the verdict depends on:
   the bytes of the running `remez` executable (`std::env::current_exe()`), the fit file's bytes, the bytes of the
   coefficient file it names (absent = a distinct key), the arguments, and any other file the run reads (find them
   with one `strace -f -e trace=openat` run of `remez --check` over all fits; list them in the D-row). A key with
   a stamp under `target/remez-check/` is "up to date (cached <key>)" without fitting; a fit that passes writes
   its stamp (atomic rename). A failing fit writes nothing. Generating mode (no --check) never reads the cache.
   `LIELLA_REMEZ_NO_CACHE=1` turns the cache off (for a release or a suspicious run).
   Hash: a 128-bit-or-wider hash from a crate already in Cargo.lock, or a std-only one you justify; one-line
   D-row in plan/07-decisions/ (section 10, next free D-10.n).
3. The same for `tables --check` only if its own time is over ~10 s (it was <= ~20 s inside the measured gap;
   measure it first, report the number either way).

Expected: about -12.8 min per FULL gate when no fit, coefficient file or mathtools code changed (nearly every
train), about -8.8 min when they did (parallel bound). Measure both on your role (cold cache, warm cache) with
the step's own wall time, `/usr/bin/time -v` on `cargo xtask remez --check crates/math/fits/*.fit`, and report
before/after; wall time is the honest unit here (this is a gate-speed change, no libc performance claim).

## Proof that coverage is unchanged
- The check is deterministic: same executable bytes + same inputs = same verdict. Show it: a cold run and a
  warm run print the same "up to date" set; `LIELLA_REMEZ_NO_CACHE=1` gives the same result as the cache.
- Plants through `cargo xtask plants` (docs/plants.md), each must be CAUGHT with a `msg:` regex, run on a WARM
  cache (stamps present from a green run) so they prove the key, not a cold start:
  a. one coefficient digit changed in crates/math/src/coeffs/fast32_y0_reg.rs -> remez --check fails;
  b. one number changed in crates/math/fits/fast32_y0_reg.fit -> fails;
  c. a change in the remez source that alters its output (e.g. one fewer iteration bound) -> the executable
     changes, the check runs and fails;
  d. a mutant key function that leaves out the coefficient file bytes -> caught by a unit test that changes
     each input of the key in turn and expects a new key;
  e. parallel mode drops one fit's failure (mutant ignores the rc of the last thread) -> a fit test fails.
  Use the small fits (y0_reg 5.6 s) so plants stay cheap.
- `cargo xtask affected` already maps fit/coeff/mathtools changes to `remez --check` (XTASK-AFFECTED-MATH-2);
  the direct tier must still select it: show `cargo xtask affected --list` for a fit change before/after.

## JOBS
gate (direct tier, `--base origin/next`), plants (labels above), and one `counts`-style timing job (cold/warm
remez check). Name each label in the hand-back on `JOBS:`.

## Hand-back first line
`COMPLETE: remez check cached + parallel (<cold s> -> <warm s>; serial was <s>)`, or INCOMPLETE: / DO NOT LAND:.
