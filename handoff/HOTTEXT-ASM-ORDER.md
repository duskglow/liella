# HOTTEXT-ASM-ORDER (branch task/hottext-asm-order, role mei:~/w/s1)

State: run 1 checkpointed at its budget (50 calls / 100k). Code commit 4cc83213 (WIP: per-function
`.S` sections) on top of next db50ef41. The hot-text list is NOT regenerated or committed yet; no
bound was touched.

## What was done

- Cause (from PERF-READDIR-ENTRY run 3): xtask/src/hottext.rs renames function sections only. The
  `.S` kernels in each object's plain `.text` could not be renamed, so they landed after the whole
  hot group. On mei's built libc.a, 12 listed keys sat in plain `.text`: memcmp, strcpy (+ stpcpy,
  _chk, strncpy, stpncpy families), memchr, index, strncmp. Also memcpy/memmove/memset (fold.S),
  read/write/close (points.S) and tp/once/guard/barrier/rdtsc were never in a function section,
  so regen dropped their traced names (it keeps only names that are function sections).
- Fix: each function in a plain `.text` region of scan, fold, points, tp, guard, barrier, rdtsc,
  once, trap, restorer, pkru, call, fpu and thread_start `.S` now opens `.text.<first label>`
  (34 new sections). The split was made by a scratch script
  (chisato ~/w/.scratch/HOTTEXT-ASM-ORDER/split.py), which splits only where the previous
  instruction is an unconditional jmp/ret/ud2, no `.cfi` region is open, and no earlier symbol's
  `.size` reaches past the split point. Kept together on purpose: rawmemchr/memchr/memrchr entry
  gates (they fall through into the kernel), stpcpy+stpcpy_chk and memmove+move_mid (`.size`
  spans), syscall_cancel+syscall_cancel4 (one CFI region). context, setjmp, mcount, entry and vdso
  were left alone (cold, or already sectioned).
- Checked: every new section of the assembled objects ends in a terminator (mei
  ~/work/hottext-asm-order/verify.sh, objdump of our own objects): none reported.
- tools/hottext/merge.py lets the new trace's order win, so after a regen the kernels take their
  first-touch positions (keys `liella_machine_<fn>`; the tracer records every alias at an address).

## Jobs (mei)

- hottext-asm-order-static: rc 0 on 4cc83213 (workspace clippy --all-features -D warnings, xtask static).
- hottext-asm-order-gate: started detached on 4cc83213 (direct tier --base origin/next, then the
  boundary kernel tests by name: machine_scanners, small_copy, s04_strings_symbols, find_seq_scan).
  Logs ~/work/hottext-asm-order/gate-*.log.
- hottext-asm-order-faults-mei: started detached (~/work/hottext-asm-order/count.sh). Three trees in
  /mnt/zram/claude/HOTTEXT-ASM-ORDER/count-1/{befr,aftr,regn}: befr = next's `.S` files, aftr =
  split, regn = split + list regenerated (fast profile) with `page_faults tool`. hot_text_products
  5 times each; per-run lines in its log (`before n:`, `split n:`, `regen n:`), regen list in
  ~/work/hottext-asm-order/hot-text.list.regen, diff in list.diff. rc 0 only if the regen tree's
  suite is green 5/5. Re-attach: `grep -P "^hottext-asm-order-faults-mei\t" ~/logs/jobs.tsv`, output
  in ~/logs/hottext-asm-order-faults-mei.log (jobrun's log) and ~/work/hottext-asm-order/run-*.log.

## Results so far (mei, Skylake-SP, 5 runs each, every run identical)

- before (next's .S): tool 68 / locale 66 / startup 19, strings 0 faults, suite 6/6 green.
- split (4cc83213, old list): tool 68 / locale 66 / startup 19, strings 0, 6/6 green: the split
  alone changes no count.
- regen (fast profile, `pf_tool tool`): 407 keys traced, list 503 -> 530 keys
  (~/work/hottext-asm-order/hot-text.list.regen). Its `cargo xtask so` FAILED the D-4.33 export
  alignment check: mempcpy/__mempcpy (fold.S, `.p2align 4` in their new own section; inside
  memmove's section they sat 64-aligned by position). Fixed in bff0ac96 (`.p2align 6`). Not yet
  re-counted. Next run: also grep every new section that holds an exported entry for `.p2align`
  < 6 (only these two were reported, but others can be aligned by luck in this layout).
- faults-mei rc 1 (the regn build failure above): SUPERSEDED. hottext-asm-order-gate on 4cc83213:
  stopped at hand-back (head moved; its sync blocked the bff0ac96 sync, sync_role rc 4): SUPERSEDED.

## Exact next step

0. Sync bff0ac96 to mei s1, rerun count.sh as hottext-asm-order-faults-mei-2 (new zram dir count-2).

1. Read the faults-mei result. If regen 5/5 is green with margin on every bound: copy
   hot-text.list.regen to crates/libc-so/hot-text.list, also run regen once with
   LIELLA_FULL_PROFILE=1 (the list is the union of both profiles; regen.sh header), commit.
2. Add the split kernels (liella_machine_strncmp, _memchr_entry, _strcpy, _compare, _read) to
   `init_cold_startup_functions_are_grouped` (or a new test) so a kernel back in plain `.text` is
   caught; plant: put `.text` back in front of strncmp in scan.S (anchored to its label), expect that test.
3. Run count.sh on keke (`hottext-asm-order-faults-keke`, in /dev/shm/claude/HOTTEXT-ASM-ORDER/), the
   gate and the plants on the final head; record before/after per test and host in the test's doc
   comment and here. No bound may be raised.

Open question: none yet; if a bound still fails after regen on either host, it is a RULING NEEDED.
