# PARITY-COPY-AVX2 (continued by -2 and -3): handoff

State: branch `task/parity-copy-avx2-3` (continues `task/parity-copy-avx2-2`, `task/parity-copy-avx2`), from origin/next
f8b77d70. Code commit 1e8ffb15 (AVX2 copy: held 128-byte end block); handoff/plants commits 9453c446, c0bff978; docs
wording a86b97cb; -2 handoff 725f3ca3; -3: register row 10 with the measured counts, this handoff, report.
Verdict: COMPLETE. Measured the change, no ladder size rose, 1e8ffb15 kept.

## Done (all runs)
1. Counter probe: `rep movsb`/`rep stosb` = ONE instruction on Skylake-SP, Haswell-EP, Ivy Bridge (docs/benchmarks.md 2a,
   register 03). Ruling #3046 / owner 11-05: rep-string copy rows are judged by timing on a booked host (same-host ratio);
   vector-vs-vector rows are gated on instructions, verdict from the timing sweep. a86b97cb aligns 2a and register row 9.
2. Profile of a 1 KiB AVX2 copy (by reading, 170 vs 137 Haswell-EP): entry/dispatch 25-30; overlap/direction 15; alignment
   setup 10; 128-byte loop 77; 32-byte tail loop 14 (the pure-overhead phase); ends 5.
3. Change 1e8ffb15: each direction loads the 128-byte block at the end it runs towards before any store, stores it last;
   the 32-byte loop is gone. Kernel contract n >= 128 (all production callers pass >= 128). SSE2 path untouched.
4. Percall (-3), keke (Zen 1, 60 vCPU, no ERMS: every 257 B-64 KiB copy runs the AVX2 kernel), instructions:u per call,
   sd 0, base f8b77d70 vs new 725f3ca3 (product = 1e8ffb15's), memcpy and memmove identical:

   | size | glibc | base | new |      | size  | glibc | base | new  |
   |------|-------|------|-----|------|-------|-------|------|------|
   | 16   | 23    | 24   | 24  |      | 256   | 40    | 44   | 44   |
   | 31   | 23    | 24   | 24  |      | 1024  | 135   | 170  | 162  |
   | 32   | 24    | 24   | 24  |      | 4096  | 423   | 434  | 426  |
   | 33   | 24    | 26   | 26  |      | 16384 | 1575  | 1490 | 1482 |
   | 64/65| 24/32 | 26/34| 26/34|     | 65536 | 6183  | 5714 | 5706 |
   | 128/129 | 32/40 | 34/44 | 34/44 | |      |       |      |      |

   -8 at every kernel size, nothing higher. Predicted -9 (by reading); measured -8. The +3 sizes (n mod 128 in 1..32 past
   the aligned start, e.g. 385) are not in the ladder and were not measured.

## Jobs
- keke:parity-copy-avx2-3-percall rc 0 (this run, head 725f3ca3; the -3 commits touch docs/ and handoff/ only).
- keke:parity-copy-avx2-3-lint: clippy --all-features -D warnings + cargo xtask static on the final head (this run).
- keke:parity-copy-avx2-gate2 rc 0 (direct tier, tree 9453c446, product identical); kanon:parity-copy-avx2-2-plants rc 0,
  6/6 CAUGHT (handoff/PARITY-COPY-AVX2-plants.txt).

## Next step (optional follow-ups, not blocking landing)
1. Timing of 1e8ffb15 on a booked AVX2 timing host (3 extra loads/stores at entry change the mix), or leave it to the sweep.
2. Measure a +3 size (385, 1153) to confirm the expected cost; add such a size to avx2_families.c if wanted.
3. Skylake-SP count of memmove_4k (436.8 vs 413.0; expect about 428).
4. 1 KiB still 27 over glibc: profile the entry/dispatch chain (25-30) with wl_trace.sh.

## Not to redo
The rep-string probe; the plants; gate2; the percall above (product unchanged).
Process slip (run 1): one line of scan.S was edited with `sed -i`; content reviewed, correct.
