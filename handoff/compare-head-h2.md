# COMPARE-HEAD-H2 / -H2-2: head-first compare entry (memcmp / bcmp / __memcmpeq), checkpoint 2

State: branch `task/compare-head-h2-2` (base origin/next 5872ad47e). Commits: 09459dd58 H2 entries (D-4.66),
954b52553 plants file, b737c441d "let 0-15 byte calls fall through the entry" (S1, this run), then this handoff.
Role mei:~/w/s1. **Verdict: INCOMPLETE, must not land as it stands: AVX2 equal rows 64-128 B are slower in TIME
than next on the N100 (ruling #3899 condition), so option (c) has to be built first (design below).**

## What the code does now (fold.S, dispatch.rs)
* Entry: `cmpq cmp_head(%rip), %rdx` then `jae .Lcmp_head` (memcmp, n >= 16) / `ja .Leq_head` (bcmp/__memcmpeq,
  n > 16); below that the code FALLS THROUGH (S1): n < 8 -> `.L*_lt8` (explicit NULL tests), 8 <= n < `cmp_short`
  (17 folded, 8 off) -> 8-byte words at 0 and n - 8, else the checked Rust entry. Head path as in H2 (16-byte block
  at 0, 17-32 block at n - 16, > 32: `cmp_long` test -> AVX2 classes, else SSE2 33-64 / `.L*_sse_65`).
* Why S1: Ivy Bridge timing of H2 showed n = 8 / 16 and mix16 12-25% slower than next for bcmp/__memcmpeq (memcmp
  6-11%): a taken branch + one more compare in front of every n < 16, and the n = 16 block compared twice.

## Evidence (rethink harness, 7 interleaved process rounds, medians; ratio = H2 or S1 / next, same host)
| row (native mode) | Ivy H2 | N100 H2 | N100 S1 |
|---|---|---|---|
| bcmp/__memcmpeq eq_8 / eq_16 | 1.19-1.25 / 1.125 | 1.00 / 1.00 | 1.00 (bcmp eq_8 1.17 = next's own spread) / 1.00 |
| bcmp/__memcmpeq mix16 | 1.12-1.13 | 1.14 | 1.00 |
| eq_32 (all three) | 0.75-0.82 | 0.875 | 0.875-0.94 |
| **eq_64** bcmp,__memcmpeq / memcmp | 0.857 / 0.80 | **1.25 / 1.125** | **1.375 / 1.25** |
| **eq_96, eq_128** | 0.83 / 1.05 | **1.083** | **1.167** |
| mix64 | 0.82-0.90 | 1.03-1.13 | 1.04-1.13 |
| head_96..4096 | 0.43-0.63 | (not read) | 0.78-0.79 |
Spread (max/min of 7 rounds) of next and H2 on those rows <= 1.01 except where noted; the proto control row moves
by one timer quantum (0.86-1.0), so one quantum (~0.4 ns) is noise, 1.083+ on several sizes is not.
Ivy S1 job `kotori:compare-head-h2-2-kotori-s1` was queued behind another story's timing job: read it with
`python3 -I ~/work/COMPARE-HEAD-H2-2/t2/analyze.py ~/logs/compare-head-h2-2-kotori-s1.log` on kotori.
Counts (wl_percall memcmp_bands, Skylake-SP, `compare-head-h2-2-percall-2`, delta S1 - next in-function):
AVX2 eq_8..16 -1..-3, eq_32 -5..-9, **eq_48/64 +7/+8, eq_96..256 +6**, head_128 -6..-8, head_8..64 +0..+2;
SSE2 eq rows -1..-9 (memcmp eq_128 +1), head rows +0..+2 at 8-64, -7/-8 at 128.
Correctness: harness check (4,250,610 checks, guard pages, all positions) 0 failures for glibc / next / H2 / S1 in
the native and forced-SSE2 modes (Skylake-SP), and H2 on the N100 and Ivy Bridge.

## Next step (in order)
1. Option (c), proposed shape (not built): a new fold word `cmp_avx_span` = 2016 when AVX2 classes and folding are
   on, else 0; at the very top of both entries `leaq -33(%rdx), %rax; cmpq cmp_avx_span(%rip), %rax; jb .L*_wide`
   (AVX2 33..2048 reach the AVX2 classes with ONE taken branch, like next's ladder; `.L*_wide` keeps its
   `cmp $64; ja .L*_big_load`), then S1's head test. The `cmp_long` test after the head block then only serves SSE2
   / > 2048 (keep it or fold into the >64 path). Cost: +3 not-taken instructions on every other row; must be timed
   on Ivy Bridge (small rows, mix16) and the N100 (AVX2 eq_64/128 must reach next within one quantum).
   H2 put 2 taken branches + the head block in front of the AVX2 bodies, S1 3 taken branches: N100 time tracks the
   taken-branch count on these rows more than the instruction count.
2. Plants: add one for the new word (AVX2 span test dropped -> AVX2 rows via SSE2 path is equivalent by result;
   use a span-off-by-one plant at 2048/2049 instead) and re-run `handoff/COMPARE-HEAD-H2-plants.txt`.
3. Re-time with `~/work/COMPARE-HEAD-H2-2/build.sh <base> <head>` on mei (links h_glibc / h_next / h_h2 from
   ~/wl/products, runs the sweep, packs timing.tgz) and `run.sh` under `~/bin/timing` on nozomi + kotori/nico.
4. D-4.66 row: add the S1 entry order and the (c) result; fold.S header prose below D-4.66 still predates it.

## Not to redo
H2-alone and S1-alone are measured (above). The memcmp head/diff8 win on Ivy Bridge (0.5-0.63 of next) and the
small-row parity both hold with S1; only the AVX2 > 32 routing is open.
