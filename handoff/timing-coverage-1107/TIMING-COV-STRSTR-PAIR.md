# TIMING-COV-STRSTR-PAIR: strstr on 64-byte haystacks executes 1.26-1.55x glibc's instructions

Id: TIMING-COV-STRSTR-PAIR
Kind: code
Priority: 60
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Stacks-on: none

## The change
Cut the instruction surplus of strstr on short haystacks (hit at 64 bytes on both classes; miss_64 on Ivy Bridge).
TIMING-N100-STRSTR-64 and TIMING-COV-STRSTR-NONAVX2 are in the build (read their handoffs and D-rows first).

## Rows (1106E; instr / time IVB, instr / time N100)
strstr_hit_64 1.555 / 1.675, 1.262 / 1.350; strstr_miss_64 0.908 / 1.087 (IVB; N100 parity); strstr_hit_4k 0.989 /
1.104 IVB with spread 0.929 (noise: re-time it, do not target it).

## Evidence (build host, region counters per call, prof2 in counters-mei.txt)
Default: strstr_hit_64 125.4 vs 99.4 instructions, 36.7 vs 27.8 cycles; strstr_miss_64 128.4 vs 173.6.
-AVX2: strstr_hit_64 154.4 vs 99.4, 38.7 vs 27.7; strstr_miss_64 157.4 vs 173.4, 42.7 vs 44-60.
Liella samples: liella_machine_strpair 50% (default) / 79% (-AVX2), strstr 11-17%.

## Hypothesis
The hit row pays the pair-candidate scan's setup before the first verify (instructions above glibc only on the hit,
below it on the miss). Look at where strpair spends instructions until the first candidate on a 64-byte haystack.

## Tests and plants
strstr correctness sweeps (needle lengths 1-n, every alignment, near-page-end) on the direct tier; plants on the
changed path.

## Rows to time
cbench_memref strstr_* and cbench_patterns rows that use strstr, both classes.
## JOBS
Gate: `cargo xtask clippy`, `cargo xtask static`, then the direct tier (`cargo xtask affected --run --base origin/next`)
through jobrun on your role. Plants: `cargo xtask plants <file>` on every changed function (msg: regex per plant).
Counts before/after on base and head (region counters or the probe, both sides printed). Timing: book an Ivy Bridge
and an N100 host (`ops timing-book`), stage once with `cargo xtask bench --stage`, run the rows below base vs head
vs glibc interleaved (>= 3 rounds), cancel the bookings when done. Keep only a change that makes no measured row slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
