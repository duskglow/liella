# PERF-RAWMEMCHR-4K-IVB (handoff)

State: branch task/perf-rawmemchr-4k-ivb from origin/next 957f58bd. Role kanon:~/w/kn1. Commits: 9f4601a3 (scan.S: int3 fill
before `.Lraw_sse_masked`), da5bece6 (plants file), 555bd57f (D-4.63 row + saturation row), then this handoff/report.
**DO NOT LAND as it stands: RULING NEEDED below** (the fix wins on Ivy Bridge and measured slower on the N100 at forced SSE2).

## Cause (found)
TIMING-IVB-MEMREF-ENTRY (ruling #3588) changed rawmemchr's SSE2 entry and moved the 128-byte SSE2 loop `.Lraw_sse_loop`
(`.p2align 4` only) from entry + 0x3a0 (32-byte aligned; before tree e98e113f = first parent of merge 18e2c9d5) to
entry + 0x3d0 (16 mod 32) in next. The 117-byte loop then spans five 32-byte fetch windows instead of four. Same
instruction count. The cause is inside rawmemchr's own code (no link-order effect: memcmp and every later symbol sit where they did).

## Fix (9f4601a3, D-4.63)
16 bytes of int3 after the `ret` that ends the SSE2 head (`.org liella_machine_rawmemchr_entry + 0x338, 0xcc`), so the loop
is at entry + 0x3e0. Never executed: no instruction count changes. rawmemchr ends 26 bytes before memcmp's 64-aligned start
in next, so +16 keeps `nm -n` of libc.so.6 identical apart from rawmemchr's internals (checked). 64-byte alignment (+48)
was not tried: it pushes memcmp by 64 and moves every later function (a confound).

## Timing (staged trees, `xtask bench --no-build --staged --counters --runs 5 --rows` of the 4 rows, side order rotated;
paired per-round Liella ratio median [min-max of per-alternation medians]; glibc drift 1.000 on every row)
| row | Ivy Bridge before/next | Ivy Bridge c32/next | N100 hwcaps=-AVX2 c32/next |
|---|---|---|---|
| rawmemchr_4k | 0.987 [0.875-0.987] (206.83 vs 209.45 ns) | **0.970** [0.969-0.989] (203.10; glibc 192.23) | **1.062** [1.006-1.167] (101.30 -> 110.60; glibc 88.13) |
| rawmemchr_64 | 1.062 | 1.000 | 1.000 |
| memchr_4k | 1.125 | 1.000 | 1.000 |
| strlen_4k | 1.000 | 1.000 | 1.000 |
Ivy Bridge: kotori:perf-rawmemchr-4k-ivb-ivb2 (6 alternations, rc 0). The +1.3% is reproduced exactly (before/next 0.987).
N100: nozomi:perf-rawmemchr-4k-ivb-n100 (4 alternations, rc 0; real N100 runs the AVX2 path, which this change does not touch
on any bench row). The N100 spread is wide (1.006-1.167); 4 alternations only (budget).
perf stat (Liella's cbench_memref only, rawmemchr_4k, Ivy Bridge, kotori:perf-rawmemchr-4k-ivb-ivb1 log): all three layouts
run the loop mostly from the DSB (idq.dsb_uops ~73-80M vs mite ~1M, lsd 2-8M); the whole-program aggregate did not separate
them (iteration calibration differs per run). Timing decided.

## Jobs
kanon: perf-rawmemchr-4k-ivb-stage-{next,before,c32} (rc 0, staged trees), perf-rawmemchr-4k-ivb-plants (rc 0: raw-fill-ret
CAUGHT by SIGTRAP in machine_scanners, raw-fill-byte EQUIVALENT, no WARN; ran on da5bece6 = same code as the final head),
perf-rawmemchr-4k-ivb-gate-final (final content 555bd57f: clippy, static, affected --base origin/next; STILL RUNNING at hand-back).
SUPERSEDED: kanon:perf-rawmemchr-4k-ivb-gate (stopped by me: the doc rows were not yet in the tree);
kotori:perf-rawmemchr-4k-ivb-ivb1 (rc 1: the before tree's older xtask writes its tables into target/liella/bench and fails
the manifest check on the next run; its next/c32 numbers agree: 0.971 [0.970-0.973]). Runner/analyzer:
~/work/PERF-RAWMEMCHR-4K-IVB/{run2.sh,analyze.py} on kotori and nozomi (run2.sh re-extracts each tree per run).

## RULING NEEDED
1. Land D-4.63? Ivy Bridge (a real SSE2-only machine) -3.0% on rawmemchr_4k, other rows flat; N100 at forced SSE2 (not a
   deployed configuration) +6.2%. Recommend: land, the same reasoning as ruling #3588 (memchr_16 +9% on N100 forced SSE2 accepted).
   If not: revert 9f4601a3 and turn D-4.63 into a REJECTED row with these numbers.

## INFORMATIONAL
- To build the "before" tree the brief asked for, I used `git log --oneline --first-parent origin/next` (to find merge
  18e2c9d5) and a detached worktree of e98e113f; I read no diffs of others' commits.
- The `bench --stage` of a pre-XTASK-BENCH-OUTPUTS commit can be run only once per unpack (above).

## Next step
The ruling. If it lands: nothing else. If rejected: revert, REJECTED row; a variant could try the loop at a 64-byte boundary with
a layout that does not move later sections (would need +48 bytes of slack before memcmp).
