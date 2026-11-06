# TIMING-IVB-STRSCAN-DISPATCH (handoff; -2 continuation: timing)

State: branch task/timing-ivb-strscan-dispatch-2 (= task/timing-ivb-strscan-dispatch + handoff), code commit 9b6419cc
(csys/src/machine/scan.S only); later commits handoff/ only. -2 role keke:~/w/n1 (lint job timing-ivb-strscan-dispatch-2-lint2 rc 0 on the final head: clippy -D warnings, xtask static).
Run 1 role mei:~/w/s2: gate timing-ivb-strscan-dispatch-gate rc 0, plants timing-ivb-strscan-dispatch-plants2 7/7 CAUGHT.

## Change (csys/src/machine/scan.S)
- strchr / strchrnul: the entry's `jae` (taken on every SSE2-class call) lands directly on the SSE2 64-byte head,
  aligned to 32 bytes. Before: entry `jae` to a far block, a second mask load (`liella_machine_schr_mask`) and a
  second taken branch back to the head. The head's own compare (ecx < 4032) separates SSE2 in-page calls; AVX2
  page-end and NULL calls leave through the existing NULL test. `liella_machine_schr_mask` and its setter are removed.
- strncmp: `.Lsnc_pt` is 32-aligned; the SSE2 level falls through the page test into the SSE2 steps (before: a taken
  `jae` to the adjacent `.Lsnc_old`); the AVX2 test of b moved out of line (`.Lsnc_ptb`).
- AVX2 level cost: a page-end call now runs `cmpl $4032; jae` (+2 instructions, one load fewer); no bench row reaches it.

## Instruction counts (run 1, Skylake-SP host, Liella/glibc; before = next ff125cce)
| row | AVX2 before/after | SSE2 class before/after |
|---|---|---|
| strchr_16_hit | 1.077 / 1.076 | 1.063 / 1.000 |
| strchr_16_miss | 1.077 / 1.077 | 0.750 / 0.714 |
| strchr_64_hit / _miss | 1.029 / 1.029-1.030 | 1.036 / 1.000 |
| strchrnul_16_hit | 1.042 / 1.041 | 1.069 / 1.000 |
| strchrnul_16_miss | 1.042 / 1.042 | 0.736 / 0.698 |
| strchrnul_64_hit / _miss | 1.000 / 1.000 | 1.038 / 1.000 |
| strncmp_eq16 / diff8 / names64 | 1.167 / 1.133 / 1.077 unchanged | 1.015 / 1.436 / 1.072 unchanged |

## Timing (-2)
Method: `cargo xtask bench --stage` on keke:~/w/n1 (XTASK-BENCH-STAGE), two trees differing ONLY in scan.S:
before = next 730f416b (manifest tree 40734fe8), after = next + this branch's scan.S (scratch commit f4235d69, not
pushed; scan.S byte-identical to this branch's head, tree b6d73ecf). Both tars copied to the booked hosts (nico #47
Ivy Bridge E3-1220L v2, nozomi #48 N100; cancelled after) and run under ~/bin/timing by
`run.sh 5 3`: `./xtask-bin bench --no-build --staged --counters --runs 5 --rows <11 rows>`, alternating
before/after three times (15 paired rounds per row). Analysis: handoff/TIMING-IVB-STRSCAN-DISPATCH-2-analyze.py
(L after/before = median of paired per-round Liella ns ratios; glibc after/before is the drift check).
Jobs: nico:timing-ivb-strscan-dispatch-2-nico rc 0 (12 min, load ~1), nozomi:timing-ivb-strscan-dispatch-2-nozomi rc 0.

Ivy Bridge (SSE2 class) - every row faster, none slower:
| row | L/g time before -> after | Liella after/before | glibc drift | instr L/g before -> after |
|---|---|---|---|---|
| strchr_16_hit | 1.318 -> 1.116 | 0.847 | 1.000 | 1.063 -> 1.000 |
| strchr_16_miss | 1.164 -> 0.919 | 0.789 | 1.000 | 0.750 -> 0.714 |
| strchr_64_hit | 1.376 -> 1.135 | 0.826 | 1.000 | 1.036 -> 1.000 |
| strchr_64_miss | 1.469 -> 1.287 | 0.875 | 0.999 | 1.036 -> 1.000 |
| strchrnul_16_hit | 1.557 -> 1.224 | 0.786 | 1.000 | 1.069 -> 1.000 |
| strchrnul_16_miss | 1.000 -> 0.750 | 0.750 | 1.000 | 0.736 -> 0.698 |
| strchrnul_64_hit | 1.535 -> 1.267 | 0.826 | 1.000 | 1.038 -> 1.000 |
| strchrnul_64_miss | 1.439 -> 1.187 | 0.825 | 1.001 | 1.038 -> 1.000 |
| strncmp_diff8 | 1.752 -> 1.585 | 0.905 (spread 0.24, bimodal host) | 1.000 | 1.436 unchanged |
| strncmp_eq16 | 1.265 -> 1.159 | 0.917 | 1.000 | 1.015 unchanged |
| strncmp_names64 | 1.000 -> 0.968 | 0.968 | 1.000 | 1.072 unchanged |
Cycles ratios match the time ratios within 0.01 (counters.tsv of the same runs).

N100 (AVX2, no-regression check): all 11 rows Liella after/before 1.000, glibc drift 1.000, instructions identical
(strchr 0.73-0.91, strchrnul 0.70-0.82, strncmp 0.85-1.00 of glibc's time). keke smoke run (Zen 1, AVX2, 1 round,
loaded, not a verdict): after/before 0.99-1.00.

Target (time <= 1.05 on the 8 strchr/strchrnul rows) NOT reached: the four misses are at or below glibc
(0.75-0.92), the hits remain 1.12-1.29 at instruction parity (1.000), so the rest of the gap is IPC, not count.

## Open / next step
Verdict: lands (faster on every Ivy Bridge row, no change on the AVX2 class). Not to redo: staging, timing, counts.
1. Ivy Bridge strchr/strchrnul hit rows (1.12-1.29 time at instruction parity): profile IPC on the booked host
   (perf stat idq_uops_not_delivered / resource stalls) of the SSE2 head; candidates: the head's four 16-byte blocks
   spanning 16-byte fetch windows, the hit path's bsf/tzcnt-to-result latency. Stage with `bench --stage` as here.
2. strncmp_diff8 (1.436 instructions, 1.585 time, SSE2) is the largest strncmp gap; strncmp_eq16 (1.159 time): the
   D-4.21 deciding-byte re-read's load-use latency (decide from the loaded vector instead).
3. Ivy Bridge runs of strncmp_diff8 are bimodal (spread 0.24 of the paired ratio, as TIMING-NSCACHE-FIRSTCALL saw).
