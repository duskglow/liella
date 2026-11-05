# PARITY-SCAN-AVX2: handoff

State: branch `task/parity-scan-avx2` from origin/next 62826474. Code commits 654cc537 (scan.S), c407f33f (memrchr test,
plants file). Role shiki:~/w/sh2 (Haswell-EP); counts on mei:~/w/psa2-cnt (Skylake-SP, RAM-backed copy synced from 654cc537;
product source identical to c407f33f, which only adds a test and edits the plants file).

## Profile (before design)
Ladder slope (count(64k) - count(16k)) / 49152 * 128, Haswell-EP, both sides: strlen 10.0, strnlen 12.0, rawmemchr 11.0,
memchr 13.0 instr per 128 B, the same for Liella and glibc. Skylake-SP: glibc lower at 64 KiB by about 512 instructions in every
row (strlen 5177.3 vs 4661.7, rawmemchr 5685.0 vs 5169.4, memchr 6701.0 vs 6209.5, strnlen 6219.4 vs 5699.7), i.e. exactly one
instruction per 128-byte step; the surplus sits in the loop phase, not the prologue. Liella's loops retired
`vpcmpeqb/vpor fold; vpmovmskb; test; jz`. `vptest x, x` sets ZF iff the fold has no set byte: one instruction for
`vpmovmskb + test`. No AVX-512 path needed (item c not built).

## Change
- strlen / strnlen / rawmemchr / memchr / memrchr AVX2 loops: `vpmovmskb + test` -> `vptest` (-1 per 128 B). memrchr's two
  tail groups too. Loop exits rebuild masks from the kept compares (unchanged).
- strnlen max > 64 head: the clamp after the first two 32-byte blocks dropped (answer < 64 < max).
- SSE2 path (Ivy Bridge class) untouched: its instructions are byte-identical, so its counts cannot move.

## Results
Skylake-SP, `cargo xtask bench --compare --counters --runs 3` (cbench_memref, cbench_hot; job parity-scan-avx2-count-avx2),
instr Liella / glibc (before from the brief):
strlen_64k 4663.9 / 4662.5 (was 5177.3, 1.11 -> 1.00); strlen_4k 338.7 / 337.5 (370.7, 1.00); strnlen_64k 5706.7 / 5704.3
(6219.4, 1.00); strnlen_4k 421.6 / 419.5 (452.8, 1.01); rawmemchr_64k 5171.4 / 5173.7 (5685.0, 1.00); rawmemchr_4k 365.6 / 368.5
(396.8, 0.99); memchr_64k 6187.6 / 6202.7 (6701.0, 1.00); memchr_4k 421.5 / 437.6 (0.96); memrchr_4k 426.4 / 411.4 (458.5, 1.04).
NOT closed (not touched): strnlen_16 28.3 / 25.3 (1.12), strnlen_16_mix 35.3 / 32.3 (1.09), memrchr_64_hit 27.0 / 25.0 (1.08).
Haswell-EP percall (parity-scan-avx2-percall; the spec avx2_families.c on this branch has memchr only of these families):
memchr 65536 6718 -> 6206 (glibc 6714), 16384 1726 -> 1598, 4096 478 -> 446, 1024 166 -> 158, 256 88 -> 86, 128 75 -> 74;
16-64 unchanged. No row rose (the change only removes instructions).
Timing not measured: vptest is 2 uops on Intel; the periodic timing sweep judges the rows.

Plants (handoff/PARITY-SCAN-AVX2-plants.txt): run 1 found memrchr-vptest-op SURVIVED (the memrchr test planted matches from
the start up, so the first vector always matched too) -> c407f33f adds a one-match-alone pass; run 2: 7/7 CAUGHT.

## Open / next step
1. strnlen_16 rows (max = n + 1 <= 64, `.Lstrnlen_small`): the 3 extra are the max = 0 test and the clamp; no cheap cut found
   (folding max = 0 into the <= 64 test costs the max > 64 path). Needs a design for the small-max head.
2. memrchr_64_hit 27.0 vs 25.0: head path, not profiled.
3. Haswell item (a): strlen/memchr/rawmemchr at 128-129 B (loop entry + 64-bit mask exit, about 16 instr) not done; profile
   with wl_trace.sh first. The percall spec lacks strlen/strnlen/rawmemchr cases: add them to avx2_families.c.
4. Ivy Bridge count not run (SSE2 code unchanged by construction); a saturation-register row for this change not added.
