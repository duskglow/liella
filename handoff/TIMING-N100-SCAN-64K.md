# TIMING-N100-SCAN-64K (handoff)

State: branch task/timing-n100-scan-64k, code head c35d7c4f (scan.S + machine_scanners.rs; later commits handoff/ only).
Stacks on TIMING-IVB-STRSCAN-DISPATCH-2 (base 883a05df). Role keke:~/w/n1. Scratch: keke:~/work/TIMING-N100-SCAN-64K
(stage.sh, jobs.sh, scan-{base,pf0,pf512,pf1024}.S, measure-<variant>-{avx2,sse2}.tsv, *.tar), nozomi:~/work/TIMING-N100-SCAN-64K.

## Change (csys/src/machine/scan.S, AVX2 level only)
The AVX2 long loops of memchr and rawmemchr already ran four 32-byte loads OR-reduced before one test (the brief's
first suggestion was in place). They now take 256-byte groups, 256-aligned (eight loads, seven vpor, one vptest; a
256-aligned group never spans two pages). When the 128-aligned loop entry is not 256-aligned, one 128-byte group runs
first. On exit, vptest of the first-half fold picks the half; the second half is resolved from ymm5-ymm8 (no reload).
memchr keeps its r8 end test at each group start. Per 128 bytes: memchr 12 -> 10, rawmemchr 10 -> 9 instructions.
Test: scanners_find_the_first_terminator... now runs from a 256-aligned base with lengths up to 1099, so rawmemchr's
alignment group, both halves and the second iteration are reached (before that, lengths < 300 never reached the loop).

## Profile / design note
I did not annotate the brief's rec.<row>.data (out of budget). Instead I designed from the counter evidence:
non_mem_scheduler stalls plus few loads in flight fit compare uops (fused with their loads) filling the vector
scheduler while they wait on L2. The wider body cuts the test, branch and pointer uops per byte. NOT VERIFIED on the
N100 yet (timing job below).

## Instruction counts (Zen 1 build host, cbench_memref --compare --counters --runs 3; Liella instr/call, glibc)
| row | base | 256 loop (final) | 256 + prefetch 512 | glibc |
|---|---|---|---|---|
| memchr_64k | 6191.5 | 5171.9 | 6194.7 | 6713.1 |
| rawmemchr_64k | 5175.3 | 4675.1 | 5700.0 | 5690.0 |
| memchr_4k | 422.0 | 364.0 | 423.9 | 465.8 |
| rawmemchr_4k | 365.9 | 347.9 | 412.1 | 402.8 |
memchr/rawmemchr _16, _64, _16_mix, _64_mix unchanged (+-0.1). SSE2 class (--tunables glibc.cpu.hwcaps=-AVX2):
every row unchanged within 1 instruction (SSE2 code untouched). No other row moved more than 0.5, except
strcasestr_miss_4k at pf1024 only (+13; not the final code). Jobs: keke:timing-n100-scan-64k-count-before (base,
rc 0), keke:timing-n100-scan-64k-variants (pf0/pf1024/pf512, rc 0; evidence, not on the final head).
Prefetch (4 x prefetcht0 per 256 B, distance 512 or 1024) was measured and dropped: rawmemchr_64k and _4k went
above glibc's count (the brief's limit). Its timing effect was NOT measured.

## Jobs on the final head (started detached at hand-back, chained on keke in this order)
keke:timing-n100-scan-64k-lint (cargo clippy -D warnings, xtask clippy, xtask static), keke:timing-n100-scan-64k-gate
(affected --base origin/next --run), keke:timing-n100-scan-64k-plants (9 plants, handoff/TIMING-N100-SCAN-64K-plants.txt,
--dir /dev/shm/claude/TIMING-N100-SCAN-64K/plants-1), keke:timing-n100-scan-64k-count-after (measure-after-*.tsv).
nozomi:timing-n100-scan-64k-n100: ~/bin/timing run.sh 5 3 base pf0 (pf0.tar = the final code: an empty prefetch
macro, so the same machine code), queued for booking #52 (nozomi, 120 min, booked 01:15Z). Cancel #52 when it ends.

## Open / exact next step
1. Read nozomi out/{base,pf0}-{1,2,3}/rounds.tsv and counters.tsv for the 8 rows (median of per-round Liella ns
   ratios, after/before; glibc drift as the check; handoff/TIMING-IVB-STRSCAN-DISPATCH-2-analyze.py works with the
   side names changed). Verdict: memchr_64k / rawmemchr_64k time ratio vs glibc (was 1.169 / 1.060).
2. The Ivy Bridge check (nico) was NOT run. The SSE2 code is unchanged and its counts are identical, but the layout of
   scan.S changed. Stage base/pf0 as above and run run.sh there.
3. If the 64k rows are still L2-bound, profile the final loop on the N100 (perf annotate on a build host). Prefetch
   would need a ruling, because it puts rawmemchr above glibc's count.
Not to redo: the variant builds and counts (the tars are on keke).
