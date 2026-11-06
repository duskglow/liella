# TIMING-IVB-STRSCAN-DISPATCH: strchr / strchrnul / strncmp on the non-AVX2 class: front-end bubbles from the dispatch

Id: TIMING-IVB-STRSCAN-DISPATCH
Kind: code
Priority: 80
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 80 calls / 140k

## Rows (TIMING-SWEEP-1105, Ivy Bridge E3-1220L v2; instr / cycles / time, Liella over glibc)
strchr_16_hit 1.063 / - / 1.318; strchr_16_miss 0.750 / 1.163 / 1.162; strchr_64_hit 1.036 / 1.374 / 1.373;
strchr_64_miss 1.036 / 1.469 / 1.472; strchrnul_16_hit 1.069 / - / 1.557; strchrnul_64_hit 1.038 / 1.533 / 1.534;
strchrnul_64_miss 1.038 / 1.437 / 1.439; strncmp_eq16 1.015 / 1.263 / 1.265 (all cbench_hot).
The N100 (AVX2 path) is faster than glibc on every one of these rows: this is the SSE2-class path only.

## Profile evidence (TIMING-REOPEN-1105, booked Ivy Bridge host, perf stat user mode, 3 runs per event set,
## CBENCH_ROWS=<row>; the row's function is 74-89% of cycle samples, so whole-process counts are the row)
- strchr_16_hit: cycles 1.33x. idq_uops_not_delivered.core 969.6M vs 43.2M (22x); the slot difference is 3.88x the
  cycle difference, i.e. about 97% of the gap at 4 slots per cycle. resource_stalls.rs 0.05x of glibc, branch misses,
  store-forward blocks, split loads, flags-merge uops, LCP stalls and machine clears all ~0 on both sides.
  DSB-to-MITE switch penalty 0.8M vs 0.15M cycles and MITE uops 11M vs 3M: under 1% of the gap. uops issued 1.03x.
- strchr_64_miss: cycles 1.46x, undelivered slots 26.6x (3.61 slots per gap cycle, about 90% of the gap).
- strchrnul_64_hit: cycles 1.53x, undelivered slots 3.55x (2.64 slots per gap cycle, about 66%), port 5 +0.40 uops per gap cycle.
- strncmp_eq16: cycles 1.27x; front-end only about 11% of the gap here (0.43 slots per gap cycle);
  cycle_activity.cycles_no_execute 8.7x and stalls_ldm_pending 9.1x (each 0.17 of the gap): cycles with no uop
  executing while a load is outstanding, i.e. load-use latency on the path; uops issued 0.97x.
- Liella disassembly (sweep build of next f2157504): on this class every strchr call runs
  entry `cmp rshort_mask(%rip)` + `jae` (taken) to a far block, `test schr_mask(%rip)` + `je` (taken) back to the SSE2
  body at +0x8a (not 32-byte aligned), then the body. Two taken branches across distant 32-byte windows and two
  dispatch-mask loads per call before the first compare. Hot samples: +0xc3..+0xd5 (first block bsf/je/ret), +0x8a
  (the SSE2 entry), +0xa2..+0xb3 (broadcast/compare). strncmp has the same entry shape (`cmp rshort_mask` + taken
  `jae` to +0xd5; hot samples at +0xd5..+0x14b and +0x250..+0x25e, the target of a further `jmp`).
- glibc: per-symbol cycle shares only (no annotation: clean-room rule, no disassembly of glibc).

## Hypothesis
Front-end delivery bubbles: on Sandy/Ivy Bridge the decoded-uop cache delivers from one 32-byte window per cycle
and a taken branch ends the cycle's delivery; the dispatch adds two taken branches (plus their target windows
starting mid-window) to a call that is only ~10-20 cycles long. strncmp_eq16 adds load latency: the class test's
mask load and the page-offset check sit in front of the first string loads.

## The change
1. Re-profile on the current next first (scan.S changed since f2157504: PARITY-STRCMP-AVX2 landed).
2. Reach the SSE2-class body with at most one taken branch and one dispatch load per call. Options to measure, cheapest
   first: (a) a single combined class test at entry that jumps straight to the SSE2 body (no second mask test),
   SSE2 body aligned to 32 bytes; (b) make the SSE2 body the entry's fall-through only when that does not add a
   taken branch on AVX2 hosts; (c) resolve the entry once at start-up to the class body (IRELATIVE or a start-up
   chosen entry): a mechanism change, needs a D-12 ruling - recommend (a) first.
3. strncmp: same entry fix; then check whether the first string loads can issue before the class/page checks resolve.
4. AVX2 hosts must not get slower: instruction counts on an AVX2 build host and the N100 timing of the same rows.

## JOBS
gate (cargo xtask clippy, static, affected --base origin/next); plants via `cargo xtask plants` on every changed
entry (wrong class taken, page-cross check dropped, NUL/hit order swapped) - .S plants anchored to the function
label, tests named explicitly; instruction counts before/after (cbench_hot, `cargo xtask bench --compare --counters`)
as the regression gate on an AVX2 build host AND the non-AVX2 class (`--tunables glibc.cpu.hwcaps=-AVX2`); a booked
timing check of these rows on the Ivy Bridge timing host (`ops timing-book nico <min> --story <ID>`, stage products
from a build host as TIMING-SWEEP-1105 did) and on the N100 for no regression. Target: time ratio <= 1.05 on Ivy Bridge.
