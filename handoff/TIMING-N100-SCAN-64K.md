# TIMING-N100-SCAN-64K (handoff, after run -2)

State: branch task/timing-n100-scan-64k-2 (stacks on task/timing-n100-scan-64k), code head c35d7c4f (scan.S +
machine_scanners.rs), later commits handoff/ only. Role mei:~/w/s4. Verdict of run -2: **DO NOT LAND** (an Ivy Bridge
row is slower in time, see below). Scratch: keke:~/work/TIMING-N100-SCAN-64K (stage.sh, scan-*.S, {base,pf0,pf512,pf1024}.tar),
nozomi and nico:~/work/TIMING-N100-SCAN-64K (run.sh, extracted trees, out/ = last run; nozomi out-base-pf0/ = base vs pf0).

## Change (csys/src/machine/scan.S, AVX2 level only; unchanged since run -1)
AVX2 memchr/rawmemchr long loops take 256-byte aligned groups (8 loads, 7 vpor, 1 vptest); a 128-byte group first when
the entry is not 256-aligned; no software prefetch (ruling #3322). Instructions (Zen 1, run -1): memchr_64k 6192 -> 5172,
rawmemchr_64k 5175 -> 4675, memchr_4k 422 -> 364, rawmemchr_4k 366 -> 348 (glibc 6713/5690/466/403). SSE2 code unchanged.

## Run -2: what was fixed
* keke:timing-n100-scan-64k-plants rc 3: xtask plants refused 4 plants whose pattern began at a local label; all now
  start at `liella_machine_<fn>:` (commit 854f2aa8). Rerun mei:timing-n100-scan-64k-2-plants rc 0: 9 plants, 6 CAUGHT,
  0 SURVIVED, 3 WARN-NO-MSG (memchr-256-end, raw-256-hi, raw-256-fold: caught by SIGSEGV on machine_scanners' guard
  page after the planted scan.S was rebuilt; the msg regex did not name SIGSEGV. Add `|SIGSEGV` to them next time).
* Timing staging: the staged trees (target/liella, bench/, xtask/Cargo.toml, xtask-bin) cannot run `bench --no-build`:
  xtask's root() needs a top-level Cargo.toml (else it falls back to the build host's path: "bench/budgets.tsv: No such
  file"), and the --no-build freshness check needs crates/ and csys/ sources (with none it reports libc.a "older than its
  inputs"). Fixed on both timing hosts by adding Cargo.toml and `git archive <commit> crates csys` of each variant's own
  commit (base 883a05df, pf0 c35d7c4f, pf512 64a80d58), mtimes set to 2026-11-06 00:00Z (before the builds). Failed
  attempts: nozomi:timing-n100-scan-64k-2-n100 and -n100b (rc 1, staging), not results.
* pf0.tar = the final code (scan.S differs from c35d7c4f only by an empty prefetch macro and a comment).

## Timing (run.sh 5 rounds x 3 alternations, interleaved; ratio = median of per-round ratios; glibc drift = check)
N100 (booking 52, load ~0.3), base -> final (nozomi:timing-n100-scan-64k-2-n100c):
| row | L/g time base | L/g time final | L final/base | glibc drift | instr L/g final |
|---|---|---|---|---|---|
| memchr_64k | 1.148 | 1.057 | 0.918 | 1.002 | 0.770 |
| rawmemchr_64k | 1.062 | 1.057 | 0.995 | 1.000 | 0.822 |
| memchr_4k | 1.036 | 0.988 | 0.953 | 1.000 | 0.782 |
| rawmemchr_4k | 1.047 | 1.035 | 0.988 | 1.000 | 0.863 |
_16_mix / _64_mix rows: 1.000 (unchanged). The 64k rows are still ~6% slower than glibc with 18-23% fewer instructions.

N100, pf0 (final) -> pf512 (prefetcht0 x4 per 256 B, 512 ahead; nozomi:timing-n100-scan-64k-2-n100-pf):
memchr_64k 1.069 -> 1.000 (0.940), rawmemchr_64k 1.054 -> 0.920 (0.875), memchr_4k 0.998, rawmemchr_4k 1.003, mix
rows 1.000; instructions +20% (rawmemchr_64k L/g 1.001, rawmemchr_4k 1.022, i.e. above glibc's count). -> RULING NEEDED.

Ivy Bridge (booking 57, SSE2 level), base -> final (nico:timing-n100-scan-64k-2-ivb): instructions identical, but
memchr_16_mix 1.164 -> 1.330 (L final/base 1.142, glibc 0.999, spread 0.000) and memchr_64_mix 0.939 -> 0.978 (1.046);
rawmemchr_16_mix 0.937 (faster); 64k rows 1.000; memchr_4k/rawmemchr_4k spread ~0.5 (noise). A layout effect: the SSE2
code is unchanged. In cbench_memref, memchr's entry is at the same address (0x425200) in both builds; the first
difference is the target liella_hot_memchr_rest (0x679680 -> 0x6797c0, both 64-aligned); code after the AVX2 loop inside
memchr moved (memchr body ~20 instructions shorter). Cause NOT pinned down.

## Exact next step
1. On Ivy Bridge: find which moved code memchr_16_mix runs (perf record on the timing host while booked; annotate on a
   build host, nico has no objdump), then pin its alignment (`.p2align` on the moved block, or keep the earlier layout)
   so base layout is restored; re-time Ivy Bridge (memchr_16_mix, _64_mix) and N100 (no row slower), recount.
2. Apply the prefetch ruling (pf512 if accepted: N100 times the whole set again, plus one other AVX2 class).
3. Rerun lint, gate, plants (with `|SIGSEGV` in the three msg regexes), counts on the new head.
Not to redo: the N100 and Ivy Bridge staging (trees ready on both hosts), base/pf0/pf512 timings above.
