# TIMING-COV-STRSTR-NONAVX2 handoff (strstr 64 B / 4 KiB on the non-AVX2 class)

State: branch task/timing-cov-strstr-nonavx2 (from origin/next c101c4c6, which holds TIMING-N100-STRSTR-64). Code commit
22cd8586 (csys/src/machine/scan.S, liella_machine_strpair SSE2 path); 2nd commit anchors the plants file. Role shiki:~/w/sh2.
No new D-row (local kernel change; the scanner contract and strstr behaviour are unchanged; header comment updated).
Report: handoff/reports/TIMING-COV-STRSTR-NONAVX2.json. Plants: handoff/TIMING-COV-STRSTR-NONAVX2-plants.txt.

## Step 1: the Ivy Bridge sweep, reproduced (job timing-cov-strstr-nonavx2-counts-before2, shiki Haswell-EP, next c101c4c6)
Liella's dispatch reads only AVX2 (plus ERMS/FMA, which strstr does not use): Ivy Bridge runs the same Liella build as any
host with `glibc.cpu.hwcaps=-AVX2`. On shiki with -AVX2, Liella / glibc instructions per call: hit_64 212.5 / 99.4 (2.14),
miss_64 237.5 / 173.4 (1.37), hit_4k 2292.9 / 2243.6 (1.02), the sweep's IVB ratios (2.169 / 1.387 / 1.023). The glibc
count on shiki is the same with no mask, -AVX2, and an Ivy Bridge-like mask (-AVX2,-AVX,-FMA,-BMI1,-BMI2,-LZCNT,-MOVBE,-F16C).
The Zen 1 probe's lower ratios (1.08 / 1.53) come from glibc's count under -AVX2 on Zen 1 (204 / 409 / 1506), not from
Liella. Settled: count this class on any Intel build host with -AVX2.

## Change (22cd8586)
Profile before: strpair ~133 of the hit_64 call. It ran a 16-byte head block (two masks, two branches), two aligned 16-byte
blocks of 19 instructions, one 64-byte group, then the same group again block by block.
After: every SSE2 block uses the group test min((Y ^ c1) | (L ^ c0), Y) == 0 (one mask, one branch; the stop byte says 0 =
terminator, else candidate). When p's page has 64 bytes from p on (offset <= 4032), four unaligned head blocks cover
[p, p + 64), then the aligned groups from the 64-byte boundary in (p, p + 64]; otherwise one head block (the old page rule).
Aligned blocks address p - off by a fixed -off register (r9). The group loop is rotated (one taken branch per 64 B).
The loads are the same kinds, within the same ranges. The AVX2 path is untouched.

## Counts (cargo xtask bench --compare --counters --runs 3 cbench_memref, shiki; -AVX2 = Liella non-AVX2 builds)
| row (-AVX2) | before | after | glibc | after/glibc |
|---|---|---|---|---|
| strstr_hit_64 | 212.5 | 154.4 | 99.4 | 1.55 |
| strstr_miss_64 | 237.5 | 157.4 | 173.5 | 0.91 |
| strstr_hit_4k | 2292.9 | 2219.6 | 2244.0 | 0.99 |
| strstr_miss_4k | 2318.2 | 2236.3 | 6449.1 | 0.35 |
AVX2 (default) rows unchanged (125.5 / 128.4 / 651 / 911). The ns columns on shiki were taken under gate+plants load
(the glibc side moved too) and decide nothing. A one-off reading: -AVX2 hit_4k time ratio was 1.05 before and 1.07 after,
with glibc also slower in that run. Watch this row in the booked timing.

## Tests and plants
Existing strstr_find covers step 3 already: needles 0..70 x haystacks 0..300 x match positions x 32 alignments (sweep),
the same with the NUL on the last mapped byte (page end: every start offset across the 4032/4064 thresholds), page cross
from the last 40 bytes, and the exact strpair stop at 0..159 x 34 alignments x 4 distances, both paths. No new test was
needed: plants -> timing-cov-strstr-nonavx2-plants2: 7 of 7 CAUGHT, no WARN. The plants: skip the last head block, stop byte
read one late, pair block one byte short, wrong block offset, page test removed (SIGSEGV), group rewind dropped, units-path
-off sign. (-plants was refused because 3 plants had no function anchor; superseded.)

## Jobs (shiki unless named)
counts-before2 rc 0 (counts-before rc 1: `--rows` is staged-only, superseded); counts-after rc 0; plants2 rc 0 (7/7);
gate `timing-cov-strstr-nonavx2-gate` (~/bin/gate 16G: clippy, static, strstr_find, affected --run --base origin/next):
clippy, static and strstr_find green at hand-back, affected still RUNNING. stage2 rc 0 (stage of cbench_memref; stage rc 1:
`--rows` only takes budgets.tsv rows, superseded). Timing: booking 73 (nico, 45 min) and job
nico:timing-cov-strstr-nonavx2-time (`~/bin/timing ... bash ~/work/TIMING-COV-STRSTR-NONAVX2/time.sh`: staged cbench_memref,
--counters --runs 7, out in ~/work/TIMING-COV-STRSTR-NONAVX2/out/) QUEUED behind the drain at hand-back.

## Next step
1. Collect: shiki `~/bin/jobwait timing-cov-strstr-nonavx2-gate`; nico `~/bin/jobwait timing-cov-strstr-nonavx2-time`, then
   read strstr_* rows (time ratio vs sweep 2.398 / 1.411 / 1.277). Land only if no strstr row is slower in time.
2. hit_64 is still 1.55 instr: the rest is the Rust entry (~77: frame push/pop, needle loop 4/byte, stack-returned KResult).
   TIMING-N100-STRSTR-64's leaf fast path for short needles (values kept in caller-saved registers) serves both classes.
Not to redo: the SSE2 head/loop shape (this story), the kernel AVX2 head (TIMING-N100-STRSTR-64).
