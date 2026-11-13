# DL-TLS-THREADS-COST: take back the static pthread_create+join cost of DL-TLS-THREADS (fix forward of #5285)

**State (2026-11-13, run -2):** branch `task/dl-tls-threads-cost-2` (continues `task/dl-tls-threads-cost`), role
kanon:~/w/kn1 (Broadwell-EP Xeon, E5-2699 v4). The stack now has origin/next (e30374455, ruling #5326: brings
LAYOUT-DEADKEY-WARN) and the base `task/dl-tls-threads-2` (38dade3c1) merged in. Commits of this run:
- 9cf010f42 merge origin/next; 538b4bb5d merge origin/task/dl-tls-threads-2 (rename detection had moved this story's
  pending record into records/decisions/D-27/; it stays at `records/pending/DL-TLS-THREADS-COST-1.toml`).
- 0633d592d CONTRIBUTING: the hot lists are regenerated and committed by the branch whose change moves hot
  functions (ruling #5325) while the train regen is paused (`ci_train_hot_lists=0`), until LT-5 deletes them.
- 700dd7a7f count check in the tree: spec `validation/percall/thread_create.c`, script
  `handoff/DL-TLS-THREADS-COST-countcheck.sh` (the plants file named home paths: `cargo xtask static`
  private-paths red, the only red step 0 found; clippy was clean).
- edcf064a3 `crates/libc-so/hot-text.list` regenerated (`tools/hottext/regen.sh . --both --allow-drop`; dropped:
  the `name::candidates` closure the loader-area change folds away and `iconv::Conv::to_words`, neither hit by
  any trace; the other three lists unchanged). Its message says "Skylake-SP"; the host is Broadwell-EP (correction).
The code commits of run 1 are unchanged (dd28e9ee5 loader area as TLS image, 00252bdcc tls_init fold, 9e717dae3
one sigprocmask per create; record D-NEW in records/pending/DL-TLS-THREADS-COST-1.toml, series D-27).

## Why run 1's gate ended rc 124
dl-tls-threads-cost-gate2 was stopped by run 1 itself after hot_text_products went red (CLUSTER-WINDOW with
uncommitted `cargo xtask hot-lists` output). With the lists regenerated in place and committed, that test passes.

## Measurements (kanon)
Per call (wl_percall, static, instructions:u, sd 0; job dl-tls-threads-cost-2-percall2, head edcf064a3):
| case | glibc | origin/next e30374455 | this head |
|---|---|---|---|
| thread_create_join | 2280.0 | 35957.2 | **35940.2** (-17.0) |

Start-up minor faults, `cargo test -p liella --test hot_text_products` (all 24 tests; job -regen2 for the branch,
-nextfaults2 for an origin/next tree with its tracked lists, copied to ~/work/DL-TLS-THREADS-COST-2/next):
| profile | tree | tool | setlocale+printf | startup | fork | tests |
|---|---|---|---|---|---|---|
| fast | next | 53 | 50 | 11 | 5 | 24 ok |
| fast | branch | **51** | **48** | 11 | 5 | 24 ok |
| release | next | 55 | 52 (bound 51) | 10 | 5 | 23 ok, 1 FAILED |
| release | branch | **52** | **49** | 10 | 5 | 24 ok |
origin/next's own release profile is over the setlocale+printf bound on this host with its tracked lists
(INFORMATIONAL; logs ~/work/DL-TLS-THREADS-COST-2/faults-next-release.log).

## Jobs (kanon)
On head edcf064a3 (the later commits are handoff/report only):
- dl-tls-threads-cost-2-gate rc 0: direct tier, `affected --run --base origin/next`: 32 suites ok, 11712 passed,
  0 failed (hot_text_products among them); conformance 127 passed; peak 16G (cache), oom_kill 0.
- dl-tls-threads-cost-2-plants rc 0: handoff/DL-TLS-THREADS-COST-plants.txt, 4 CAUGHT, 0 SURVIVED, 0 WARN.
- dl-tls-threads-cost-2-percall2 rc 0 (table above).
- dl-tls-threads-cost-2-regen2 rc 0 (regen + faults), dl-tls-threads-cost-2-nextfaults2 rc 0 (next's counts).
- dl-tls-threads-cost-2-lint2 rc 0 (static on 700dd7a7f).
Superseded: -lint (static private-paths red, fixed in 700dd7a7f; its clippy passed), -regen (regen refused to drop
two keys, rerun with --allow-drop), -nextfaults (killed: its script was overwritten while it ran), -percall (base
38dade3c1 not primed; rerun with next's sha).

## Open
- Dynamic thread_create_join row: not measured (wl_percall links Liella statically only); the dynamic thread TLS
  behaviour is covered by crates/boundary/tests/dl_tls_threads.rs.
- The mask merge (9e717dae3) is outside the TLS code but on the same row; mask semantics unchanged.

## Not to redo
The regen with --allow-drop (both keys verified untraced); next's fault counts above; the per-call row.

## Next step
Land after DL-TLS-THREADS (-2). Then DL-16d / DL-19 (they build on `StartInfo::tls_*`).

## Plan rows to read
D-22.10, D-27.20, D-27.36 (DL-TLS-THREADS), records/pending/DL-TLS-THREADS-COST-1.toml.
