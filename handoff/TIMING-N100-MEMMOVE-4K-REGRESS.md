# TIMING-N100-MEMMOVE-4K-REGRESS: handoff (one page)

State: branch `task/timing-n100-memmove-4k-regress` from origin/next eef6b2dd. Code commit c18471c5 (AVX2 copy: no held
128-byte head block in the backward path); this handoff, the plants file, register row 10 (03) and the report follow.
Verdict: INCOMPLETE. The fix is measured and the plants pass, but the direct-tier gate has one failure that is not yet
explained (see Gate).

## Cause (verified, not assumed)
- File-level diff: between f2157504 (1105 sweep) and ff125cce^ no copy code changed (strcmp and math only); ff125cce
  (PARITY-COPY-AVX2-3) changed the AVX2 copy kernel `liella_machine_move_avx2` (scan.S). On the N100 (FSRM) every
  257 B-64 KiB copy runs that kernel (no rep movsb window); memmove_4k is `memmove(d + 8, d, 4096)`: the backward path.
- Counts, Haswell-EP build host, Liella/glibc instructions (jobs counts3, stage2): memmove_4k f2157504 1.003,
  ff125cce^ 1.002, ff125cce 0.976, next eef6b2dd 0.976; memcpy_4k 0.998 / 0.997 / 0.984 / 0.984; memmove_64k 0.916 /
  0.915 / 0.914 / 0.914. The landing lowered instructions: the regression is not in the count.
- Profile (scratch kernel microbench, ~/work/TIMING-N100-MEMMOVE-4K-REGRESS/mb on shiki and nozomi; kernels before
  and after the landing, a hybrid, two store-order variants and glibc, interleaved, min of 9, TSC ticks/call; N100 job
  nozomi:timing-n100-memmove-4k-regress-mb2-n100): new/old 4 KiB backward 1.175 (dst+32), 1.179 (dst+64), 1.160
  (disjoint, d = 1 KiB), 1.034 (bench placement dst+8); 1/16/64 KiB backward 0.97-1.00; forward 4 KiB 0.99, 1 KiB 0.97,
  forward overlap src = dst + 8 4 KiB 1.033. Held-block stores descending or tail first: same as new (1.17). The
  hybrid (new forward, old backward): 0.994-1.007 of old on every backward row, forward as new.
  Mechanism inside Gracemont not established (not store forwarding: the disjoint backward copy pays the same).

## Change
Backward path: 128-byte steps, then 32-byte steps, holding only the first and last 32 bytes (the pre-landing backward
shape). Forward path unchanged (held 128-byte end block). Plants 5/5 CAUGHT (handoff/TIMING-N100-MEMMOVE-4K-REGRESS-plants.txt).

## Timing, staged trees (cargo xtask bench --stage --only cbench_memref), N100, 3 rounds interleaved, --runs 3
nozomi:timing-n100-memmove-4k-regress-n100, booking 79. Time ratio Liella/glibc, before (next eef6b2dd) -> after (c18471c5):
memmove_4k 1.085/1.080/1.085 -> 0.971/0.966/0.971 (1105: 0.965); instructions 1.000 -> 1.036. Unchanged: memcpy_4k
0.476, memcpy_4k_off 1.048, memcpy_64k 0.99, memmove_64k 0.99, memcpy_256 1.077, memmove_256 0.947.
Ivy Bridge (kotori:timing-n100-memmove-4k-regress-ivb, 2 rounds): no AVX2, the kernel is not reached; the job was still
running at hand-back (read its log; expect no difference).
Deviation from the brief: ff125cce^ and ff125cce could not be staged (no `bench --stage` in those trees), so the staged
pair is next before/after this fix; the old-vs-new kernel comparison on the N100 is the interleaved microbench above.

## Gate (shiki:timing-n100-memmove-4k-regress-gate, direct tier, --base origin/next)
One failure: liella-boundary (exports interposed) `search_nomem::tsearch_returns_null_and_leaves_the_tree_walkable`
(the re-executed child exits non-zero under its address-space limit, tests/search_nomem.rs:162). The test file uses no
memmove/memcpy; the non-interposed run passed. Not shown unrelated yet: shiki ran gate, plants and three builds at once
(load ~14). Next: run that test interposed on origin/next and on this branch on a quiet host; re-gate.

## Not to redo
The cause analysis; the store-order variants; the counts of the three commits; the N100 before/after timing.

## Open
INFORMATIONAL: the forward held block costs 3% on a 4 KiB forward overlap by 8 (memmove(d, d + 8, n)) on the N100;
no bench row covers it. Slips: one python edit of the scratch microbench was driven by a heredoc; stage2's count step
failed on a wall-time budget row on the loaded build host (strcmp_16_mix), superseded by stage3.
