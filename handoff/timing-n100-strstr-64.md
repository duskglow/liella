# TIMING-N100-STRSTR-64 handoff (strstr over 64 bytes on Alder Lake-N)

State: branch task/timing-n100-strstr-64 (stacks on TIMING-IVB-STRSCAN-DISPATCH-2, 883a05df). Role shiki:~/w/sh1
(Haswell-EP, AVX2). Code commits: fd822851 (strpair head blocks: one mask per block), df0ada21 (exact strpair test +
selftest::strpair hook). Report: handoff/reports/TIMING-N100-STRSTR-64.json. No new D-row (local kernel change; the
scanner contract and strstr behaviour are unchanged).

## Profile (one strstr_hit_64 call, 134.4 instr; from the disassembly of the staged bench binary on shiki)
Rust ~72: 5 push + 5 pop + stack frame ~12, the unrolled needle/prefix loop 4 per needle byte (~22 for "ZEBRA"),
cpu::ready 3, kernel argument setup 6, candidate compare by length ~15, KResult/Option plumbing through the stack ~8.
Kernel ~52: entry and page test 6, level test 2, broadcasts 5, r8 clear/test 3, two head blocks 15 + 12 (each: three
compares, an and, two mask extracts, lea/xor/and chain, two branches), page test of block 2 4, exit 6. Bench loop ~10.
miss_64 ("lazy cat"): the same with 8 needle bytes, the terminator in head block 2, no compare.

## Change (fd822851, csys/src/machine/scan.S liella_machine_strpair)
Each head block uses the group loops' test min((Y ^ c1) | (L ^ c0), Y) == 0: one mask extract and one branch per
block, the stop byte says terminator (0) or candidate. Block 2's page test reuses the entry's page offset. The direct
entry no longer clears/tests r8; the unit-by-unit path sets pair mode and its own vectors (AVX2 and SSE2), so the SSE2
entry is also 3 shorter. Filter mode for the continuing run is computed only on that path. Same loads as before.

## Counts (cargo xtask bench --compare --counters cbench_memref, instructions per call, shiki, Liella AVX2)
| row | before | after | glibc |
|---|---|---|---|
| strstr_miss_64 | 139.4 | 128.4 | 173.4 |
| strstr_hit_64 | 134.4 | 125.4 | 99.3 |
| strstr_miss_4k | 922.5 | 910.4 | 6446.5 |
| strstr_hit_4k | 662.7 | 650.7 | 2243.2 |
Other rows: +0.1 on 20 rows the change does not reach (memset, memchr, wcsstr, ...; memset_64k +0.4): run-to-run noise
of the per-call setup subtraction, not code (wcsstr uses wcspair, untouched). hit_64 target (<= 99.3) NOT reached.
SSE2 (Ivy Bridge-class) path: not counted this run (by construction -3 at entry, the head is unchanged).

## Jobs
count-before, count-after (rc 0); plants (8: 7 CAUGHT, stop32 SURVIVED -> new exact test df0ada21); gate killed rc 143
(superseded: it was on fd822851 and slow under load 16). Final head jobs (started at hand-back): -gate2 (clippy
--all-features -D warnings, static, strstr_find, affected direct tier --base origin/next), -plants2 (same 8 plants;
stop32 should now be CAUGHT by strpair_names_the_first_candidate_or_the_terminator_exactly).
Timing NOT run: bookings 53 (nozomi) / 54 (nico) made and cancelled for budget.

## Next step
1. Read -gate2 / -plants2 on shiki (jobwait). 2. Booked timing of strstr_miss_64 / hit_64 (and 4k) on nozomi (N100)
and nico (Ivy Bridge), Liella vs glibc, from a stage of this head (handoff/timing-sweep-1105.md "Stage").
3. hit_64 needs ~-26 more: the Rust side (push/pop frame and the stack-returned KResult ~20, needle loop 4/byte).
A leaf fast path for needles <= 16 that keeps the kernel call's values in caller-saved registers is the next design.
Not to redo: the kernel head chain (done here).
