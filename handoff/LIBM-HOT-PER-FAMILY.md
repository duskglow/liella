# LIBM-HOT-PER-FAMILY: crates/boundary/src/hot.rs split into crates/boundary/src/hot/

State: branch `task/libm-hot-per-family-2` (LIBM-HOT-PER-FAMILY-2; carries the first run's commits 367bbc35, ea8e009a,
83f10cf8, 80ea5419 from origin/next c6e5db9d), then cad174bf (merge of origin/next 5ac80194), e7468d77 (plants file),
then this handoff. Role keke:~/w/n3. Ruling #3129 accepted: plan S2's hot-kernel location is the module directory `src/hot/`.

## What moved (pure move, script-cut at item boundaries; no code changed)

| File | Holds |
|---|---|
| `hot/mod.rs` | layout doc + FLOW RULE, `mod` list, glob re-exports, the shared inline `sqrtsd`/`sqrtsd_sse2` (invtrig and hyperbolic use it) |
| `hot/mem.rs` | copy/fill/compare kernels, KERNEL CONTRACT header, `avx2` submodule |
| `hot/delim.rs` | getdelim fused scan + copy (`delim_copy*`, `fuse`) |
| `hot/exp_log.rs` | `exp_nearest`, `exp10_nearest`, `log_nearest`, and (merged from next) `exp2_nearest`, `log2_nearest`, `log10_nearest`, `pow_nearest` |
| `hot/invtrig.rs` | `invtrig_nearest!` and its invocation (atan, asin, acos, atan2, hypot) |
| `hot/hyperbolic.rs` | `asinhf_nearest`, `acoshf_nearest` |
| `hot/{trig,erf,gamma,bessel,f32_<family>}.rs` | empty, declared so a first wrapper needs no mod.rs edit |

Every `crate::hot::<name>` path is unchanged (`pub use mem::*`, `pub(crate) use` for the others); no caller edited.
Also: unsafe audit (`xtask/src/policy.rs`) hot-kernel category `src/hot.rs` -> `src/hot/`; plan S2 text; benchrows
tests, docs/architecture.md, docs/arch-seams.md, two fold.S comments name the new paths.

## The CI conflict (run 2) and its resolution

CI's merge mapped next's `hot.rs` onto `hot/mem.rs` by rename detection (`-X no-renames` did not stop it on git 2.43).
Since base c6e5db9d next changed hot.rs in ONE way: +46 lines after `log_nearest` (the exp2/log2/log10/pow FMA
dispatchers, LIBM-F64-EXPLOGPOW-TIER). Next's hot.rs lines 1-2218 equal mem.rs apart from this branch's own comment.
Resolved by hand: mem.rs = the branch's version, the 46 lines appended to `hot/exp_log.rs`; `hot.rs` stays deleted.
Pure-move check (`pure_move.py`, scratch; next's hot.rs vs all of `src/hot/*.rs`): items 122 -> 138, 0 missing (the 16
extra are the `mod` lines); UNSAFE comment lines 301 -> 301, 0 missing, 0 extra; code lines 0 missing (extra only the
mod/use plumbing and two `use super::sqrtsd`). The coordinator's list (getdelim, erf/erfc, f32 hyperbolic/invtrig) was
already in base c6e5db9d and so already in the split.

## Proof (final code head e7468d77; the later commit is this handoff only)

- `cargo xtask clippy`, `cargo xtask static`: rc 0 on keke:~/w/n3. `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`: rc 0 (RAM plants copy, same source verified with diff -rq).
- plants `libm-hot-per-family-2-plants` (cargo xtask plants, handoff/LIBM-HOT-PER-FAMILY-2-plants.txt, dir
  /dev/shm/claude/libm-hot-per-family-2/plants-1): 6 CAUGHT, 0 SURVIVED, 0 WARN (the 4 of run 1 + exp2 and pow
  dispatchers in exp_log.rs).
- gate `libm-hot-per-family-2-gate`: FULL tier (`--tier full --base origin/next`). Result: see the report json.
- counts `libm-hot-per-family-2-counts` (keke:~/work/libm-hot-per-family-2/counts.sh + noise_cmp.py): origin/next tree
  once, the split tree twice (RAM copies), cbench_libm/hot/memref/stdio; fails if any row's base-vs-split difference in
  `liella_instr` exceeds max(0.1%, 2 x the largest same-tree difference). Per-call counts are not bit-stable run to run
  (run 1: two runs of ONE tree left 140 of 348 rows identical; base vs split 152), so exact equality is no test.
  On run 1's data this rule gives max same-tree 0.99%, max base-vs-split 0.61%, mean signed shift +0.010%: OK.

## FLOW RULE (for the coordinator's rules)

A libm story adds its boundary wrappers in its family's `crates/boundary/src/hot/<family>.rs` (`f32_<family>.rs` for
`float`), never in `hot/mod.rs` unless two or more families share the item. A new family gets its own file and one line
each in mod.rs's `mod` list and re-exports.

## Merge note for in-flight libm branches

A branch that added items to hot.rs conflicts once (git maps hot.rs onto hot/mem.rs). Resolve: take this branch's
`hot/` as is, move the branch's additions into the family file (exp/log -> exp_log.rs, invtrig/hypot -> invtrig.rs,
special erf/gamma -> erf.rs / gamma.rs, float -> f32_<family>.rs); use `super::sqrtsd` for the shared root; delete
hot.rs. Check: every item and UNSAFE line of the branch's hot.rs appears once under src/hot/.

Open: none. Next step: none for this story (land it; the held libm stories rebase their hot.rs additions per the note).
