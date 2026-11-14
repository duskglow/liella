# HEAP-FIRST-USE-7: merge next's traced layout; release green; fast first-float +1 open

State: branch `task/heap-first-use-7` = HEAP-FIRST-USE-6's tip 838556d6d + 559d48321 (merge of origin/next
f6cab48d7, LAYOUT-TEXT-PIN-15 landed) + b032b6f42 (get_mempolicy through the generic entry) + f32a2c7e2
(plants file) + this note. Role mei:~/w/s4 (Skylake-SP). Status: INCOMPLETE (fast first float 4 > 3), do not land.
Scripts: mei:~/work/HEAP-FIRST-USE-7/ (bounds.sh, fmapfork.sh, float.sh); next's RAM build
/mnt/zram/claude/HEAP-FIRST-USE-7/next-1 (fast products now).

## Decisions in force
D-3.20 (ask the node only on a multi-node policy; lazy folding decision). Next traces every product build and
commits no hot-text list or startup keys (LAYOUT-TRACE-DEFAULT, LT-5, D-X.88 / D-X.92). Never commit views.

## What this run did
1. Step 0 `hfu7-step0` (inherited head): clippy + static rc 0.
2. Merge of origin/next. Next had deleted hot-text.list and startup-text.keys: they stay deleted. regen.sh takes
   next's side; its canonical runner gives each trace LANG and, when named, TZ only, which covers -5's clean env
   and -6's tz= fix. Kept: p_strlazy in the strings tier right after p_str. `hfu7-lint1` rc 0.
3. `hfu7-bounds` (merged head, release): -6's strings red is GONE under the traced default. New red: fork child
   6 > 5 (and the data-phase test's fork rows at cold/lbss/data pads). Next on the same host (`hfu7-next-fork`,
   RAM copy, release): fork 5 at every phase, so it is this branch (it touches start-up code: no jitter waiver).
   Faultmap of the second fork child (`hfu7-fmapfork2`): the extra fault is `liella_sys_set_robust_list`
   (0x40a640). On next `liella_sys_syscall` (0x40e45d) shares the first 64 KiB window with it; on the head it sat
   at 0x412880. CAUSE: next's start-up first runs liella_sys_syscall through the vDSO name compare -> bcmp ->
   resolve_fold_slow (trace key 49); this branch's same_bytes keeps that decision out of start-up (#4170), so the
   first touch moved to the first stdout write (key 80, after the malloc and stdio keys). The fork child returns from
   clone (fork.rs raw::syscall) and exits (exit_group) through that entry.
   FIX b032b6f42: allowed_node_count (decide_topology's one get_mempolicy) goes through raw::syscall, so start-up
   first runs liella_sys_syscall on its first heap slow path, inside the first window.
4. `hfu7-bounds2` (b032b6f42; lint + static + both profiles): lint rc 0; release 26/26 green;
   fast 25/26: `init_first_float_conversion_stays_within_its_page_fault_bound` 4 > FLOAT_BOUND 3.
   `hfu7-float`: next fast reads 3 (same host). Faultmap of the float run: the extra fault is
   `printf::render_plan<ToFd>` at 0x420700. On the head `__snprintf_chk` is at 0x41b3c0, on next at 0x419900
   (+0x1ac0); the printf group now crosses the 0x420000 window. Same mechanism: the start-up group before the
   printf keys is larger or ordered differently on this branch (decide_topology 0x319 bytes and the heap/stdio keys
   moved ahead), and fast has less slack than release.
5. Plants `heap-first-use-7-plants` (handoff/HEAP-FIRST-USE-7-plants.txt, on b032b6f42): -6's file minus the
   memchr list plant (list deleted) plus `mempolicy-through-stub` (release fork bound must catch it). Started
   13:2x, left running for the dispatcher.

## Next step
1. Find which start-up keys put the fast printf group 0x1ac0 later: diff the fast traced lists (s4
   target/product/fast/hot-text.list vs next-1's, as ~/work/HEAP-FIRST-USE-7/cmplists.sh does for release) and the
   sizes up to `__snprintf_chk`. Candidates in order: (a) decide_topology's text (0x319 B, `#[cold]`): if the trace
   places it in the start-up group, shrink it or make the one-time question a small inline check plus a cold tail
   that start-up does not run on one node; (b) the malloc/stdio keys that now precede liella_sys_syscall (order only:
   should not add bytes before snprintf; check). Keep the fork fix (step 3) and recheck release fork at the end.
2. Then: bounds both profiles (bounds.sh), the direct gate (`affected --run --base origin/next`), plants on the
   final head. Never loosen FLOAT_BOUND or FORK_BOUND (ALWAYS FORWARD).

## Not to redo
-6's memchr-rest cluster work (the traced default fixed it). The TZ/env fixes (next's runner has them). The fork
diagnosis (step 3). Counts: HFU5's (Zen 1) stand for the heap change.
Plan rows: D-3.20, D-X.88, D-X.92, #4347.
