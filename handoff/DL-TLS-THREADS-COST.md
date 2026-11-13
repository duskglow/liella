# DL-TLS-THREADS-COST: take back the static pthread_create+join cost of DL-TLS-THREADS (fix forward of #5285)

**State (2026-11-13):** branch `task/dl-tls-threads-cost`, stacked on `task/dl-tls-threads-2` (8e7f701ec; DL-TLS-THREADS
not landed yet). Role kanon:~/w/kn1. Commits (all on top of the stack):
- dd28e9ee5 cherry-pick of bbe7c0530 (loader area as the TLS image with address 0; `LOADER_INIT` / `LOADER_RELEASE`).
- 00252bdcc `tls_init` called inside the image branch (address 0) before the thread block's stores; the cache-reuse
  path clears from the TLS block again (no loader word read; the loader's surplus is now cleared on reuse too).
- 9e717dae3 one `sigprocmask` per create: the call that blocks every signal returns the creator's mask (was a query
  plus a change); restored after the clone only when blocking succeeded.
- 395a8a34a pending record `records/pending/DL-TLS-THREADS-COST-1.toml` (D-NEW, series D-27).

## Measurements (wl_percall, static, instructions:u per call, sd 0, kanon; spec ~/work/DL-TLS-THREADS-COST/threads.c,
a copy of DL-TLS-THREADS' thread_create_join case)
| tree | thread_create_join | job |
|---|---|---|
| glibc (dynamic) | 2280.0 | all |
| pre-DL-TLS-THREADS base cdafa3d1c | 35957.2 | dl-tls-threads-cost-percall |
| DL-TLS-THREADS head 1f09927be | 35986.2 | dl-tls-threads-cost-percall-prev |
| + bbe7c0530 (dd28e9ee5) | 35976.2 | dl-tls-threads-cost-percall |
| + tls_init fold (00252bdcc) | 35960.2 | dl-tls-threads-cost-percall2 |
| + one sigprocmask (9e717dae3, final code) | **35940.2** (-17.0 vs base, -46.0 vs DL-TLS-THREADS) | dl-tls-threads-cost-percall3 |

The bbe7c0530 re-apply alone took back 10 of the 29; the remaining loader checks were the separate loader flag on
create and the loader word in the reuse path (fold: -16) and the `tls_release` check on exit (+3, kept: the hook
is the loader's interface, ruling #5286), offset by the merged mask call (-20).
Dynamic row: NOT MEASURED. wl_percall links Liella statically only (no dynamic mode); the dynamic thread TLS
behaviour is covered by `crates/boundary/tests/dl_tls_threads.rs` (in the gate job). On the dynamic path the reuse
clear now also zeroes the loader's area (its size, once per reused mapping), and the mask merge saves one call.

## Hot-text keys (coordinator note #5312)
HOTLIST-TRAIN-REGEN (CONTRIBUTING.md): the lists `hot-text.list`, `startup-text.keys`, `probe-text.keys`,
`startup-data.keys` are train-owned, never committed on a branch. So the lists were regenerated with
`cargo xtask hot-lists` (= tools/hottext/regen.sh . --both, both profiles) into target/hot-lists on the role, not
committed. On dd28e9ee5 (kanon, job dl-tls-threads-cost-hot): tracked lists, fast tool 53, release tool 56; fresh
lists, fast tool 52, release tool 54; `hottext-recheck` dead-key check passed in both profiles. The fresh list
replaces `locale::publish_global` (fast) with `thread::each_block<publish_global closure>`, drops the
`name::candidates` closure, renames `global_locale::setlocale` to `setlocale_locked`, adds two catalog keys (fast).
The gate job dl-tls-threads-cost-gate2 repeats this on the final head (log ~/logs/dl-tls-threads-cost-gate2.log,
fault counts in ~/work/DL-TLS-THREADS-COST/faults-{fast,release}.log). Next's own counts on kanon were not measured.

## Jobs (kanon)
dl-tls-threads-cost-hot (rc 0), -percall, -percall-prev, -percall2, -percall3 (rc 0); dl-tls-threads-cost-gate2
(final code; clippy, static rc 0, hot_text_products red, stopped before `affected --run`) and
dl-tls-threads-cost-plants (rc 2, baseline build refused: see below).
Superseded: dl-tls-threads-cost-gate (static red on the first record: empty `amends`, host name; fixed in 395a8a34a).

## Why this run is INCOMPLETE (budget checkpoint)
- dl-tls-threads-cost-plants: rc 2, no plant ran. This stack is based where DL-TLS-THREADS was cut (63 commits
  behind origin/next) and does NOT have LAYOUT-DEADKEY-WARN: a plain `cargo xtask so` in the plant copy (tracked
  lists, no target/hot-lists) still FAILS on the two dead keys (`locale::publish_global`, the `name::candidates`
  closure) that bbe7c0530 creates.
- dl-tls-threads-cost-gate2 (stopped by me after the failures below; clippy and static rc 0 on the final code):
  with the fresh lists from `cargo xtask hot-lists`, `hot_text_products` fails in BOTH profiles:
  `init_zero_fault_clusters_sit_in_one_window` (CLUSTER-WINDOW: cluster `time` at 0x43bc40..0x48991b fast,
  0x444180..0x489a4b release, straddles a 64 KiB window) and `init_zero_fault_calls_hold_at_every_text_phase`
  (same cluster, pre 0x800). Not established whether the fresh list alone causes it (next with fresh lists not
  run). Logs: kanon ~/work/DL-TLS-THREADS-COST/faults-{fast,release}.log. Tool faults with fresh lists on the final head: fast 51, release 52.
- Next step: merge origin/next into this branch (brief allowance needed: it brings LAYOUT-DEADKEY-WARN), rerun
  `cargo xtask hot-lists`, check CLUSTER-WINDOW on next with fresh lists vs this branch, then the gate and the
  plants file (handoff/DL-TLS-THREADS-COST-plants.txt; countcheck.sh is in kanon ~/work/DL-TLS-THREADS-COST/,
  copy it to the repo or the new host).

## Open / RULING NEEDED
- The hot-text lists: the coordinator's note asks for regen.sh output committed; CONTRIBUTING (HOTLIST-TRAIN-REGEN)
  forbids committing them. Recommend: the train regenerates them when this lands (as it does for every branch).
- The mask merge is outside the story's TLS code but on the same row; it keeps the mask semantics (old mask =
  creator's) and moves the block earlier (the list insertion before the clone now runs with signals blocked).

## Next step
Land after DL-TLS-THREADS. Then DL-16d / DL-19 (they build on `StartInfo::tls_*`).

## Plan rows to read
D-22.10, D-27.20, records/pending/DL-TLS-THREADS-1.toml, records/pending/DL-TLS-THREADS-COST-1.toml.
