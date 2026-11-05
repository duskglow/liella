# STRSTR-SHORT-DENSE-PARITY handoff (after -2: short rows at parity; hit_1k and dense_4k not yet)

State: branch task/strstr-short-dense-parity-2 (on task/strstr-short-dense-parity, merge base a65a823f with origin/next).
Commits of -2: 8c8837b5 (wl_trace.sh --list), 0859c43b (strstr first pass inline, loop out of line), 5e0526f6 (miss
ends in the first pass), then docs. Role mei:~/w/s1. Report: handoff/reports/STRSTR-SHORT-DENSE-PARITY-2.json.
Decisions in force: owner ruling #2686 (adaptive filter). No new D-row (local refactor, behaviour unchanged).

## Per-phase split of one call (wl_trace.sh --list, Liella only; traced = percall minus ~9 harness)
Before -2 (head 8c8837b5 = a65a823f code):
| phase | ss_hit_64 (179) | strstr_hit_short (162) | ss_hit_1k (427) | strstr_dense_4k (986) |
|---|---|---|---|---|
| export wrapper + KResult out-pointer + result stores | 23 | 23 | 23 | 23 |
| find_str_n save/restore | 16 | 16 | 16 | 16 |
| NULL checks, needle measure + prefix check (2/byte) | 27 | 29 | 27 | 29 |
| cpu::ready + loop-state setup (spills) | 36 | 36 | 36 | 36 |
| kernel entry, broadcast, head block | 36 | 34 (hit in head) | 36 | 36 |
| kernel alignment prologue (block loop to 64/128, loop64, mode) | 15 | 0 | 146 | 55 |
| kernel main loop (one-stream 16 or pair 23 per 128 B) | 0 | 0 | 105 | 736 |
| kernel group exit (back to block loop) + return | 6 | 0 | 14 | 31 |
| verify (length dispatch + 2 word compares) + decode | 28 | 28 | 28 | 28 |
The Rust rows are read from the full ss_hit_64 listing (the other calls were split by function, Rust total 124-128,
and by kernel offset); kernel rows are exact per call. Reference totals (Zen 1, black box): hit_64 163.2, hit_short 153, hit_1k 355.2, dense_4k 818 (= scan_4k).
The surplus on the short rows was the fixed Rust cost (~128 of 162-179), so -2 aimed there.

## Result (instructions:u per call, wl_percall on mei; Liella's AVX2 path counts equal keke's: base 188/997 match)
| row | Zen 1 glibc | base | -2 head | ratio |
|---|---|---|---|---|
| ss_hit_64 | 163.2 | 188 | 141 | 0.86 |
| strstr_hit_short | 153 | 173 | 126 | 0.82 |
| ss_hit_1k | 355.2 | 436 | 389 | 1.10 (open) |
| strstr_dense_4k | 818 | 997 | 950 | 1.16 (open) |
| ss_miss_64 / ss_hit_4k / ss_pairmiss_4k / ss_miss_4k | 339.8 / 833.6 / 951 / 7507 | 217 / 769 / 695 / 961 | 181 / 722 / 659 / 925 | |
| strstr_scan / verify / miss / hit _4k | 818 / 18524 / 10146 / 10155 | 775 / 1021 / 1003 / 1271 | 728 / 974 / 967 / 1275 | |
Logs: mei ~/work/strstr-short-dense-parity-2/pc_*_C.log (job strstr-short-dense-parity-2-counts2).

## Layout noise in the 4 KiB rows (new finding; explains -1's "not understood" +15)
The kernel's alignment prologue (13 per 32-byte block up to a 64/128 boundary, then loop64) depends on the
haystack's address mod 128, and strstr_cycles.c's static arrays move with the binary's code size. Same kernel,
different link: dense kernel 858 -> 903; between counts B and C (only the miss check changed) hit_4k 1230 -> 1275,
dense 995 -> 950, scan 675 -> 728. So the 4 KiB rows carry about +-50 of layout noise; hit_4k's +4 vs base is that
noise (the Rust saving of 47 shows on every row whose kernel path does not depend on alignment). RULING NEEDED.
The base vs origin/next 4k differences (+-45 on the strstr_cycles rows, none on the ss_ rows) look like the same
effect; NOT VERIFIED (origin/next's tree was not inspected).

Jobs on 5e0526f6 (mei): clippy+static strstr-short-dense-parity-2-clippy2 rc 0; counts strstr-short-dense-parity-2-counts2
rc 0; plants strstr-short-dense-parity-2-plants2 rc 0 (5 CAUGHT, 0 WARN; plants file ~/work/strstr-short-dense-parity-2/plants.txt;
next-nul-at was declared equivalent but was caught, so that reasoning was wrong: the test notices the wrong re-check);
gate strstr-short-dense-parity-2-gate2 (direct tier). Superseded: -countsA (no base primed), -counts, -gate, -plants
(head 0859c43b, ended by kill-mine for the miss fix).

## Next step (in order)
1. Pin the haystack alignment in validation/percall/strstr_cycles.c (e.g. aligned(4096) plus a fixed offset per case),
   re-measure base and head: then the 4k rows compare builds, not layouts.
2. Kernel: replace the alignment prologue with one unaligned 128-byte group test at p (pages permitting) and then
   align down, so the prologue is a fixed ~25 instead of 15-146 by address. Expected: ss_hit_1k 389 -> ~320 (target
   373), dense_4k -> ~880; then a direct exit mask from the four group vectors (non-destructive vpminub) instead of
   re-running the block loop (~-25 per hit row). Dense floor with the AVX2 pair test: 4 ops per 32 bytes + 7 per
   group = 21.5-23 per 128 B (688-736 for 4 KiB), so 1.05x (859) needs a fixed cost <= 123-171; now ~214.
Not to redo: -1's DENSE hand-back and SSE2 prefix step. No bound is proved yet for hit_1k / dense_4k.
Tooling: wl_percall's default base is the merge base: prime it (wl_prime_base.sh) or pass --base <full sha>.
Plan rows to read: docs/saturation/03 strstr rows.
