# LAYOUT-TEXT-PIN-13: start-up +1 found (a data object outside the start-up cluster), two regen blockers fixed (budget checkpoint)

**State (2026-11-11):** branch `task/layout-text-pin-13` = task/layout-text-pin-12 fc5c2c481 + 4108d457d
(startup_data.py) + 10e20c108 (hottext.rs) + this note. Role shiki:~/w/sh2 (Haswell-EP). Scripts on shiki in
~/work/LAYOUT-TEXT-PIN-13/: regen.sh (both regens + padmap head/next), padmap.sh (modes fork true tool locale),
execsplit.c / split2.sh (exec-time vs run-time split of the relayed child), spot.sh. Next reference: the RAM copy
/mnt/zram/claude/LAYOUT-TEXT-PIN-12/next (a551cd8c2, fast, built). origin/next is now e642c8ce4 (not merged: the brief
allows no merge). Decisions in force: as -11/-12 (D-X.63, D-X.56, D-X.47, D-X.49, D-4.53). Reserved, unused: D-X.71.

## Done
1. **Spot-check of the 30 unreferenced keys (#4778):** run_thread_dtors, printf::send, getenv_many are not linked at
   all in page_faults, p_str or p_fmt (dead). `liella_machine_strncmp` names an EMPTY section; the code sits in the
   `.text...strncmp` section of the same object, whose key `strncmp` stays listed. The check is right; elf_refs unchanged.
2. **Start-up +1 (ruling #4777), cause found; it is not a perf-invisible fault.** ptrace split of the relayed vfork
   child (execsplit, LAYOUT-TEXT-PIN-12's padmap binaries, pads 0/0x2000/0x8000, 3 runs each, all identical):
   exec-time faults 2 vs 2; faults to exit head 18 vs next 19 (head 1 better); at the probe's getrusage head 13 vs 12.
   perf's time-ordered maps: before main the head touches `.data` page 0x871 at ip 0x409185 (`runtime::start` reads
   `__libc_single_threaded` with `mov $0x871378,%rax`); next keeps that object in the `.data1` start-up cluster
   (0x8785ea) and takes page 0x871 only after getrusage (`stderr` in the result print). -12's "invisible" reading
   compared totals, where the head is ahead. Kernel tracepoints (`perf stat -e exceptions:page_fault_*`) are not
   available to the claude user (perf_event_paranoid 4); not needed after the split.
   **Cause:** startup_data.py took only RIP-relative `# <addr>` operands of the start-up functions plus perf's first
   touch per page, so an object addressed by an absolute immediate on an already-faulted page never got a key.
   **Fix 4108d457d:** also collect absolute immediates and absolute memory operands (resolve() keeps named data objects only).
   NOT VERIFIED: needs the regen to rewrite startup-data.keys, then padmap (true 12 expected at pad 0).
3. **Regen blocker, fix 10e20c108:** the release regen (job layout-text-pin-13-regen3, rc 1) dropped release from
   `liella_machine_strncmp` but the fast column kept it, and `unreferenced_error` ignored the profile column, so the
   rebuild failed (either order fails). The check now names only keys whose column includes the profile being built.

## Next step (exact)
1. On a synced role: `bash ~/work/LAYOUT-TEXT-PIN-13/regen.sh` as a jobrun job (release regen, fast regen, copies
   crates/libc-so/*.list *.keys to ~/work/LAYOUT-TEXT-PIN-13/out/, prints whether __libc_single_threaded is in
   startup-data.keys, then padmap head and next). Copy out/ back into the worktree, commit ("Regenerate ...").
2. Check from the padmap lines: true <= next at every pad (12 at pad 0), fork <= 6 at every pad (the -12 thread.rs fix),
   tool/locale not above next. If true is still +1, compare `.data1` membership of `__libc_single_threaded` first.
3. Gate `cargo xtask affected --run` (--base origin/next) and plants handoff/LAYOUT-TEXT-PIN-12-plants.txt, plus a
   plant for 10e20c108 (drop the profile filter; the unit test needs a case with a key listed for the other profile only).
   startup_data.py has no direct test; its proof is the padmap/hot_text_products counts.

## Not to redo
The spot-check, the exec/run split, the -12 per-pad matrix and fault maps.
