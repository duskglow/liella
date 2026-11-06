# TIMING-COV-MEMCHR-SHORT-NONAVX2 (handoff, run 4 = TIMING-COV-MEMCHR-SHORT-NONAVX2-4)

State: branch task/timing-cov-memchr-short-nonavx2-4 (run 3's branch + merge of origin/next 18e2c9d5-era, commit
d5dad9bc). Role mei:~/w/s1. Scratch mei:~/work/TIMING-COV-MEMCHR-SHORT-NONAVX2-4 (counts.sh, head.tar, base.tar),
mei:/mnt/zram/claude/timing-cov-memchr-short-nonavx2-4/base-1 (base tree), kotori:~/work/TIMING-COV-MEMCHR-SHORT-NONAVX2-4
(rows4.sh, base/, head/). Decisions: D-4.57 (s04, rewritten in run 4), D-4.56. Rulings #3500, #3501, #3579 accepted.

## Run 4: reconcile with TIMING-IVB-MEMREF-ENTRY
Both entries did the same thing: on SSE2 (short_lim 0) every call leaves the kernel's first test, and one
`cmpl $4032, %ecx` is the level and in-page test. Next's `.Lmemchr_sse_pre` does it at the target of that first
branch (32-byte aligned, no level load, one taken branch). Run 3's version did it one jump later at `.Lmemchr_entry`
(two taken branches, same test). **Kept: next's entry and next's layout whole** (its edge block, no `.org 0x25e/0x3ba`
pads: those pinned run 3's layout and have no meaning in next's). **Re-applied: only the n <= 16 early return**
(`cmpq $16, %rdx; jbe .Lmemchr_none` at `.Lmemchr_sse_head16`). Code diff vs next: those two lines and a comment.
With next's entry already merged, nothing pays the test back: long SSE2 calls with a block-0 miss run 2 more
instructions (D-4.57 rewritten to say so).

Counts, job mei:timing-cov-memchr-short-nonavx2-4-counts (instr/op, `--compare --counters`, base = head minus
the two lines = next's memchr code, built in RAM):
| row | SSE2 next | SSE2 head | SSE2 glibc | AVX2 next | AVX2 head |
|---|---|---|---|---|---|
| memchr_16 | 44.3 | 29.3 | 29.3 | 25.3 | 25.3 |
| memchr_16_mix | 51.3 | 36.3 | 40.0 | 32.3 | 32.4 |
| memchr_64 | 40.3 | 42.3 | 54.3 | 29.3 | 29.3 |
| memchr_64_mix | 47.4 | 49.4 | 66.2 | 36.3 | 36.4 |
| memchr_4k | 965.9 | 967.8 | 1157.9 | 363.6 | 363.6 |
| rawmemchr_4k | 938.0 | 938.0 | 1032.8 | 347.6 | 347.6 |
| memrchr_64_hit | 31.0 | 31.0 | 28.0 | 27.0 | 27.0 |
AVX2 rows: unchanged (0.1 is the counter's resolution; the AVX2 level never reaches head16).

Timing: kotori:timing-cov-memchr-short-nonavx2-4-time (Ivy Bridge, AVX2 off, 30 rotated alternations
next/head/glibc, rows4.sh), ns per call, low / high clock mode (min / median of 30):
| row | next | head | glibc | head vs next |
|---|---|---|---|---|
| memchr_16 | 7.63 / 11.93 | 6.03 / 9.42 | 4.82 / 7.54 | -21% |
| memchr_16_mix | 8.43 / 13.18 | 6.83 / 10.67 | 7.15 / 11.30 | -19% |
| memchr_64_mix | 8.03 / 12.55 | 8.43 / 13.18 | 9.20 / 14.50 | **+5.0%** (one quantum, both modes) |
| memchr_4k | 115.30 / 180.16 | 107.87 / 168.55 | 136.10 / 210.57 | -6.4% |
| rawmemchr_4k | 134.05 / 209.45 | 134.03 / 209.45 | 123.03 / 192.23 | 0 |
| memrchr_64_hit | 4.42 | 4.42 | 3.46 | 0 |
memchr_64_mix is slower beyond the spread (quantised values; head never reached next's 12.55 in high mode). It runs
2 more instructions (49.4 vs 47.4) and stays 9% faster than glibc: the trade ruling #3579 accepted against run 3's
base, now measured against next's entry. Verdict INCOMPLETE + RULING NEEDED: the coordinator's bar for COMPLETE was
no row slower beyond the spread.

rawmemchr_4k (+1.3% from MEMREF-ENTRY, coordinator note 2): not investigated in this run (budget); this story does
not touch rawmemchr (identical counts; timing above).

Jobs on the final code (d5dad9bc; later commits are handoff/ and the D-4.57 text): mei:timing-cov-memchr-short-nonavx2-4-lint
(clippy -D warnings + static, rc 0), mei:...-plants (4 CAUGHT, 1 EQUIVALENT: n16-test-removed, the test is
performance only), mei:...-gate (still running at hand-back, started 13:16Z on d5dad9bc's synced tree; the dispatcher reads its row), mei:...-counts, kotori:...-time.

## Next step
Ruling: accept memchr_64_mix +5% for memchr_16 -21%, memchr_16_mix -19%, memchr_4k -6.4% (recommend accept and
land: SSE2 only, still faster than glibc, memchr_16 at glibc parity in instructions). If rejected: remove the two
lines (the branch then equals next in code) and record D-4.57 as REJECTED with this table. Still open from note 2:
rawmemchr_4k +1.3% after MEMREF-ENTRY (look at where the rawmemchr SSE2 loop sits).
Plan rows to read: D-4.56, D-4.57 (s04), D-4.45, D-4.48.
