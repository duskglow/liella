# HEAP-FIRST-USE-6: continue HEAP-FIRST-USE-5 after LAYOUT-TEXT-PIN-11 landed

State: branch `task/heap-first-use-6` = HEAP-FIRST-USE-5's tip 52a50d133 + merge of origin/next c0b379b7e
(ba5151a66) + 648de3d6f (regen.sh: probe time zone through the clean trace) + 0bb812276 (lists regenerated) +
b06ab1948 (plants file) + d0d235cb1 and its revert 73b38a31d + this handoff commit.
Role: mei:~/w/s4 (Skylake-SP). Status: INCOMPLETE, release strings path red (item 4).

## Decisions in force
D-3.20 (amends D-3.12: ask the node only on a multi-node policy; lazy folding decision). LAYOUT-TEXT-PIN's
D-X.63 clusters (`crates/libc-so/hot-text.clusters`, hand-kept). #4318 (regen.sh traces under `env -i`) is kept on
this branch: HOTTEXT-REGEN-HOSTS should take it from here. Never commit views.

## What this run did
1. Step 0 `hfu6-step0` (inherited head): clippy and static rc 0.
2. Merged origin/next (LAYOUT-TEXT-PIN-11; brief allows it). Conflicts: regen.sh (kept `env -i`, next's single
   page_faults build), hot-text.list and startup-text.keys (generated: took next's, then regenerated).
3. Regen `hfu6-regen` (rc 1): the merged regen.sh lost the `TZ=America/New_York` of the p_cold trace (next,
   LAYOUT-TEXT-PIN-3) because `env -i` drops TZ. The traces ran in the host's UTC zone, the time-rule keys
   dropped, and hot-text.absent's zone_path line went stale. FIX 648de3d6f: `tz=<zone> trace ...` passes only that
   zone; the caller's TZ still never reaches a trace. `hfu6-regen2` (fast then release, `--build --allow-drop`)
   rc 0, all 12 layout contracts hold. Drops: only the keys this branch had already removed (start-up folding
   keys resolve_fold_slow, bcmp, __memcmpeq, liella_machine_equal, liella_hot_equal_rest, liella_sys_close;
   record_for, TscGate::fresh).
4. Release bounds `hfu6-release-bounds` (rc 101): the time path is green now (the window-safe `time` cluster
   ended the days_from_civil64 straddle, HFU5's red). New red: first string calls after printf, 1 text fault at
   every text phase (pre/cold pads), so it is not layout jitter. CAUSE: with the lazy folding decision the strings
   mode's first memchr runs `liella_hot_memchr_rest`; p_strlazy first touches it after its printf, so it sits
   after `__printf_chk`, the `strings` cluster's end key, i.e. outside the cluster.
   TRIED AND REVERTED (d0d235cb1, reverted 73b38a31d): ending the cluster at liella_hot_memchr_rest. The
   release cluster grew to 33612 bytes (28 sections, positions 116..=145, with the printf body keys between),
   took a 64 KiB alignment (the ruling #4363 gap), and `hfu6-release-bounds2` was STILL red the same way (rc 101).
   So membership alone is not the answer; not understood yet.
5. Plants (#4321): the two WARN-NO-MSG plants failed at the mutant's `cargo xtask so` because the build's own
   key checks rejected them (an inlined decide_topology or a renamed list key = a dead key). handoff/
   HEAP-FIRST-USE-6-plants.txt retires `topology-decision-inlined` (the time cluster rules out its straddle) and
   deletes the memchr key line instead of renaming it. Not run on a final head (hfu6-plants, hfu6-plants2 killed).

## Jobs (mei)
hfu6-step0 rc 0; hfu6-regen rc 1 (TZ, fixed); hfu6-regen2 rc 0; hfu6-release-bounds rc 101 (strings: the
final head's code, = b06ab1948); hfu6-release-bounds2 rc 101 (reverted try); hfu6-gate, hfu6-plants,
hfu6-plants2 killed by me (they tested a stale or reverted head). Nothing of mine runs in s4.

## Next step (INCOMPLETE: release strings red; do not land)
1. Find why memchr's rest path faults in release, with the faultmap (HFU5 used it: `hfu5-fmapk`): which function
   and address takes the strings fault, and is it memchr_rest at all (the plain cluster extension did not help,
   so it may be another checked entry p_strlazy reaches after printf, or the folding decision's own code).
2. Candidate designs, in order: (a) p_strlazy calls the strings before its printf (the lazy entries then sort
   next to memchr_entry, inside the 24 KiB cluster; needs a regen and a check that page_faults' strings mode
   reaches the same code); (b) a separate small cluster for the checked rest paths placed right after
   `strings`. Keep the strings cluster at 32 KiB alignment or below (#4363). Never loosen the bound.
3. Then the fast gate (affected --base origin/next), plants (the file above), release hot_text_products.
Plan rows: D-3.20, D-X.63, D-X.49.

## Not to redo
The release red of HFU5 (regen LC_CTYPE). The TZ loss (item 3). Extending the strings cluster to the memchr rest key (item 4).
Counts: HFU5's (keke, Zen 1) stand for the heap change; this run changed only layout lists and regen.

NOT VERIFIED (HFU5, still open): hf.c's M variant `exited with 1` under strace.
