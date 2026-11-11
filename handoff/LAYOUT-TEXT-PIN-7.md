# LAYOUT-TEXT-PIN-7 (brief A, continuation 7): gate red fixed in part; the +2 start-up faults are this branch's

**State (2026-11-11):** branch `task/layout-text-pin-7` (= task/layout-text-pin-6 3c5e984d3 + this run's commits);
HEAD = the commit carrying this note. Role shiki:~/w/sh1 (Haswell host). Decision D-X.63 (part 3 on this chain).
Earlier notes: handoff/LAYOUT-TEXT-PIN-6.md, -5 ... LAYOUT-TEXT-PIN.md (not to redo). Plants:
handoff/LAYOUT-TEXT-PIN-7-plants.txt.

## Done this run
1. **Affected-model red (2 xtask tests):** the alias test in xtask/src/hottext.rs named scan.S's
   `liella_machine_fill_avx2_entry` (and fold.S's `liella_machine_memset`) as fixture data; the affected model sends a
   csys label named by a non-crate file to FULL. The fixture now uses fictional names (commit "Name only fictional
   symbols ..."). Neither affected test changed.
2. **s04_classed_memset (the -5 gate red, "flaky"): a real defect, layout-dependent, now fixed.** hottext::group reset
   every grouped section aligned >= 1024 that heads no cluster to 64 bytes (CLUSTER_MARK heuristic). fold.S's classed
   memset section asks for 4096 itself (D-4.53); once it was no cluster head it linked at 0x41c040, the swap's mremap
   could not run, and the program failed with the swap on (both "=0" and PR_SET_MDWE children). so.rs gives group a
   fresh copy of cargo's archive every run, so every alignment there is the object's own: now only cluster heads change
   (`head_alignments`, test `sections_keep_their_own_alignment_unless_they_head_a_cluster`). Job layout-text-pin-7-check
   (before the import fix): libc.a section align 4096, `conformance: 1 passed`.
3. **Fault bounds attributed (coordinator note 2):** same host, fast, hot_text_products:
   | tree | tool (58) | locale (55) | true (17) | fork (7) |
   |---|---|---|---|---|
   | origin/next f1f5358d5 (job layout-text-pin-7-next, /mnt/zram copy) | 56 | 53 | 16 | 7 |
   | this branch, classed fix in (job layout-text-pin-7-check) | **60** | **57** | 15 | 7 |
   | -6 head, release (job layout-text-pin-6-release) | 60 | 57 | 14 | 6 |
   Next reads under its bounds, so this is NOT layout jitter: the +2 (tool and locale, both profiles) comes from this
   branch. Not yet fault-mapped (budget). -5 read 58/55 before the origin/next merge and the #4467 alias regen.

## Open (next step, in order)
1. Fault-map tool and locale on this branch vs origin/next (`perf record -e page-faults -d` on page_faults_liella_tool /
   page_faults_liella, attribute by section; scripts winmap.py/build_probe.sh in ~/work/LAYOUT-TEXT-PIN-6 on shiki).
   Suspects: the -6 merge + regen moved start-up functions (memmove to position 89) and the classes R/D/B, not T (fast
   true went 16 -> 15). Fix by placement, never a bound change (RATCHET).
2. Plants NOT proven (job layout-text-pin-7-plants2, rc 1): ltp7-realign-all's mutant does not compile (`*h != 0`
   on &u64: pick a compiling mutant, e.g. drop the `.filter`); ltp7-real-label SURVIVED (the affected test passed on
   the mutant: check whether the model scans the plant copy or another tree before trusting the plant). Fix both.
3. Then the direct-tier gate (`--base origin/next`) and plants (handoff/LAYOUT-TEXT-PIN-7-plants.txt).
4. Unchanged from -6: STARTUP-TEXT-WINDOWS 4/4 -> 2/2 waits for #4348; regen bootstrap (#4397) for C.

## Not to redo
The fixture rename; the alignment fix; next's counts above (job layout-text-pin-7-next, tree at
/mnt/zram/claude/LAYOUT-TEXT-PIN-7/next on shiki until ram-sweep). Plan rows: D-X.63, D-4.53, D-X.49.
