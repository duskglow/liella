# PERF-STRSTR-HIT-4K

State: branch task/perf-strstr-hit-4k. Kernel change only: `liella_machine_strpair` (csys/src/machine/scan.S) tests 128 bytes per
iteration (four blocks, `vptest`, add before the test) instead of 64 (19 instr per 64 bytes).

Counts (validation/percall/strstr_cycles.c, instructions:u per call, Zen 1 host, no AVX-512; labels strstr-hit-4k-percall-before / perf-strstr-hit-4k-percall2):
| row | before | after | glibc |
| strstr_scan_4k | 1299 | 1144 | 830 |
| strstr_verify_4k | 1299 | 1144 | 18536 |
| strstr_miss_4k | 1281 | 1126 | 10158 |
| strstr_hit_4k | 1549 | 1394 | 10167 |
| strstr_hit_short | 173 | 173 | 153 |

Finding: the brief's hit_4k row (Liella 1278 vs glibc 826) does not match this spec's text-like hit_4k (glibc 10167, Liella
already 0.14x). The row that matches the brief's glibc figure is strstr_scan_4k (clean scan): ratio 1.57 -> 1.38. Parity (<= 1.05) is NOT
reached. Two streams (first and last needle byte) per block cost 28 instructions per 128 bytes (0.22/byte); glibc's 0.2/byte total
implies a one-stream filter. A one-stream filter on the last byte (about 18 per 128) stops at every common last byte in text and would
make the text rows slower, so it was not built.
Plants: p1 (drop block 4 from the combine), p2 (no rewind on a hit), p3 (wrong load), p4 (wrong step), p6 (wrong offset) must be CAUGHT;
p5 (weaker filter) EQUIVALENT: extra candidates are rejected by the Rust compare.
Next step (if wanted): a RULING on a one-stream first-byte-adaptive kernel (pair filter only after the first false stop), measured against all strstr rows.
Do not redo: AVX-512 (host has none); a 4x unroll beyond 128 (loop overhead is already 4 of 28).
Saturation row: docs/saturation/03-memory-and-string-kernels.md.

## Run 2 (PERF-STRSTR-HIT-4K-2)
Ruling #2521 (owner): strstr_scan_4k closes at 1.05x of glibc as the parity target is not reachable without slowing text rows; the
pair-filter kernel (128 bytes per iteration, 1144 vs 830 instr) stays. No code change in this run.
Cause of the plants2 rc=124: p4_step (step 96 instead of 128) makes the mutant loop forever. New plants script wraps each test
run in `timeout -k 10 600` and counts rc 124 as CAUGHT (by timeout); build is separate (`--no-run`). Plants p1..p4, p6 rerun on the final head
in RAM copies; p5 EQUIVALENT (weaker filter, Rust compare rejects extras).
Jobs: perf-strstr-hit-4k-2-gate (affected, base origin/next), perf-strstr-hit-4k-2-plants. clippy and static clean on the role.
Next step: none.

## Run 3 (PERF-STRSTR-HIT-4K-3)
State: branch task/perf-strstr-hit-4k-3, merge of origin/next aac6956b0 (commit 79cf2a92) plus this handoff commit.
CI conflict cause: next had also landed a 128-byte group loop in liella_machine_strpair (memory operands, vpmovmskb+test+jnz at
the top, sub+jmp at the bottom: 26 instr per 128 B). Resolution combines both: next's memory-operand blocks (no separate loads)
plus this branch's loop bottom (step, then one vptest+jz; hit = step back + jmp to the block loop; 64-byte pair out of line):
23 instr per 128 B. Behaviour on a hit is the same on both sides (the run resumes in the block loop at the group's start).
No other file conflicted (docs/saturation/03 auto-merged).
Counts (perf-strstr-hit-4k-3-percall3, instructions:u per call, Zen 1 host; base = origin/next aac6956b0):
| row | glibc | next | this branch | ratio |
| strstr_scan_4k | 830 | 1073 | 1014 | 1.22 |
| strstr_verify_4k | 18536 | 1073 | 1014 | 0.05 |
| strstr_miss_4k | 10158 | 1055 | 996 | 0.10 |
| strstr_hit_4k | 10167 | 1323 | 1264 | 0.12 |
| strstr_hit_short | 153 | 173 | 173 | 1.13 (unchanged) |
No row is slower. strstr_scan_4k stays closed under ruling #2521 (1.05x not reachable without slowing text rows).
Plants (perf-strstr-hit-4k-3-plants, keke:~/work/PERF-STRSTR-HIT-4K-3/plants.sh, RAM copies under /dev/shm, each test run
`timeout -k 10 600`, patterns anchored after the loop128 label, a pattern that does not apply fails the run): baseline ok;
CAUGHT p1_dropblock4, p2_noreset, p3_last96, p3b_nul96 (SIGSEGV in strstr_find: missed NUL runs into the guard page),
p4_step (by timeout), p6_offset96, p7_pairstep (loop64 step 96); p5 weaker filter EQUIVALENT (Rust compare rejects extra candidates).
Superseded: perf-strstr-hit-4k-3-percall (rc 2, --base sha run from ~/x, an empty repo), perf-strstr-hit-4k-3-percall2 (rc 2, base
not primed on keke; primed with validation/wl_prime_base.sh keke aac6956b0).
Next step: none (CI merge of next).
