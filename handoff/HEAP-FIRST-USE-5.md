# HEAP-FIRST-USE-5: continue HEAP-FIRST-USE-4 (release hot-text red, `hfu4-head2`)

State: branch `task/heap-first-use-5` = HEAP-FIRST-USE-4's tip 272b847e2 + bbce0fc66 (regen.sh: clean trace
environment) + 9f60a8bd0 (hot-text list regenerated on the Skylake-SP host, mei) + this handoff/report commit.
Roles: keke:~/w/n3 (build host, Zen 1), mei:~/work/HEAP-FIRST-USE-5/tree (the regen, a copy of the tree).
Status: INCOMPLETE. Release products green; fast products red by 1 fault on the time path (below). Do not land.

## Decisions in force
D-3.20 (amends D-3.12; `records/decisions/D-3/D-3.20.toml`). Rulings #4241 (keep the folding decision lazy, group
checked rest paths, never loosen the 0-fault bound), #4197 (done in HEAP-FIRST-USE-3: the vDSO lookup no longer
takes the folding decision), #4199 (first_* timing goes to the next timing sweep: not booked here), #4291 is
HOTTEXT-REGEN-HOSTS (not fixed here; see the INFORMATIONAL below). Never commit views.

## What this run found and did
1. Step 0 `hfu5-step0` (keke, inherited head): clippy rc 0, static rc 0.
2. Why `hfu4-head2` was red (release: locale 56 > 55, tool 59 > 58). origin/next's release products on keke
   (`hfu5-basecheck`, `hfu5-basefaults`): true 15, locale 54, tool 57, all green. faultmap (`hfu5-fmap2`) of
   next vs the HFU4 head, release:
   - .text +1 in the locale run: `liella::text::builtin_charset` outside the hot group. CAUSE: regen.sh's trace
     set LANG and emptied LC_ALL but passed the rest of the caller's environment to the probe. ssh forwards LC_*
     to the hosts and our jobs carry `LC_CTYPE=C.UTF-8`, so p_loc's `setlocale(LC_ALL, "")` took LC_CTYPE from
     C.UTF-8 and never ran the locale file's LC_CTYPE path. The list lost builtin_charset, CtypeFile::wide,
     fill_u8/fill_erms (HFU4 item 4). hot_text_products runs its oracle with env_clear + LANG, so it does
     reach that path. This is one concrete cause of #4291's "output depends on the tracing host": the result
     depends on the session that started the regen.
     FIX (bbce0fc66): the traces run under `env -i PATH HOME LANG HOT_TEXT_*`. A regen on mei with only the
     list regenerated before the fix (`hfu5-regen-skx`, rc 101) still gave 56/59. After the fix,
     `hfu5-regen-skx2` re-added the keys and the release hot_text_products passed 14/14 on mei.
   - .data +1 in every program: the HFU head's text is one page longer, so all of .data moves up 0x1000 and
     `liella_legacy::global_locale::C` (next 0x7af710, head 0x7b0710) crosses a 64 KiB fault-around boundary.
     Same layout inside .data; only the start moved. With the restored keys the release bounds hold anyway.
3. Hot-text list regenerated on the Skylake-SP host only (mei, coordinator note #4291), both profiles, with
   the fixed regen.sh: 9f60a8bd0 (+3 keys, `Locale::with` and fill_u8 reordered; startup-text.keys unchanged).

## RED, open: fast products, time path (`hfu5-regen-skx2` on mei, `hfu5-head` on keke)
`init_first_mktime_after_localtime_r_takes_no_fault` 1 (bound 0) and `init_first_localtime_r_and_mktime_...`
21 (bound 20). faultmap on mei (`hfu5-fmapk`): the fault is `liella::time::civil::days_from_civil64` at
0x450640, 0x640 bytes past the 64 KiB window (0x440000-0x44ffff) that holds localtime_r (0x446bc0) and mktime
(0x44d840). HEAP-FIRST-USE-3 fixed the same straddle by moving `decide_topology` out of the hot text. Here the
three restored locale-tier keys, which precede the time keys, push days_from_civil64 back over.
HFU4's `hfu4-gate2` (keke-traced list, fast) was red on the same path (localtime_r and the combined test).
The time path's 0-fault bound depends on where a 64 KiB boundary falls, so any change to the hot group can
break it.
Next step, in order:
1. A window-aware layout in xtask/src/hottext.rs. Recommended: place the time-path keys (the localtime_r /
   mktime trace) so they never straddle a 64 KiB window. Either start that group on a window boundary when it
   would straddle, or order it before the locale tier. Prove it with plants: a pad before the group must not
   break the test. Do not shrink code by hand to slide days_from_civil64 back. Never loosen a bound.
2. Then: direct-tier gate, plants (`handoff/HEAP-FIRST-USE-4-plants.txt`; in `hfu4-plants2` 5 CAUGHT, 2
   WARN-NO-MSG: `topology-decision-inlined` and `hot-list-drops-memchr-rest` failed at the plant's `cargo xtask
   so` step, not in the test. Check their so.log under `/dev/shm/claude/HEAP-FIRST-USE-4/plants-2/logs/`).

## Counts (keke, `hfu5-head`; base `~/work/HEAP-FIRST-USE-4/base/counts.txt`)
Zen 1 host, 2 allowed nodes (Mems_allowed 0-1), fast products, perf stat -r 200, user instructions / kernel
instructions / faults, base -> head: `_` 3073/199163/13 -> 3072/198477/12; `P` 4838/229841/16 -> 4839/241200/16;
`M` (first malloc) 9193/319170/24 -> 9720/309145/23; `PM` 10969/324880/24 -> 11498/326211/24; `PS` (setlocale)
67624/521895/48 -> 68824/534671/49; `PSG` 102733/692260/61 -> 104023/694223/59. On a multi-node policy the
first malloc asks for the node (D-3.20), which costs +527 user instructions and saves 10k kernel instructions.
A one-node host does not ask (the count on one node is not measured here). glibc (static) M: 150011 iu / 25 faults.
The first malloc does no openat (no mmap_min_addr read, #4197 holds). NOT VERIFIED: under strace, `hf.liella M`
printed `exited with 1`. Check whether hf.c's M variant returns 1 by design.
Release hot_text_products on keke: 14 passed. Fast: mktime 1 (bound 0), localtime_r + mktime 21 (bound 20),
localtime_r 20 (passes).

## Not to redo
The cause of the release red (item 2). The cause of #4243 (HFU4). #4197 (HFU3).
