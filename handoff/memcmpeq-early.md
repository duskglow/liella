# PERF-MEMCMPEQ-EARLY (branch task/perf-memcmpeq-early)

State: one code change in csys/src/machine/fold.S `.Lcmp_wide` (memcmp 33-64 AVX2): the first block's mask is tested
before the AND; the last block's mask only when the first is equal. Register row added in docs/saturation/03.

Counts (keke, cbench_memref, instr/op, Liella after vs glibc): memcmp_diff8_64 34.4 -> 30.4 (glibc 24.4, 1.25x);
every other row of the brief unchanged (tail_32 31.4, head_1k 30.4, head_128 28.4, diff8_96/127 33.4, memcmp_4k 671.3).
No row of the bench got more instructions (eq rows, memcmp_64/128/16 identical or lower; checked in the counts-after log
~/work/memcmpeq-after.log on keke). Parity (1.05x) NOT reached on any row.

Why the rest did not move (hypothesis partly wrong: not the exit, the entry):
- 65-2048 equality classes: 4 instr of NULL tests + 4-6 of class dispatch before the first compare; glibc has neither.
- tail_32: 32 bytes uses the SSE2 blocks on purpose (MEMCMP-MID-4: AVX2 blocks cost 5 more cycles); time 0.60-0.70 of glibc.
- diff8_96/127: the 65-128 compare class was not reworked (budget).

Next step (RULING NEEDED first): extend NULL folding (no explicit NULL test, first load faults on page zero) to the
65-2048 AVX2 classes of the compare and equality entries; saves 4 instr on those rows. Then try a probe-less 33-64 compare
(vector first; would take diff8_64 to about 26 but adds 4 instr to head-difference rows: check them before keeping).
Do not redo: the first-mask-first reorder (done).
Jobs (queued on keke at hand-back): perf-memcmpeq-early-gate, -plants, -percall; counts-after done rc 0.
Plants (in ~/work/memcmpeq-plants.sh): three mutants in .Lcmp_last / .Lcmp_first.
Note: a docs-only commit follows the synced tree (code identical).
