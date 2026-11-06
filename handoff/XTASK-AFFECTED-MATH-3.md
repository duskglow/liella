# XTASK-AFFECTED-MATH-3: the math fits and the tables generator map to the math set (continues -2)

State: COMPLETE. Branch `task/xtask-affected-math-3` (from -2's head fabd1bb7, which is based on origin/next c7c320a2).
Code commits: 50f0b63a (-2: the mapping), 9523eaa8 (-3: the shared `ordinary_mapping` helper). After them, handoff
commits only. Rulings in force: #3204 (on top of #3188 / XTASK-AFFECTED-MATH). The -2 handoff
(handoff/XTASK-AFFECTED-MATH-2.md) describes the mapping itself.

## Why -2 went red, and the fix
`xam2-plants` (mei) rc 1: plant `p-tables-ownsuite` SURVIVED. That plant removed `sel.own_suite(repo, f)` from the
MathTables arm. The call added nothing there, because `classify_path` already selects liella-mathtools's unit tests for
`crates/mathtools/src/tables.rs`, and no test could see it go. The other three -2 jobs were green on 50f0b63a (lint 47
unit tests, gate 199 passed, replay 0 misses).
Fix (9523eaa8): the MathTables arm and the `None` arm now call one helper, `ordinary_mapping` (`classify_path` +
`own_suite`). The one `own_suite` site is covered by the AFF-OWNER sweep `the_owning_crates_suite_is_always_selected`
(plant `p-ordinary-ownsuite` CAUGHT by that test). The math test builds its reference with the helper and now also
compares the unit-test crates (`libs`). Selections are unchanged: the replay output is identical to -2's.

## Jobs (shiki, role sh2, on code head 9523eaa8; all rc 0)
* foreground `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
* `xam3-lint`: `cargo xtask static` ok, `cargo test -p xtask -- affected::` 47 passed.
* `xam3-gate`: `cargo xtask affected --base origin/next --run`: 199 passed, 0 failed, SKIPPED 0, peak 981M.
* `xam3-plants`: `cargo xtask plants` (~/work/XTASK-AFFECTED-MATH-3/plants.txt, dir /mnt/zram/claude/XTASK-AFFECTED-MATH-3/plants-1):
  10 CAUGHT, 0 SURVIVED, 0 WARN. Each plant names its expected message.
* `xam3-replay`: the last 10 libm merges (records from -2): Direct 0 misses, 693 test targets vs 1260 wide, 296 vs 619
  programs; Wide 0 misses; 4 FULL.
* Superseded: xam2-lint, xam2-gate, xam2-replay (green, older head 50f0b63a), xam2-plants (rc 1, the survivor above).

## INFORMATIONAL
The 4 replayed stories that still go FULL do so for files outside this story's math set. libm-hot-per-family-2:
csys/src/machine/fold.S, xtask/src/benchrows.rs, policy.rs. libm-kernel-index: xtask/src/kernel_index.rs, main.rs,
math.rs, crates/math/KERNELS.md. libm-tables-per-family-2: crates/mathtools/src/bin/tables.rs, special_tables.rs,
xtask/src/main.rs. libm-f64-fast-kernels-3: crates/boundary/build.rs, crates/boundary/src/*.rs. Only the exact
`crates/mathtools/src/tables.rs` is mapped (rule 8). Whether the generator's siblings (`bin/tables.rs`,
`special_tables.rs`) should map the same way is a possible follow-up story, not done here.

## Next step
None for this story: it lands. Possible follow-up: map `crates/mathtools/src/bin/tables.rs` and
`special_tables.rs` like `tables.rs` (needs a ruling extending #3204).
