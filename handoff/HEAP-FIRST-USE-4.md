# HEAP-FIRST-USE-4: continue HEAP-FIRST-USE-3 (the strings fault red, #4243)

State: branch `task/heap-first-use-4` (HEAP-FIRST-USE-3's tip + merge of origin/next e3b89455a + this run), role
keke:~/w/n2. Commits: a4fabadfa (probe p_strlazy), c536f1339 (memchr_rest key, superseded by the next one),
6d8cc4c96 (both lists regenerated, stale absent line removed), then this handoff/report commit.
Status: INCOMPLETE. The release-profile hot-text test is red by 1 fault on two programs (below). Do not land as it is.

## Decisions in force
D-3.20 (amends D-3.12; `records/decisions/D-3/D-3.20.toml`). Rulings #4241 (keep the folding decision lazy, group
the checked rest paths, never loosen the 0-fault bound), #4242, #4243, #4244 (no heredocs). Never commit views.

## What this run found and did
1. Step 0 (`hfu4-step0`, merged head): clippy, codemap, static rc 0. `cargo xtask codemap` changed nothing.
2. #4243, why regen missed `liella_hot_memchr_rest`: p_str's `memcpy(b, s, 40)` takes the lazy folding decision
   (`resolve_fold_move`) before its memchr, so p_str's memchr always runs folded through the direct entry. The
   oracle's memchr runs before any copy. Fix: new probe `tools/hottext/probes/p_strlazy.c`, which makes the oracle's
   calls in the oracle's order. It runs in regen.sh as tier `strings`, right after p_str. `hfu4-regen` traced the
   key in both profiles (`strings fast,release`).
3. The merged head's fast `cargo xtask so` failed its dead-key check (plants baseline `hfu4-plants`): two keys
   are inlined away now (`TscGate::fresh` under time_compat::select, `catalog::variants::each` under intl::resolve).
   origin/next alone builds (`hfu4-counts-base`). The keke regen drops them. Committed regen's full output
   (hot-text.list, startup-text.keys) and removed the now-stale `CtypeFile::wide release` line from
   hot-text.absent (that key left the list, which failed the release rebuild in `hfu4-regen`).
4. The regen also dropped keys keke's traces did not reach: `CtypeFile::wide`, `text::builtin_charset`,
   `fill_erms`, `intl::record_for`. They may depend on the host (locale data, CPU).

## RED, open: release profile, `hfu4-head2` (head 6d8cc4c96, in a copy of the role)
`LIELLA_FULL_PROFILE=1 cargo xtask so` ok; hot_text_products 12/14: locale (setlocale + printf) 56 > 55, coreutils-
shaped tool 59 > 58. The strings test passes there. NOT KNOWN: whether origin/next's release build is already at
56/59 on keke (no base release run), and whether the dropped locale keys (item 4) cause it.
Next step, in order:
1. Release hot_text_products on origin/next on keke (`~/work/HEAP-FIRST-USE-4/base` exists: run
   `LIELLA_FULL_PROFILE=1 cargo xtask so` + the test there). If next is green, run `faultmap.py` on the locale
   program on both trees to find the faulting page, then put the dropped locale keys back by a trace that reaches
   them (a probe path keke misses), not by hand. Never loosen the bounds.
2. Read `hfu4-gate2` (fast, direct tier) and `hfu4-plants2` (7 plants: HFU3's 6 + hot-list-drops-memchr-rest).
3. Head counts: `bash ~/work/HEAP-FIRST-USE-4/head.sh` (release check + meas.sh counts; base counts already in
   `~/work/HEAP-FIRST-USE-4/base/counts.txt`). Timing goes to the next sweep (#4199).

## Results (keke)
- `hfu4-step0` rc 0; `hfu4-counts-base` rc 0 (origin/next); `hfu4-regen` rc 1 (release rebuild: stale absent line).
- `hfu4-plants` rc 2, `hfu4-gate`/`hfu4-head` killed: they ran on c536f1339, whose fast build failed the dead-key check.
- `hfu4-head2` rc 1 (above). `hfu4-gate2`, `hfu4-plants2`: running at hand-back (`~/bin/jobwait hfu4-gate2 hfu4-plants2`).

## Not to redo
The cause of #4243 (item 2). HEAP-FIRST-USE-3's plants were all CAUGHT on its head (mei `hfu3-plants`).
