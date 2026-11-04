# PERF-STRSTR-LONG handoff

State: branch task/perf-strstr-long, code commit 6d7b0dbc (csys/src/machine/scan.S strpair, crates/boundary/tests/strstr_find.rs), then docs and report.

## Change
`liella_machine_strpair` (the strstr candidate/terminator scanner):
- AVX2: after the 32-byte steps reach a 64-byte boundary, one 64-byte group if not 128-aligned, then a 128-byte loop
  (4 blocks, memory operands: vpxor/vpminub read the block directly, 4 instructions per block, one branch per 128 B).
- SSE2: a 64-byte-group loop (4 blocks) from a 64-byte boundary on (was one 16-byte block per pass).
- New test `strstr_large_haystacks_find_every_position_in_the_wide_groups` (haystacks 300..780, matches at every 3rd offset and
  the last, 4 alignments, both paths).

## Counters (sumire, Intel Xeon Platinum 8151, `cargo xtask bench --compare --counters --runs 5 cbench_memref`, jobs perf-strstr-long-base / -after1 / -after2)
| row | level | instr before | instr after | glibc instr | ns before | ns after |
|---|---|---|---|---|---|---|
| strstr_hit_4k | default | 1277.9 | 1051.7 | 2243.7 | 202 | 177 |
| strstr_miss_4k | default | 1269.3 | 998.1 | 6488 | 202 | 172 |
| strstr_hit_64 / miss_64 | default | 168.4 / 196.4 | 168.5 / 195.4 | 99.4 / 210.4 | 13.8 / 15.9 | 14.1 / 15.9 |
| strstr_hit_4k | -AVX2 | 5006.9 | 2375.8 | 2243.6 | 551 | 414 |
| strstr_miss_4k | -AVX2 | 5021.1 | 2341.0 | 6484 | 551 | 408 |
| strstr_miss_64 | -AVX2 | 225.4 | 260.5 | 210.4 | 21.8 | 23.5 |
| strstr_hit_64 | -AVX2 | 214.5 | 214.5 | 99.4 | 19.4 | 19.1 |
(-AVX2 ns is noisy: the same SSE2 code measured 551 ns (base) and 351 ns (after1, SSE2 code not yet changed), then 414 after the
change. Decide the -AVX2 level on instructions only.)

## Not settled
- The register row is for keke (Zen 1; glibc 826 instr there). I had a sumire role only; sumire's glibc takes a different path
  (2244 instr). The instruction gain is real on both, but "at least glibc" on keke is NOT VERIFIED.
- `strstr_hit_64` (1.7x glibc in instr) is the setup cost of the call, not the scan: not touched (PERF-STRSTR-SHORT-style story).
- -AVX2 `miss_64` +35 instr: the extra `testl/jnz` per 16-byte step.
## Proof
Gate perf-strstr-long-gate (direct tier, --base origin/task/perf-next-batch-3); plants perf-strstr-long-plants2: 9 CAUGHT, 1
EQUIVALENT (a weaker candidate mask only adds candidates, which Rust verifies). Plants run 1 (perf-strstr-long-plants) survived on
that one mutant, so was replaced by a suppressing one. wl_count not run (no workload named by the brief).

Next step: measure on keke (book timing there) and decide whether to shrink the SSE2 `miss_64` cost.
