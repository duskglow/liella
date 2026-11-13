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
- 3c174a479 then 250543bc2: `cargo xtask static`'s hottext check (LAYOUT-HOTKEY-INLINE-STABLE) refused three keys
  of the new list that name generics without a fixed boundary (`thread::each_block` for publish_global,
  `catalog::variants::each`, `intl::record_for`), and three `hot-text.unpinned` lines whose keys left the list
  (removed; the file is a remove-only ratchet, so the new keys could not be listed there). First try,
  `#[inline(always)]` (3c174a479): a second regen then keyed five OTHER generics (find_cached, Files::locpath,
  the candidates closure, attach_charmap, passwd_cached) because inlining shifted: not chased (regen loop). Final,
  `#[inline(never)]` (250543bc2): the fast code the edcf064a3 list was traced from is unchanged (the three were
  out of line there), so the list stays as committed; the release profile gains three calls (gettext rows below).
The code commits of run 1 are unchanged (dd28e9ee5 loader area as TLS image, 00252bdcc tls_init fold, 9e717dae3
one sigprocmask per create; record D-NEW in records/pending/DL-TLS-THREADS-COST-1.toml, series D-27).

## Why run 1's gate ended rc 124
dl-tls-threads-cost-gate2 was stopped by run 1 itself after hot_text_products went red (CLUSTER-WINDOW with
uncommitted `cargo xtask hot-lists` output). With the lists regenerated in place and committed, that test passes.

## Measurements (kanon)
Per call (wl_percall, static release products, instructions:u, sd 0; head 250543bc2, jobs
dl-tls-threads-cost-2-percall3 and -percall-gettext; edcf064a3 gave the same thread row in -percall2):
| case | glibc | origin/next e30374455 | this head |
|---|---|---|---|
| thread_create_join | 2280.0 | 35957.2 | **35940.2** (-17.0) |
| gettext_language_steady | 472.0 | 637.0 | 637.0 |
| gettext_found | 461.0 | 517.0 | 515.0 |
| ngettext_found | 606.0 | 648.0 | 646.0 |
| gettext_language_alternating | 2303.8 | 1245.5 | 1245.5 |

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
On head 250543bc2 (later commits: handoff/report only):
- dl-tls-threads-cost-2-gate2: clippy, static, both fault profiles (same counts as the table: 51/48 fast, 52/49
  release, 24 ok each), then direct tier `affected --run --base origin/next`: rc 0, 32 suites ok, 11712 passed, 0 failed (hot_text_products
  among them); conformance 127 passed; peak 16G (page cache), oom_kill 0.
- dl-tls-threads-cost-2-plants2 rc 0: handoff/DL-TLS-THREADS-COST-plants.txt, 4 CAUGHT, 0 SURVIVED, 0 WARN.
- dl-tls-threads-cost-2-percall3, dl-tls-threads-cost-2-percall-gettext rc 0 (table above).
Evidence on earlier heads: -regen2 rc 0 (the regen + faults of edcf064a3), -nextfaults2 rc 0 (next's counts).
Superseded: -lint (static private-paths red, fixed in 700dd7a7f; its clippy passed), -lint2 (static on
700dd7a7f), -regen (refused to drop two keys; rerun with --allow-drop), -nextfaults (killed: its script was
overwritten while it ran), -percall (base 38dade3c1 not primed), -gate, -plants, -percall2 (green, on edcf064a3,
before the hottext static red), -regen3 (the inline(always) attempt: static hottext red; also a
tools/hottext/__pycache__ .pyc left in the role tripped private-paths).

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
