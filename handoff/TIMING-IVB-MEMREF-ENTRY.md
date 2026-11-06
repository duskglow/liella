# TIMING-IVB-MEMREF-ENTRY (handoff)

State: branch task/timing-ivb-memref-entry (from origin/next 18f5d4c5, which already holds TIMING-IVB-STRSCAN-DISPATCH and -2).
Code: csys/src/machine/scan.S only (commit fea51965; its fold.S part was reverted in the next commit, so fold.S equals next).
Role mei:~/w/s2. Final tree (synced c60ea675, handoff-only commits after): mei:timing-ivb-memref-entry-gate2 rc 0
(clippy -D warnings, xtask static, affected --base origin/next: 231 of 237 targets + 117 conformance programs, peak
1.4G), mei:timing-ivb-memref-entry-plants2 rc 0: 3 plants, 3 CAUGHT, no WARN. The first gate (timing-ivb-memref-entry-gate, rc 0) ran on fea51965 with
the fold split. The first plants run (timing-ivb-memref-entry-plants) was REFUSED by xtask plants (a .S plant must
anchor on `liella_machine_<fn>:`), so it ran nothing.

## Change (scan.S)
- memchr: on the SSE2 class the entry's `jae` (taken on every SSE2 call) now lands on `.Lmemchr_sse_pre`, the SSE2
  64-byte head, 32-byte aligned. Its `cmpl $4032` is both the level test and the page test (ecx >= short_lim there, and
  short_lim is 4033 on the AVX2 level). Before, it went to `.Lmemchr_entry`, loaded the scan level and took a second
  branch to the head. Page-end calls on either level go to `.Lmemchr_entry` as before (`.Lmemchr_sse2` is now the
  SSE2 page-end form only).
- rawmemchr: the entry's `jae` lands on `.Lraw_sse_fast` (32-aligned, with the `cmpl $4032` moved there) instead of
  going through `.Lraw_pre`'s compare and a second taken branch. The instruction count is the same; there is one taken
  branch fewer.
- AVX2 level: nothing changes on any bench row. A page-end call runs the same compares in a different place.

## Instruction counts (mei, cbench_memref, Liella / glibc instructions per call; before = next 18f5d4c5)
| row | -AVX2 (hwcaps=-AVX2) before -> after | AVX2 before -> after |
|---|---|---|
| memchr_16 | 46.3 -> 44.4 (1.58 -> 1.52) | 25.3 -> 25.4 (1.04 -> 1.05, noise: code path unchanged) |
| memchr_64 | 42.4 -> 40.4 (0.78 -> 0.74) | 29.4 -> 29.3 |
| memchr_4k | 968.4 -> 966.3 | 422.0 -> 421.7 |
| rawmemchr_4k | 938.3 -> 938.4 (same count, one taken branch fewer) | 365.8 -> 365.8 |
| other memref rows (memcmp, bcmp, __memcmpeq, memset, memmove) | unchanged | unchanged |
Jobs: mei:timing-ivb-memref-entry-counts-before and -counts-after (rc 0). The after run measured fea51965, whose scan.S is
byte-identical to this head. Its fold.S rows (the split) are given below. On this head fold.S is the before tree.

## Not landed: SSE2 129-192 copy/fill split (handoff/TIMING-IVB-MEMREF-ENTRY-fold-split.patch). RULING NEEDED
With six 16-byte blocks from each end for 129-192 (12 stores instead of 16; Ivy Bridge retires one store per cycle, and
glibc's memset_144 at 15.4 cycles per call is below a 16-store floor), the -AVX2 counts were memmove_144 53.4 -> 47.4
and memset_144 39.3 -> 37.4. But memmove_256 went 53.4 -> 55.4 and memset_256 39.4 -> 41.4, from the size compare on
193-256. Under "no measured row slower" it is reverted. Recommendation: time it on nico (memset_144/memmove_144 vs
_256). If the 193-256 rows hold cycles (they are store-bound) while 129-192 gains, rule that instruction-only
regressions on store-bound rows can be accepted.

## Step 1 (stall attribution): NOT DONE
No perf sampling on the Ivy Bridge host and no booked timing in this run (budget). The diagnosis is static: the memchr
SSE2 path had two taken branches plus a global load before its first compare, the shape STRSCAN-DISPATCH fixed. The
memset/memmove 144 gap fits the store-port floor above. memcmp_144 / bcmp_tail_128 / __memcmpeq_tail_8 have no taken
branch before their first compare on the 8-byte path, so the hypothesis does not explain them: NOT VERIFIED, still open.

## Next step
1. Booked timing (nico Ivy Bridge, nozomi N100) of memchr_16/64/4k and rawmemchr_4k, before = next, after = this branch,
   using the `bench --stage` method and handoff/TIMING-IVB-STRSCAN-DISPATCH-2-{run.sh,analyze.py}. Add the fold-split
   patch as a third tree for the ruling above.
2. Profile memcmp/bcmp/__memcmpeq on nico with region counters plus idq_uops_not_delivered / resource_stalls.
Do not redo: the counts above, the plants.
