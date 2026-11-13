# DL-LIBC-START-10 report

Branch `task/dl-libc-start-10`, on DL-LIBC-START-9's head f9200d008. Role keke:~/w/n3 (Zen 1 host).

## Step 0
dl-libc-start-10-lint0 (inherited head f9200d008): `cargo xtask clippy && cargo xtask static`, rc 0.

## Merge (eb99c12df)
`git merge origin/next` (5e5ec2a57): conflicts only in crates/libc-so/hot-text.list and hot-text.unpinned. On both
sides the conflict hunks were the textdomain keys. Next dropped the generic `intl::textdomain` instantiation and
`getopt::next` and kept `catalog::textdomain` and `thread::list_insert`. The branch had dropped `catalog::textdomain`,
`list_insert` and `getopt::next` (-8's dead-key drops on an older next; the branch's code touches none of these
functions). Resolution: hot-text.list, hot-text.unpinned and hot-text.absent are taken from next unchanged (the absent
lines went with the branch's drops). Not regenerated. Ruling #5171's five keys are in neither list (grep).

## Fault found after the merge, fixed (84fb8d645)
Fast profile on the merged head (dl-libc-start-10-fast): hot_text_products 21/23. `init_first_mktime_after_localtime_r_takes_no_fault`
and the mktime rows of `init_zero_fault_calls_hold_at_every_text_phase` failed: the first mktime took 1 fault at every
text phase. origin/next's own fast build in the same host (git archive into ~/work, dl-libc-start-10-nextfast): 0.
Taking next's probe-text.keys alone did not change it (dl-libc-start-10-fast2, still 1).
`perf record -e page-faults -d` of the mktime probe for both builds (dl-libc-start-10-pf2: minflt 1 vs 0, 193 vs 192
faults) showed one difference. Next inlines the time zone's instantiation of the generic `recheck::TscGate::fresh`
into `time_compat::select`. The branch's build emitted it out of line in cold text, and the first mktime faulted on it.
The inliner's choice moved with unrelated code (the same instability -8's trace saw, and ruling #5171 kept the generic
out of the lists). Fix: `#[inline(always)]` on `TscGate::fresh`. probe-text.keys is back to next's: -8's regen line
for the out-of-line copy no longer names a function.

## Results (keke, head 84fb8d645 code)
- dl-libc-start-10-fast3: fast `xtask so` + hot_text_products 23 passed; mktime 0 at every phase; localtime_mktime 19.
- dl-libc-start-10-rel: release `xtask so` + hot_text_products 22 passed (all but the bound test, run separately below).
- dl-libc-start-10-lint2: clippy + static rc 0.
- dl-libc-start-10-plants: 1 plant (`#[inline(always)]` removed), CAUGHT by the mktime test, 0 WARN.
- dl-libc-start-10-gate2: `cargo xtask affected --run --base origin/next` escalated to FULL (as in -9); left
  running for the dispatcher on the final head.

## INFORMATIONAL
- Release profile on this host: `init_short_programs_stay_within_their_page_fault_bounds` gets setlocale + printf
  53 > 51 on this branch AND on origin/next 5e5ec2a57 itself (dl-libc-start-10-nextrel, rc 101, same count). Not this
  branch's change; the gate builds fast products (51, within bound). A layout story should look at next's release count.
- Superseded: dl-libc-start-10-so2 (merged head, before the fix: release 53 as next; fast never ran), -fast, -fast2
  (mktime 1 fault, the cause above), -pf (dash had no `exec -a`), dl-libc-start-10-gate (stopped, restarted on the
  final head).
- One local python heredoc was used to patch a scratch script (not in the tree): noted for the record.
