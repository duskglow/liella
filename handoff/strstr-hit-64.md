# STRSTR-HIT-64 handoff (strstr_hit_64 to parity on Ivy Bridge and Alder Lake-N)

State: branch task/strstr-hit-64 from origin/next 7fcb6acda. Code commit d68f07deb (csys/src/machine/scan.S); plants
file handoff/STRSTR-HIT-64-plants.txt; SR-33 `tried` updated. Report json: handoff/reports/STRSTR-HIT-64.json.
No new D-row (local entry change; the strpair contract and strstr behaviour are unchanged).

## Profile (before)
wl_percall, keke (Zen 1), pinned specs: `ss_hit_64` 103 (AVX2) / 132 (SSE2, GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2).
glibc: 167 / 205 on Zen 1, but about 99 on Intel (Haswell-EP, both levels; timing-cov-strstr-nonavx2), so the SSE2 gap
(132 vs ~99) matched the 1.25x Ivy Bridge time. Split of the SSE2 call: entry ~28, strpair ~73 (4 head blocks 44,
broadcasts 9, page/level tests and the stop encoding ~20), after the call ~21, bench loop ~10. About 30 were the call
interface: call/ret, push/pop, the second page test, the 2k/2k+1 answer encoded and decoded, the stop byte read twice,
a page test before the compare. Neither the filter nor the verify loop held the surplus. The head blocks were already
one test per block.

## Change
liella_machine_strstr runs strpair's head test itself when h's page offset is 1..4016 ([h, h + 80) in one page):
AVX2 two 32-byte blocks, SSE2 four 16-byte blocks (needle bytes broadcast by imul 0x01010101, block 0 reuses the
prefix load), the needle compared at the stop in place, the stop byte read only when that compare fails (0: NULL; else
`liella_hot_strstr_next` with r = 2k). If those 64 bytes hold no stop, it calls strpair's aligned run through hidden
mid-entries `liella_machine_strpair_avx_run` / `_sse_run` with the registers strpair's own head would leave. Offsets
4017..4079 keep the strpair call.

## Results
wl_percall (keke, instructions per call; before -> after; glibc Zen 1): AVX2 ss_hit_64 103 -> 74, strstr_hit_short
93 -> 67, ss_miss_64 91 -> 76, ss_hit_1k 291 -> 283, all 4k rows -8; SSE2 ss_hit_64 132 -> 103, strstr_hit_short
113 -> 84, ss_miss_64 120 -> 105, 4k rows -4. No strstr row went up.
Time (staged cbench_memref, Liella vs glibc on the same host; sweep 1107b before):
| row | Ivy Bridge (kotori) | Alder Lake-N (nozomi) |
|---|---|---|
| strstr_hit_64 | 1.000 (was 1.25) | 0.809 (was 1.18) |
| strstr_miss_64 | 0.573 | 0.531 |
| strstr_hit_4k | 1.050 | 0.728 |
| strstr_miss_4k | 0.418 | 0.293 |
strstr_hit_4k on Ivy Bridge at 1.05: its SSE2 count went down (2194 -> 2190), so this change did not cause it. The
4k SSE2 group loop (~2190 instructions) is the next item for that row.

## Jobs
keke: strstr-hit-64-pc-base, strstr-hit-64-check1 (strstr_find on both levels + counts), strstr-hit-64-gate2,
strstr-hit-64-plants2. nozomi: strstr-hit-64-time-nozomi (rc 0). kotori: strstr-hit-64-time2-kotori (rc 1 only from
advisory cycle limits on __memcmpeq_eq_128 / _tail_1k; no strstr row over its limit). Superseded: -gate (static failed
on the stage tarball I left in the role root, removed), -plants (sse-stop32-offset SURVIVED, now marked equivalent),
kotori -time-kotori (`--no-build --compare` needs a host-static build that the stage does not hold).

## Next step
Ivy Bridge strstr_hit_4k (1.05): profile the SSE2 64-byte group loop. strstr_dense_4k stays open in SR-33.
IMPROVEMENT: `cargo xtask bench --stage <name>` writes the tarball into the role root, where `xtask static` then
scans it (private-paths, hottext): stage to ~/work/<story>/ instead.
