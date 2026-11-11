# LAYOUT-TEXT-PIN-12: start-up +1 mapped (partly), fork child fix written, unreferenced-key check (budget checkpoint)

**State (2026-11-11):** branch `task/layout-text-pin-12` = task/layout-text-pin-11 8fbc53655 (in CI, not landed) +
4c33d8c6c (code) + this note. Role shiki:~/w/sh1 (Haswell-EP). Next reference: a RAM copy of origin/next a551cd8c2,
/mnt/zram/claude/LAYOUT-TEXT-PIN-12/next (built). Scripts: ~/work/LAYOUT-TEXT-PIN-12/{padmap.sh,classify.py,check.sh}
on shiki (padmap.sh <tree> <tag> [nobuild]: per pre pad 0..0xf000, fork/true minflt + a perf fault map per mode).
Decisions in force: as -11 (D-X.63, D-X.56, D-X.47, D-X.49, D-4.53). Reserved and unused: D-X.71.

## Measured (fast products, minflt, min of 3, no ASLR; jobs layout-text-pin-12-padmap-head / -next)
| pre pad | 0 | 800-1000 | 2000-5000 | 6000-7000 | 8000 | 9000-c000 | d000 | e000-f000 |
|---|---|---|---|---|---|---|---|---|
| fork, -11 head | 6 | 6 | **7** | 6 | 6 | 6 | **7** | 6 |
| fork, next a551cd8c2 | 6 | 6 | **7** | 6 | 6 | 6 | **7** | 6 |
| true, -11 head | 13 | 14 | 14 | 14 | 13 | 14 | 14 | 14 |
| true, next | 12 | 13 | 13 | 13 | 12 | 13 | 13 | 13 |

**Fork (#4760), cause found:** at pre 0x2000 the child faults on `liella_sys_set_robust_list` (window 0x40) and
`liella_sys_syscall` (window 0x41); at pad 0 both are in 0x40. The child resumes in `liella_sys_syscall` (fork's clone
goes through `raw::syscall`), and `thread.rs`'s after-fork reset re-registered the robust list via the dedicated wrapper,
16 KiB away in the start-up head. 4c33d8c6c routes that one call through `raw::syscall(nr::SET_ROBUST_LIST, ..)`, sets
FORK_BOUND 7 -> 6 and measures the fork child at the `pre` pads in `init_data_clusters_hold_at_every_text_phase`.
**NOT VERIFIED:** the per-pad run of the fix did not happen (the check job stopped at `cargo xtask so`, below).

**Start-up +1 (ruling #4777 follow-up), not removed:** the +1 is identical at every pad (one constant offset). perf's
time-ordered maps (head 19 / next 20 faults) differ only in which `.data1`/`.data` pages are touched, and next's extra
`_IO_2_1_stderr_` fault comes late (the result print / exit, after the probe's getrusage). Both trees show exactly one
kernel-mode fault (the bss tail zeroing), and the last PT_LOAD ends on page 0x87a in both. So the +1 is a fault perf does
not see. Hypothesis, not proven: an exec-time fault taken without registers (GUP-style, counted in min_flt but not
as a perf event). Next step: count `exceptions:page_fault_*` / the probe's own getrusage at main entry vs at the
measurement point, head vs next.

## Unreferenced-key check (#4778), done; it fails the head as expected
`group()` now reads each member's symbols and RELA sections (`elf_refs`). A key is unreferenced when none of its sections
is named by a relocation of another loaded section (`.eh_frame` and self-calls excluded) and none defines a global
C-level (unmangled) name. `so` writes `target/product/<profile>/hot-text.unreferenced` and fails on a listed one (regen mode:
printed only). regen.sh leaves those keys out of the new list. Unit test
`dead_key_check_flags_a_key_nothing_references` (assembled objects) passes. Job layout-text-pin-12-check (rc 1): clippy,
static and the test pass. Then `cargo xtask so` (fast) reports **30 listed keys** (startup: run_thread_dtors,
TryBox<Slot>::new, printf::send; locale 5, tool 3, extra 19). Not yet checked one by one that each is really an
inlined copy (trace names inlined frames, as with `program`); spot-check two with `objdump -d` of the probe first.

## Next step (exact)
1. Spot-check 2-3 of the 30 (no call/jmp to their address in the linked probe). If any is really called, fix `elf_refs`.
2. `tools/hottext/regen.sh . --build --allow-drop` in the fast and the release profile (it drops them and never re-adds them).
3. `padmap.sh ~/w/sh1 headN` (fork <= 6 at every pad expected), then affected --base origin/next, then plants
   handoff/LAYOUT-TEXT-PIN-12-plants.txt.

## Not to redo
The per-pad matrix of the -11 head and next, the fork child's fault map, the start-up perf comparison.
