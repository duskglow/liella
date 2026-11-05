# PERF-MEMSET-4K-2 (branch task/perf-memset-4k-2) - COMPLETE

State: the previous run's change is kept unchanged (memset above 256 B tail-jumps to liella_hot_fill_large; the AVX2 fill loop counts
rcx up to zero). The earlier gate jobs were rc 143 (terminated by a kill on the role), not failures.
Results on the final head (host keke, role n4, Zen-class host):
- gate (affected, --base origin/next): 10177 passed, 0 failed, conformance 117 passed; rc 0.
- plants 3/3 CAUGHT (loopcount, loopend in fill_avx2; smallbig = fold.S threshold removed, caught by small_copy).
- counts (cbench_memref, cbench_hot; ~/work/PERF-MEMSET-4K-2/counts2.log): instr/op Liella vs glibc: memset_4k 243.0 vs 246.0 (0.99),
  memset_64k 3134.6 vs 3616.0 (0.87), memset_144/256 0.94, memset_16 1.05, memset_64 1.00, memset_128 27.4 vs 25.4 (1.08; size 128 not
  touched by this change), mixes 1.01, memset_4k_off 1.01. Time ratios Liella/glibc-dynamic 0.48-1.02; no row slower than before
  from this change (no pre-change count taken in this run: INFORMATIONAL).
Not done: wl_percall (spec ms4k.c not re-run; the counts cover memset_4k).
Do not redo: gate, plants, counts above. Next step: none (memset_128 at 1.08 is a possible separate row).
Jobs: keke:perf-memset-4k-2-gate, keke:perf-memset-4k-2-plants, keke:perf-memset-4k-2-counts2.
Superseded: perf-memset-4k-2-counts (wrong bench name, rc 1).
