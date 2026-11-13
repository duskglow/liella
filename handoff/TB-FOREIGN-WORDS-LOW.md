# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-6` (stacks on -5; next merged at 3dc8d485a, merge base 68e72ab32). Code head
1a2693c4b; HEAD = the handoff commit after it. Decisions in force: D-X.58, D-X.60, rulings #4835, #4849, #4851,
#4779 (<= +2 per access vs next), #4889 (judge on Release), #5325, #5378 (no full regeneration committed on this
branch). Older reports: handoff/TB-FOREIGN-WORDS-LOW-report.md (-1..-4), git history of this file (-5).

## What -6 did
- **ca7208ce9**: hot-text.list, startup-text.keys, probe-text.keys, startup-data.keys back to origin/next's
  versions (unchanged between the merge base and next's tip), per #5378. The pin on locale::name::candidates
  (cebb4c756) existed only for a regen key and is undone (name.rs = next's). hot-text.absent (07c81c2b3) and
  startup-state.list (75181c245) keep this chain's own edits.
- **1a2693c4b**: two hand-added keys (the list says "generated", but #5378 rules out the regen; the brief asks
  for exactly the keys a moved function needs):
  - `thread::after_fork_child` (fork tier) + `#[inline(never)]`. A perf minor-fault map of the second fork child
    (fast products, no randomisation) put fault 7 on after_fork_child alone at 0x731000; the rest of the child sat
    in the hot group. This chain's D-X.58 body change (owned()) left it out of line, outside the keyed
    fork::child_reset. -5's regen produced the same key. Fork child 7 -> 6 (bound 6).
  - `liella_machine_errno_location` (tool tier) in place of next's `__errno_location`, which is now a label of
    that assembly section (machine/tp.S) and was a dead key (so warning).

## Results (keke:~/w/n2)
- lint0 (inherited 2c065b6f5): clippy green, static RED (hottext check_pinned: 18 unpinned regen keys). Fixed by ca7208ce9.
- step2 (ca7208ce9): clippy + static GREEN; hot_text_products (fast) 22/24: fork child 7 > 6, data clusters.
- step2b (1a2693c4b): clippy + static GREEN; hot_text_products (fast) 23/24: `init_data_clusters_hold_at_every_text_phase`
  fails only on "tool: fault count moves with the text phase": 53 at `cold 0x800`, 52 at the other 9 pads.
  Fork child 6 at every pad. heap_meta_products did not run in that job (script stopped at the failure); it passed
  in the release job.
- nextphase (merge base 68e72ab32, fast, RAM copy): same test passes; tool 53 at all 10 pads, fork 5, true 11.
  So the branch's tool count is <= next's at every pad (52-53 vs 53); fork child is +1 over next (6 vs 5, bound 6).
- rel (1a2693c4b, LIELLA_FULL_PROFILE=1 products): hot_text_products 24/24 and heap_meta_products 2/2 GREEN.
- plants (1a2693c4b, handoff/TB-FOREIGN-WORDS-LOW-6-plants.txt): 4 plants, 0 SURVIVED. errno-foreign-block and
  slow-path-own-copy CAUGHT with their message. errno-owned-offset and errno-owned-to-registry: WARN-NO-MSG, both
  caught by s2_1_c_0 through its first step, the conformance program s01_startup on the mutant products (exit
  status 71). The errno_owned.c check after it never runs, so the message the plants file names was absent. A real
  catch, not a build failure. Next run: msg `s01_startup: status [0-9]+, expected 0|errno in a started process is not tp \+ 0x80`.
- percall2 (1a2693c4b, Release, base 68e72ab32): errno_location 9.0 -> 11.0 (+2, #4779 limit), malloc_free 115.2
  -> 115.2, calloc_free 162.9 -> 162.9 (glibc 10.0 / 144.1 / 281.4).
- NOT RUN: the FULL tier. It would be red on the fast data-phase test above.

## Open question (RULING NEEDED)
The fast data-phase test fails on constancy, not on a bound: the branch reads 52 at 9 pads and 53 at one, next
reads 53 at all. That looks like the layout jitter in #4347 (one data object sharing or not sharing a page as the
data classes move). The `cold` pads move only the data classes. The chain added `thread::SPARE` (one
BLOCK_SIZE, zero, address only in a started process) and the owned bytes (in .data1 already). Recommendation:
treat it as LAYOUT-JITTER (the branch is at or below next's tool count at every pad), or have a continuation map the
tool run at cold 0x800 vs 0x2000 (below).

## Next step
1. If not jitter: map `page_faults tool` faults at the `cold 0x800` and `cold 0x2000` pads. The test deletes its
   padded links, and a kept TMPDIR did not keep them (toolmap job, ~/work/TB-FOREIGN-WORDS-LOW-6/toolmap.sh),
   so build the padded crt1 the way `padded_crt1` in hot_text_products.rs does and run tools/hottext/faultmap.py
   on both. The extra page names the object to declare in startup-state.list (hand-written) if it is this chain's.
2. Then the FULL tier (`cargo xtask affected --run --base origin/next --tier full`) and the plants rerun with the msg above.
3. Fork child +1 over next (6 vs 5): optional, map it the same way (~/work/TB-FOREIGN-WORDS-LOW-6/forkmap.sh).

## Not to redo
-1's 0x80-0x9b audit; the registry/FOREIGN/OwnPages/asm design; the heap slow-path KernelPages fix (-5); no
full regeneration of the lists (#5378); the fork map above (after_fork_child was the 7th fault).
