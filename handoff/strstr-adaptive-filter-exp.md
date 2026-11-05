# STRSTR-ADAPTIVE-FILTER-EXP (experiment, ruling #2521; not for landing without data and an owner ruling)

State: branch exp/strstr-adaptive-filter (from origin/next db50ef41, which contains PERF-STRSTR-HIT-4K-3). Role keke:~/w/n2.
Change (AVX2 path of `liella_machine_strpair` only; SSE2, wcspair and memmem are unchanged):
- The head block also records whether it holds c1 (the last needle byte of the pair). No c1 there: the 128-byte
  groups are scanned in ONE stream (c1 or 0: min(Y ^ c1, Y) == 0), 16 instructions per 128 bytes, no q - off load.
  c1 seen there, or a page-end start (unit path): the pair filter as before (23 per 128).
- A one-stream stop at group G: G - (last stop, initially p) >= 1024 -> sparse: the block loop resolves G, and the run
  comes back through `avx_mode`. < 1024 -> dense: pair filter for the rest of this call, from G's start (nothing skipped).
- Design source: the first-byte / pair filter idea of the published SIMD substring-search literature (filter on one or
  two needle positions, verify candidates); the threshold is from this kernel's own costs (a false sparse stop costs about
  50 instructions, the one-stream loop saves 7 per 128 bytes, break-even about one stop per 900 bytes).
- New test `strstr_filter_switch_keeps_every_candidate` (strstr_find.rs): one decoy c1 at 4 distances, the match at every
  gap 1..1300 after it, 3 alignments, AVX2 and SSE2. New percall row `strstr_dense_4k` (clean 200 bytes, then c1 every 2 bytes).

Counts so far (wl_percall, instructions:u per call, Zen 1 host, new head 70f1194b; from the plants job's unplanted copy):
| row | glibc | before (next, PERF-STRSTR-HIT-4K-3 figure) | after | after/glibc |
| strstr_scan_4k | 818 | 1014 | 722 | 0.88 |
| strstr_dense_4k | 818 | n/a (new row) | 1042 | 1.27 |
scan_4k reaches parity (was 1.22x). The full before/after table (every strstr_cycles row base vs new, cbench_memref
strstr/strcasestr/memmem rows before/after, default and hwcaps=-AVX2) is the job `saf-exp-counts`; logs on keke:
~/work/STRSTR-ADAPTIVE-FILTER-EXP/{percall,bench-after,bench-after-noavx2,bench-before,bench-before-noavx2}.log.
NOT VERIFIED here: the text rows (miss_4k, hit_4k, verify_4k, hit_short, cbench_memref). Expected cost on text: the head
almost always holds c1, so the pair path runs, plus 2 instructions per call that passes the head (vpmovmskb, mov); a call
that returns inside the head pays nothing extra.

Plants (saf-exp-plants, keke:~/work/STRSTR-ADAPTIVE-FILTER-EXP/plants.sh, RAM copies under /dev/shm, own target each):
baseline rc 0; p1_switchdrop (the dense switch skips the group) CAUGHT; p2_norewind (a stop does not step back to the
group) CAUGHT; p3_direction (jae -> jb: switch when sparse, stay one-stream when dense) CAUGHT by count: strstr_dense_4k
1042 -> 2665 instructions (> 1.3x). The threshold direction is not observable by results (both paths are exact), so it
is a count plant by design.
Gate: saf-exp-gate (affected, --base origin/next) rc 0: 10 suites, 10153 passed, 0 failed; conformance 116/0; peak 12G. clippy, static and strstr_find (19 passed) clean on the role.
wl_count: not run (budget); the workload set's strstr callers still need it before any landing.

Recommendation: decide after saf-exp-counts. Land (owner ruling) only if no text row or cbench_memref row is slower beyond
noise; strstr_dense_4k (1.27x of glibc) is a new row with no before. If any text row is slower, record the negative in
docs/saturation/03-memory-and-string-kernels.md (strstr row) with these numbers.
Open: memmem has its own kernel and was not given the switch; the -AVX2 path is unchanged (its rows only confirm no change).
Next step: read saf-exp-counts' logs, fill the table, run wl_count for the strstr workloads, then the ruling.
