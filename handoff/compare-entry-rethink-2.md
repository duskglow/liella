# COMPARE-ENTRY-RETHINK-2: per-ISA short compare entries (memcmp / bcmp / __memcmpeq)

State: branch `task/compare-entry-rethink-2`, base origin/next aa16620da. Planning story: no product code changed.
Prototype and evidence in `handoff/compare-entry-rethink-2/` (proto2.S, harness2.c made by mkharness.py from the first
rethink's harness, build.sh, time2.sh, analyze2.py, counts-skylake-sp.txt, time-ivb.txt, time-n100.txt). Order kept:
inputs, designs, prototype + full-matrix timing, and only then next's entry (fold.S `memcmp:`, read last).

## 1. Inputs
* Rows behind glibc on next (results/timing-sweep-1106e-table.md): Ivy Bridge `__memcmpeq/bcmp_head_128/1k` 1.46-1.54,
  `*_eq_128` 1.26, memcmp diff8 64-127 1.25-1.33, `memcmp_256` 1.16; N100 only `memcmp_256` 1.06; Zen 3 none.
* Tried (handoff/compare-head-rethink.md, compare-head-h2.md of -H2/-H2-2/-H2-3): H2 (head block before dispatch),
  S1 (small sizes fall through), (c) (AVX2 33-2048 span test first). Each traded rows: a runtime ISA test in the
  shared entry costs the small rows one timer quantum (about one cycle; Gracemont decodes straight-line code about 3
  instructions per cycle per cluster), and the head block + ISA test in front costs the N100 AVX2 33-128 rows.
* Binding costs on record: classed slot swap (D-4.53 M6; off by default, #3434/#3841): 0 per call, AVX2 class +65.5k
  kernel instructions and +29.3 us per static start on the N100 (CLASSED-SLOT-SWAP-COST), 0 on SSE2 hosts; floor one
  VMA call (>= 20k). M1 IFUNC/RELRO and M3 pointer: +1 indirect `jmp` per static call, +0 dynamic (PLT paid anyway).
  D-4.25 flag: +2 per call. Phase 14 plans IFUNC/IRELATIVE and the hwcaps subdirectory search; no `ld.so` runs yet.

## 2. Designs (each ISA its own entry, no runtime ISA test in any entry)
Bodies (both schemes): **S** (SSE2): `cmp head(%rip),n` (data word 17 when NULL folding is on, so the fold state
costs nothing extra) -> n <= 16 falls through to the small path; n >= 17: 16-byte block at 0 first, then 17-32 block
n-16, 33-64 blocks 16/n-32/n-16 ANDed, 65-128 3+4 blocks, > 128 loop. **A** (AVX2, small first): same small path;
n >= 17: 17-32 two VEX-128 blocks (taken), else ymm block 0, 33-64 block n-32 (falls through), 65-128 3 ymm blocks.
**B** (AVX2, span first): `lea -33(n); cmp span(%rip)` (2016 folded) -> A's 33-2048 code with one taken branch, then A's
order. Counted in-function on Skylake-SP (instructions, taken incl. ret: counts-skylake-sp.txt), memcmp eq:

| class (row) | next | S | A | B |
|---|---|---|---|---|
| 1-16 (eq_8 / eq_16; mix16) | 17 / 17; 18.6 | 12 / 12; 18.1 | 12 / 12; 18.1 | 15 / 15; 21.1 |
| 17-32 (eq_32 / head_32) | 24 / 12 | 15 / 11 | 13 / 13 | 16 / 16 |
| 33-128 (eq_64 / eq_128 / head_128) | 17 / 26 / 19 | 25 / 48 / 11 | 16 / 24 / 13 | 15 / 23 / 12 |
| 129-2048, > 2048 | next's long classes | prototype loop (eq_1k 350, not proposed) | loop (190, not proposed) | same |

Schemes (binding), startup and memory:
* **Scheme 1, classed pages (D-4.53 M6):** S at the exported labels on page 0, the AVX2 entry on page 1. Per call 0.
  Static start: SSE2 class 0; AVX2 class +65.5k kernel instr, +29.3 us (N100). Dynamic: same in libc init, or the loader
  maps page 1's file offset over page 0 (+1 `mmap`, about 20k). Memory: one more page pair in the file, +2 VMAs.
* **Scheme 2, bound pointer (static IFUNC/.iplt or RELRO pointer; dynamic IFUNC through the PLT):** per static call
  +1 instruction and one taken indirect jump (row P below), dynamic +0; start: three resolver calls (< 0.1 us); memory:
  3 GOT words. The NULL-fold decision stays a data word (lazy), so no writable code pointer is needed.
* Scheme 3, per-ISA library (hwcaps `x86-64-v3/libc.so.6`): dynamic only, 0 per call and start; static cannot choose
  at load time (link-time choice only for programs built for v3). Not prototyped (no `ld.so` yet).

## 3. Measurements (full matrix: 15 sizes 4-4096 x eq/head/tail/diff8 + mix16/mix64, 3 functions)
Correctness: harness check (every n 0-300 + large n, all positions, both signs, guard pages) 0 failures: Skylake-SP
4,675,671 checks (glibc, next, next with -AVX2, every prototype), N100 4,675,671, Ivy Bridge 2,975,427 (SSE2 entries).
Timing: 7 interleaved process rounds, medians, idle timing hosts; ratio to next in the same binary; one quantum
about 0.4 ns. Per-row ratio distribution (60 rows per function; time-*.txt has every row):

| host, function | S/next | A/next | B/next | P/next (pointer to the host's entry) |
|---|---|---|---|---|
| Ivy Bridge memcmp | median 0.576 (0.409-0.835), 0 rows > 1.05 | - | - | 0.693 (0.467-0.896) |
| Ivy Bridge bcmp, __memcmpeq | 0.90 (0.55-1.18), 8 rows > 1.05 | - | - | 1.05 (0.65-1.30), 29 rows |
| N100 memcmp | 0.879 (0.32-1.73), 23 rows | 0.909 (0.36-1.23), 7 | 0.933 (0.36-1.23), 10 | 0.994 (0.41-1.38), 20 |
| N100 bcmp, __memcmpeq | 1.000 (0.46-16), 22 rows | 1.000 (0.54-15), 13 | 1.000 (0.54-15), 9 | 1.083, 35 |

Rows behind the numbers: Ivy memcmp head_128 next/glibc 1.545 -> S 0.452 of next (0.698 of glibc), eq_128 1.111 ->
0.640, eq_64 0.640, mix16 0.721, eq_8/16 0.569. Ivy eq kernel: S head_24-64 1.10 (the only S rows above 1.05 apart
from the prototype loop). N100: S on 33-128 eq/tail rows 1.25-1.73 (what an AVX2 host runs with the swap off); B
eq_48/64 1.000 against A 1.125, A memcmp mix16 0.999 against B 1.158; every prototype variant is 1.08-1.17 on N100
head_8-16 / memcmp 4 rows (the prototype's own small-path forms, not the entry: keep next's small paths); the 15x max
is tail_1k/4k, where next's long class probes the tail early (keep next's long classes). Pointer: P/S 1.07-1.33 on
short rows on both hosts, i.e. one quantum per call.

## 4. Current implementation (read last)
next's `memcmp:` runs a 5-instruction gate-word ladder (`liella_machine_cmp_wide`/`_low`: ISA and fold state folded
into carry tests) and a byte-swapped 8-byte head probe before any size split, so every class pays both; the head
rows above 64 then take further taken branches (19 instructions on head_128 against S's 11). S drops the ladder
because the entry itself is the ISA choice; the fold state rides on the one size compare that is needed anyway.

## 5. Reading and recommendation
* A per-ISA entry removes the rut on SSE2 hosts outright: S is at or below next on every Ivy memcmp row (median
  0.576) and fixes all the Ivy rows behind glibc. The equality kernel's 17-64 head rows (1.10) need one more look.
* The binding decides the AVX2 side. Pointer (Scheme 2) costs a quantum per call on both hosts: rejected for static
  (kept for dynamic IFUNC in phase 14, where it is free). Without a binding (swap off), AVX2 hosts run S: N100 33-128
  rows 1.25-1.73 of next, rejected. With the swap on, the AVX2 entry in order B keeps N100 33-128 rows at 0.91-1.0.
* Recommendation: adopt per-ISA entries (D-NEW-1) with page 0 = S and page 1 = B, and ask the owner for the swap.
  Until that ruling, do not land S alone (it regresses AVX2 hosts); next stays.

RULING NEEDED (owner): turn the classed swap on by default on AVX2 hosts for the compare entries. Cost +29.3 us per
static process start on the N100 (+65.5k kernel instructions; 0 on SSE2 hosts). Gain per call on the N100 against the
swap-off alternative: about 0.4-1.2 ns (1-3 quanta) on 33-128-byte calls. Break-even about 25k-75k such calls per
process: long-running sort/hash workloads gain, short-lived processes (exec_true_static +15.6%) lose. Recommend: on
only together with the user-space guard scan of CLASSED-SLOT-SWAP-COST (cuts about half the kernel work, not
measured here), and only after the wl_count workloads show no start-up-heavy row slower; otherwise keep next.

Proposed rows (D-NEW: the coordinator reserves the s04 ids):
* D-NEW-1 | §4.3 | PERF | Per-ISA short compare entries, no runtime ISA test: SSE2 entry (small first, head-first
  16-byte blocks, fold state in the size word) at the exported labels; AVX2 entry (33..2048 span test first) on the
  classed AVX2 page; small paths and long classes as on next. Evidence: COMPARE-ENTRY-RETHINK-2 tables.
* D-NEW-2 | §4.1-4.3 | PERF | Static entries are not bound through a pointer (IFUNC/.iplt or RELRO): measured one
  quantum per call on Ivy Bridge and N100 (P/S 1.07-1.33). Dynamic IFUNC (phase 14) is exempt (the PLT jump is paid).

## Code briefs (after the ruling)
1. COMPARE-ENTRY-PAGES (opus, L): move `memcmp`/`bcmp`/`__memcmpeq` into the classed pages: page 0 = S with next's
   small paths and long classes, page 1 = B with the same; drop the gate words if nothing else reads them; fix the
   S equality head_24-64 rows (try next's word probe for 17-64 heads). Prove with small_compare, plants, this harness
   (both pages, guard pages), and booked timing on Ivy Bridge + N100 (+ Zen 3) with the swap on and off.
2. If the ruling is no: keep next; record this run's tables as the evidence (saturation register 03, NEGATIVE row).

Jobs: mei `compare-entry-rethink-2-build2` (build, check, counts; `-build` SUPERSEDED: printf argument order in the
harness, fixed), kotori `compare-entry-rethink-2-kotori`, nozomi `compare-entry-rethink-2-nozomi` (timing; read with
`python3 -I analyze2.py ~/logs/<label>.log` in `~/work/COMPARE-ENTRY-RETHINK-2/t`).
