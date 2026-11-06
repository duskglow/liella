# XTASK-BENCH-ROWS-BASE: `bench --compare --rows` and `bench --stage --base <ref>`

State: branch `task/xtask-bench-rows-base`, code head 4c023629 (this handoff commit on top). Tooling only, no
library code. Decision row: D-X.33 (`plan/07-decisions/x-implementation-decisions.md`). Docs: `docs/benchmarks.md`
(compare section: `--rows`; staged section: `--base`).

## What changed
- `bench --compare --rows <item>,...`: an item is a row (every compared bench), `<bench>/<row>`, or a glob (`*`, `?`).
  Each bench gets one `CBENCH_ROWS` list, set by `compare_command`, which every build uses (Liella, glibc
  dynamic and static, v3, sorted, and the counters runs). The Liella warm-up round must print every selected row.
  An unknown row is an error that lists the bench's rows, taken from one unfiltered Liella run (that run happens
  only on this error and for globs). Rows printed outside the filter are dropped from the table and named.
- `bench --stage <out> --base <ref>`: after staging `<out>`, xtask checks `<ref>` out as a detached worktree
  under `target/bench-base/<pid>` and runs the same `cargo xtask bench` there without `--base`, staging to
  `<stem>.base.tar`. A drop guard removes and prunes the worktree on success, error and panic. The last line names
  both tarballs and their commits.
- Row names come from the binary, not from `bench/budgets.tsv`: many rows have no budget line (`memchr_16`), and
  macros such as `ROW("memchr", ...)` build the names.

## Proof
- Unit tests (xtask, `no spec clause`): `compare_rows_select_per_bench`, `compare_rows_globs_and_unknown_rows`,
  `compare_rows_reach_both_builds_identically` (a fake bench honouring CBENCH_ROWS),
  `base_run_forwards_the_command_without_base`, `stage_base_stages_two_trees_with_their_commits_and_cleans_up`,
  `stage_base_cleans_up_on_failure` (failing build, a build that stages nothing, an unknown ref). Job
  keke:xtask-bench-rows-base-unit ran them: 12 passed. Its clippy lint (`cloned_ref_to_slice_refs`) is fixed in 4c023629.
- Plants: `handoff/XTASK-BENCH-ROWS-BASE-plants.txt`, job keke:xtask-bench-rows-base-plants rc 0:
  6 plants, 6 CAUGHT, 0 SURVIVED, 0 WARN. The unknown-row check inside `compare()`'s round loop has no unit test,
  because it needs built bench programs. The end-to-end run below covers it.
- Gate keke:xtask-bench-rows-base-gate (`cargo xtask clippy && cargo xtask static && cargo xtask affected --run --base
  origin/next`): clippy and static passed. `affected` ESCALATED TO FULL by its own rule ("xtask: its commands are
  part of the gate"). It was still running at hand-back; re-attach with `~/bin/jobwait xtask-bench-rows-base-gate` on keke.
- End-to-end run, job keke:xtask-bench-rows-base-e2e rc 0. It ran in a fresh git clone under ~/work/xtask-bench-rows-base,
  because a synced role directory has no git history:
  ```
  cargo xtask bench --stage ~/work/xtask-bench-rows-base/s.tar --base origin/next --only cbench_memref
  bench: --base origin/next: staging 8357b11d... from the temporary worktree .../e2e/target/bench-base/1282483
  bench: staged two trees: current .../s.tar (commit 4c023629...); base .../s.base.tar (commit 8357b11d..., from --base origin/next)
  == after: target/bench-base holds: 0 entries; worktrees: (only the clone itself)
  ./xtask-bin bench --compare --no-build --runs 1 cbench_memref --rows memchr_16,memchr_nope
  xtask bench: bench --compare --rows: `memchr_nope` is not a row of cbench_memref; its rows (the Liella build, unfiltered): strstr_miss_4k, ...   (rc 1)
  ./xtask-bin bench --compare --no-build --counters --runs 3 cbench_memref --rows memchr_16,memchr_16_mix   (current tree)
  | `memchr_16` | 4.72 (4.02-4.73) | 5.52 (5.52-5.52) | 1.17 | - | 25.9 | 28.6 | 0.91 |
  | `memchr_16_mix` | 5.12 (4.43-5.20) | 6.40 (6.30-6.59) | 1.25 | - | 32.4 | 35.4 | 0.92 |
  (base tree, same command with the current tree's xtask-bin)
  | `memchr_16` | 4.72 (4.01-4.73) | 5.52 (4.69-5.52) | 1.17 | - | 25.8 | 28.6 | 0.90 |
  | `memchr_16_mix` | 5.12 (4.35-5.91) | 6.30 (6.30-6.50) | 1.23 | - | 32.4 | 35.4 | 0.92 |
  ```
  These figures come from an unbooked build host and show only that the tool works. They are not a
  performance claim. The two trees have the same library sources (tree hash 90c801cc in both), so equal counts are expected.

## Open / informational
- The base tarball's `xtask-bin` is the base ref's binary. A ref older than this change has no `--rows`, so measure
  both unpacked trees with the CURRENT tree's `xtask-bin` (`cd base && ../cur/xtask-bin bench --compare ...`). The
  root is found from the working directory. Option: `--base` could stage the current xtask-bin into the base
  tarball as well, so both trees use one harness. Recommendation: do it as a follow-up if stories keep needing it.
- `--base` builds the base from scratch in its own `target/` (about 4 min of the 8 min e2e on keke for `--only cbench_memref`).
- `affected` escalates every xtask change to FULL, even at the direct tier. So a pure-tooling story always runs the
  full gate.

## Next step
The dispatcher reads keke:xtask-bench-rows-base-gate (FULL `cargo xtask test`). If it is green, the branch is ready to land.
