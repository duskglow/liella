# TB-FOREIGN-WORDS-LOW: errno, TID, dtor, heap and node words (0x80-0x9b) in a block Liella did not make

State: branch `task/tb-foreign-words-low-11` = -10's 3c90ec0bf plus this handoff and its report (no code change in
-11). Code head c0ac5f025. Decisions in force: D-X.58, D-X.60, D-X.62, rulings #4835, #4849, #4851, #4779 (<= +2 per
one-access call vs next, judged on Release #4889), #5325, #5378, #4347. The owner's safety requirement (words
0x80-0x9b) is done; only the layout cost is open. Older reports: handoff/TB-FOREIGN-WORDS-LOW-report.md (-1..-4),
git history of this file (-5..-10).

## -11: why CI train 709 saw STARTUP-DATA-WINDOWS 8 > rw-budget-fast 7
- Step 0, job `tb-foreign-words-low-11-lint0` (keke n2, 3c90ec0bf): workspace clippy, xtask clippy, static: rc 0.
- Jobs `tb-foreign-words-low-11-layhead` (3c90ec0bf) and `-laynext` (origin/next f6cab48d7), keke (Zen 1), RAM
  copies, fast products; the copies' data budgets were lowered (scratch only) so the check prints every page.
  **Both: 7 written start-up pages.** Both have the `.got` page (2 slots addressed by `locale::refresh_words`, 3 by
  `stdio::parts_first_use`): next has the same five slots, so that page is not this branch's (coordinator's suspect
  cleared; no GOT load was added).
- CI's list (merged tree, mei): the start-up `.data.rel.ro` objects span 0x850000-0x852118 = 8472 bytes: 3 pages,
  280 bytes over two. On keke they span 2 pages with about 1350 bytes to spare (next 0x860000-0x861ab8, head
  0x8602b0-0x861ab0).
- The difference is one object at the head of CI's group: `anon.487b3ce21fd28cd89b51a3b06485385c.94` (1640 bytes,
  0x850030). In keke's next build it is the GLIBC_TUNABLES name table (addressed by `liella::config::warnings` for
  `runtime::start`'s closure and by `config::glibc_tunable_type`), placed just past the group (0x861c08) and not a
  start-up key. `warnings` reads it only when `GLIBC_TUNABLES` is set. Without it CI's group is 6832 bytes: two pages.
  CI's group also holds `__fini_array_end` and `anon.97253...139` (as next on keke does; head on keke does not).
- The start-up data keys come from a perf trace of `page_faults true` on the build host
  (tools/hottext/startup_data.sh), so the set depends on the host and its environment: keke results cannot stand in
  for the CI host. `GLIBC_TUNABLES` is not set by the trace tools, by `~/bin/gate` on mei or in mei's ssh environment.
- Not settled: whether next's own CI build carries the table too (CI prints the page list only when over budget),
  and what makes the CI trace touch it.

## Refused (this run)
Starting the same two builds on mei (the CI host class) under jobrun, labels `tb-foreign-words-low-11-meihead` /
`-meinext`, RAM copies in /mnt/zram/claude/TB-FOREIGN-WORDS-LOW-11/{head,next}: refused (not my assigned host).
The source copies and ~/work/TB-FOREIGN-WORDS-LOW-11/ scripts there were left for ram-sweep; nothing was started.

## Next step (in order)
1. With mei use authorised: build head and next there as layhead/laynext did (keke ~/work/TB-FOREIGN-WORDS-LOW-11/:
   lay.sh <tree> <outdir>, lower.py, ana.sh, pages.py <log> <max-addr>) and diff the two page lists. Answer: is the
   tunables table a start-up key in next's mei build, and which access in the trace touches it.
2. If only the trace environment brings it in: a tooling question for the coordinator (trace in a fixed
   environment), not this branch's layout. If it is a real start-up access: keep the table out of the start-up
   `.data.rel.ro` group (its own cold section) or shrink the group by >= 280 bytes on the merged build. No budget
   change (ratchet); if neither works, RULING with the page list.
3. Then the CI gate command (ops `ci_gate_cmd`) on the final head on a non-shiki host, as the -11 brief asks.

## Not to redo
-1's audit; the registry/FOREIGN/OwnPages/asm design; -7's SPARE split; -8's Haswell-EP comparison; -10's mei
fault-set comparison (pfset.sh, an.py, sz.py in mei ~/work/tb-foreign-words-low-10/); -11's keke page lists (logs
keke ~/logs/tb-foreign-words-low-11-lay{head,next}.log; probe disassembly in /dev/shm/claude/TB-FOREIGN-WORDS-LOW-11/ana-*).
Open from -10: the text before the 32 KiB-aligned string kernels is about 9 KiB over the boundary next sits under
(LAYOUT-RETHINK's question); the remaining `OwnPages` monomorphisations (memalign family, `first_cache`).
