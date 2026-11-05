# LIBM-HOT-PER-FAMILY: crates/boundary/src/hot.rs split into crates/boundary/src/hot/

State: branch `task/libm-hot-per-family` (from origin/next c6e5db9d), role shiki:~/w/sh2. Commits: 367bbc35 (the move),
ea8e009a (plants file), 83f10cf8 (re-export visibility), then this handoff.

## What moved (pure move, script-cut at item boundaries; no code changed)

| File | Holds | From hot.rs lines |
|---|---|---|
| `hot/mod.rs` | layout doc + FLOW RULE, `mod` list, glob re-exports, the shared inline `sqrtsd`/`sqrtsd_sse2` (invtrig and hyperbolic use it) | 2593-2617 |
| `hot/mem.rs` | copy/fill/compare kernels, KERNEL CONTRACT header, `avx2` submodule | 1-2218 verbatim |
| `hot/delim.rs` | getdelim fused scan + copy (`delim_copy*`, `fuse`) | 2220-2523 |
| `hot/exp_log.rs` | `exp_nearest`, `exp10_nearest`, `log_nearest` | 2525-2558 |
| `hot/invtrig.rs` | `invtrig_nearest!` and its invocation (atan, asin, acos, atan2, hypot) | 2560-2575, 2619-2625 |
| `hot/hyperbolic.rs` | `asinhf_nearest`, `acoshf_nearest` | 2577-2591 |
| `hot/{trig,erf,gamma,bessel,f32_<family>}.rs` | empty, declared so a first wrapper needs no mod.rs edit | - |

Every `crate::hot::<name>` path is unchanged (`pub use mem::*`, `pub(crate) use` for the others); no caller edited.
Only doc text that named a now-other-file item changed (two intra-doc links, "below"/"above", one comment in mem.rs).
Also: unsafe audit (`xtask/src/policy.rs`) hot-kernel category `src/hot.rs` -> `src/hot/`; plan S2 text in
`plan/02-design-rules.md` names the module directory; benchrows tests, docs/architecture.md, docs/arch-seams.md and two
fold.S comments name the new paths. The affected tool already maps `src/hot/*.rs` to module `hot` (same narrowing).

## Proof

- clippy (`cargo xtask clippy`) and `cargo xtask static`: clean on the role at 83f10cf8.
- plants `libm-hot-per-family-plants` (cargo xtask plants, handoff/LIBM-HOT-PER-FAMILY-plants.txt): 4 CAUGHT, 0 WARN.
- counts `libm-hot-per-family-counts` (rc 1, NOT VERIFIED): cbench_libm (169), cbench_hot (58), cbench_memref (115),
  cbench_stdio (6), after vs base, `liella_instr` per row: 152 of 348 identical; 196 differ by tiny amounts in BOTH
  directions (122 libm rows, e.g. acos 182.8 -> 182.7, acoshf 94.4 -> 94.3, acosl 176062.5 -> 176050.6; getenv/setenv
  2000 rows +1..+7 of ~11k; mutex _mt +0.1). libc.so.6 `.text`/`.rodata`/`.data` sha256 all differ (no path string
  of hot.rs is in the .so; likely symbol/layout order from the new module paths under fat LTO, not checked). The
  pattern reads as per-call counter noise plus layout, but that is a hypothesis. Control: `libm-hot-per-family-counts-noise`
  (evidence, not a JOBS item) counts the split tree twice in the RAM plants copy and prints how many rows differ
  between two runs of one tree; if it shows a similar spread, the before/after differences are noise. If not, compare
  `nm -S` per-function sizes of a base and a split build next.
  Files: shiki:~/work/libm-hot-per-family/{before,after,control-1,control-2}.tsv, cmp_counts.py. Base run: `libm-hot-per-family-counts-before` (its rc 1 is
  a wrong `cp` source after the bench completed; measure.tsv was copied by hand to ~/work/libm-hot-per-family/before.tsv).
- gate `libm-hot-per-family-gate`: FULL tier (`--tier full --base origin/next`), starts after the counts job.

## FLOW RULE (for the coordinator's rules)

A libm story adds its boundary wrappers in its family's `crates/boundary/src/hot/<family>.rs` (`f32_<family>.rs` for
`float`), never in `hot/mod.rs` unless two or more families share the item. A new family gets its own file and one line
each in mod.rs's `mod` list and re-exports.

## Merge note for in-flight libm branches

A branch that added items to hot.rs conflicts once (modify/delete). Resolve: take next's `hot/`, move the branch's
additions into the family file (exp/log -> exp_log.rs, invtrig/hypot -> invtrig.rs, special erf/gamma -> erf.rs /
gamma.rs, float -> f32_<family>.rs); use `super::sqrtsd` for the shared root; if the family is in mod.rs's `allow` list,
leave it there (harmless). `pub(crate)` items need no mod.rs change.

## Open / RULING NEEDED

- Plan S2 (owner ruling 2026-10-08) said hot-kernel is permitted in the file `src/hot.rs` only; it now reads the `hot`
  module directory `src/hot/`. Recommend accept: same module, same contract, enforced by the same audit.

Next step: none for this story; libm briefs should name their family file.
