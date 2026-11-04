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
