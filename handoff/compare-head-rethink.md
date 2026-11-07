# COMPARE-HEAD-RETHINK: short compare entry (memcmp / bcmp / __memcmpeq), rethink

State: branch `task/compare-head-rethink`, base origin/next `e0c6b146` (TIMING-COV-COMPARE-HEAD-SSE2 #3814 is NOT in
that base: "Liella" below = next without it). Planning story: no product code changed. Prototype, harness and scripts:
`handoff/compare-head-rethink/` (proto.S, harness.c, run_count.sh, time.sh, analyze.py). Order kept: inputs, black-box
measurement, designs, prototype, and only then the current source (fold.S `memcmp:` entry, read last).

## 1. Inputs
* Rows (results/timing-sweep-1106e-table.md, Ivy Bridge): `__memcmpeq_head_128/1k` 1.538, `bcmp_head_128/1k` 1.462,
  `memcmp_diff8_64/96/127` 1.25-1.33, `__memcmpeq_eq_128` 1.263, `bcmp_eq_128` 1.260, `memcmp_256` 1.162; N100 faster
  except `memcmp_256` 1.061. No Zen 3 table on next (not read: it is only on another branch).
* History: about 12 stories of -1 to -6 instructions each (saturation register 03, rows on `.Lcmp_wide`, PERF-MEMCMPEQ-EARLY,
  PARITY-MEMCMP-SMALL-EQ, TIMING-N100-MEMCMP-2). MEMCMP-33-64-RECOVER: sort-C's 33-64 calls differ in bytes 0-7 in
  99.9996% of cases, so the head-difference path is the common case of ordered compare. Earlier profile (head_128, SSE2):
  entry ladder 5 + jns 1 + class hops 4 + probe 6 + order 7.

## 2. Black-box measurement (harness.c; null loop = 13 instr, 3 taken subtracted, so call+ret are in-function)
Correctness first: every n 0-300 at every position, both signs, n up to 8000, buffers against PROT_NONE guard
pages, prototype both ISA settings: 4,250,610 checks, 0 failures (glibc, Liella and prototype).

Skylake-SP class, forced SSE2 (`glibc.cpu.hwcaps=-AVX2`), in-function instructions / taken branches (deterministic):

| row | glibc | Liella | prototype H |
|---|---|---|---|
| memcmp head_128, head_1k, diff8_96/127 | 13 / 2 | 21 / 4 | 11 / 1 |
| memcmp eq_16 / head_16 | 13 / 1, 13 / 2 | 17 / 0, 12 / 1 | 15 / 0, 11 / 1 |
| memcmp eq_64 | 29 / 3 | 32 / 2 | 26 / 1 |
| memcmp eq_128 / tail_128 | 54 / 2, 62 / 3 | 47 / 3, 76 / 6 | 48 / 2, 68 / 3 |
| memcmp mix16 / mix64 (sizes 1-16 / 1-64) | 14.7, 22.0 | 18.2, 26.3 | 15.2, 20.2 |
| __memcmpeq head_128 | 9 / 2 | 15 / 5 | 8 / 1 |
| __memcmpeq eq_8 / eq_16 | 11 / 0, 11 / 1 | 13 / 0, 13 / 0 | 10 / 1, 16 / 0 |
| __memcmpeq eq_128 | 52 / 3 | 49 / 4 | 44 / 2 |

AVX2 mode (instructions / uops): head_128 Liella 19 / 21 vs H 11 / 9, `__memcmpeq` 14 / 16 vs 8 / 6; but the
prototype's SSE2-only middle classes lose on equal rows: eq_64 17 vs 26, eq_128 26 vs 48, `__memcmpeq` eq_128 27 vs 44.
Hence design H2 below (ISA test after the head block).

**Front-end finding (Skylake-family only):** legacy-decode uops per call (IDQ.MITE_UOPS, raw 0x0479): glibc's head
and short rows about 0; Liella memcmp head_128 7.9-10, `__memcmpeq` eq_8/eq_16 13.0 (every uop), eq_128 24-56; the
unpadded prototype 6-42, the same prototype assembled with `-Wa,-mbranches-within-32B-boundaries` about 0. This
matches the JCC-erratum microcode rule (a jump that crosses or ends on a 32-byte boundary is not cached in the uop
cache; Intel, "Mitigations for Jump Conditional Code Erratum", 2019). Cycles on that host are advisory (load about 29):
Liella `__memcmpeq` eq_16 14 cycles against glibc 4. Ivy Bridge and Gracemont do not have this erratum.

Ivy Bridge timing (idle host, 5 interleaved process rounds x 9 inner rounds, medians; ratio to glibc in this harness;
H = prototype in the Liella binary, its copy in the glibc binary within 0-15% of it):

| row | Liella / glibc | H / glibc |
|---|---|---|
| memcmp head_128, diff8_96 | 0.741, 0.739 | 0.478, 0.478 |
| memcmp head_1k | 0.862 | 0.558 |
| memcmp eq_128 / tail_128 / eq_256 | 0.577 / 0.715 / 0.849 | 0.522 / 0.589 / 0.761 |
| memcmp eq_16 / head_16 | 0.522 / 0.522 | 0.600 / 0.550 (worse) |
| memcmp mix16 / mix64 | 0.878 / 1.021 | 0.565 / 0.522 |
| memcmp eq_1k / eq_4k | 0.622 / 0.616 | 0.694 / 0.695 (worse: H's plain loop) |
| __memcmpeq head_128 / head_1k | 1.750 / 1.575 | 0.717 / 0.717 |
| __memcmpeq eq_128 = tail_128 | 1.257 | 0.704 |
| __memcmpeq eq_16 / eq_64 / eq_1k | 0.899 / 1.000 / 0.969 | 0.781 / 0.680 / 0.949 |
| __memcmpeq mix16 / mix64 | 1.099 / 0.969 | 1.041 / 0.750 |

`__memcmpeq` ratios agree with the sweep (eq_128 1.257 here, 1.263 there). This harness's glibc `memcmp` is slower
than in cbench (Liella memcmp rows 0.52-0.88 here, 0.82-1.33 in the sweep): for memcmp, read the H-vs-Liella column
pair (same binary), not the ratio to glibc. N100: job `compare-head-rethink-n100` (native + forced SSE2) was queued
on the N100 host behind another timing job: PENDING, not read.

## 3. Designs (first principles: Intel optimization manual costs, psABI; no libc source)
* **H, head-first, ISA-late (prototyped).** `n < 16` to the small path; otherwise the 16-byte block at 0 (legacy
  SSE2, valid on every x86-64) is compared before any size or ISA dispatch; a difference leaves at once (memcmp: bsf,
  two byte loads, sub; equality: 1). Then 17-32 one overlapping block at n-16, 33-64 three blocks ANDed, 65-128 3+4
  blocks ANDed in two groups, >128 the long class. Head rows cost the same at every n (11 / 1, 8 / 1 measured);
  diff8 rows fold into head rows. NULL with n >= 1 faults at the first load for every n.
* **H2 = H with the ISA test after the head block (recommended).** For n > 32 one flag test picks the AVX2 32-byte
  classes (Liella's existing ones) or the SSE2 classes. Estimate (Skylake-SP, AVX2): head rows 19 -> 11 (memcmp),
  14 -> 8 (equality); eq_64/eq_128 about the current 17 / 26 + 1 (head block 6 + test 2 replace the ladder's 5-7);
  SSE2 rows as measured for H. Needs nothing from the classed slots (D-4.53): the only ISA test sits behind a
  passed head block and n > 32, in front of bodies of 15+ instructions.
* **J, per-ISA entries chosen once + computed size-class jump.** IFUNC (+1 `jmp *GOT` per call in static programs,
  design doc M1) or the classed slot swap (+65.5k kernel instructions per start, CLASSED-SLOT-SWAP-COST) removes the
  ISA test; the class is `min(n, 129) >> 5` through a read-only table (about 5 instructions, 1 indirect taken branch).
  Head rows: 2 + 5 + 6 + 5 = about 18 / 2-3 (the dispatch still runs before the first compare), 7 worse than H;
  mixed sizes mispredict the indirect jump like a chain. Rejected: it optimises the dispatch H makes unnecessary.
* **S, classed slot swap on by default.** Saves the flag test only; costs every process start (above). Not needed.

## 4. Current implementation compared (read last: fold.S `memcmp:`)
Two memory gate words (`liella_machine_cmp_wide`, `liella_machine_cmp_low`) fold size class and ISA into a carry
ladder BEFORE any byte is compared (2 loads, 2 adds, 2 branches); for 8-64 the head probe byte-swaps both words
before the equality test (2 uops on every call, also when equal); n >= 65 reaches its class through further taken
branches (measured 4 taken in-function on head rows, against 1 for H). H keeps: the small paths (n < 16, faster than
glibc today), the long classes (TIMING-N100-MEMCMP-2 tuning), the equality kernels' 0/1 contract (D-4.41/D-4.43).

## 5. Recommendation
Adopt H2 for `memcmp`, `bcmp`, `__memcmpeq` (D-NEW-1), and separately make the hot machine assembly safe from the
JCC-erratum uop-cache penalty (D-NEW-2), which is measured on Skylake-family cores and invisible in instruction counts.

Proposed rows (D-NEW: the coordinator reserves the s04 ids):
* D-NEW-1 | §4.3 | PERF | Head-first compare entry: for n >= 16 the 16-byte block at offset 0 is compared before any
  size or ISA dispatch; size classes follow; the only ISA test is after the head block for n > 32; the compare
  entries use no gate words. Evidence: COMPARE-HEAD-RETHINK (head rows 21 / 4 -> 11 / 1 instructions / taken;
  Ivy Bridge `__memcmpeq_head_128` 1.75 -> 0.72 of glibc in the prototype).
* D-NEW-2 | §4.1-4.3 | PERF | Hot `csys/src/machine/*.S` code keeps jumps off 32-byte boundaries (assembler option
  `-mbranches-within-32B-boundaries` or explicit alignment), checked with IDQ.MITE_UOPS on a Skylake-family host;
  reference: Intel JCC erratum white paper (2019).

## Code briefs
1. **COMPARE-HEAD-H2** (opus, L): replace the three entries in fold.S by H2 (prototype in
   `handoff/compare-head-rethink/proto.S`, H's 17-128 SSE2 bodies; the AVX2 classes reused after the head block);
   keep n < 16 and the long classes; remove the compare gate words (`dispatch::store_cmp_gates`) if nothing else
   reads them. Prove with small_compare tests (NULL tests per the ruling below), plants, this harness and wl_percall
   (both ISA modes), timing on Ivy Bridge + N100 + a Skylake-family host. Done when head/diff8 rows are at or below
   glibc on Ivy Bridge and no eq/tail row is above its current count by more than 1 instruction.
2. **MACHINE-JCC-PAD** (sonnet, M): assemble the machine .S files with the option of D-NEW-2 (or align by hand),
   measure IDQ.MITE_UOPS and cycles on every cbench_memref row before/after on a Skylake-family host, and confirm by
   counters and timing that no Ivy Bridge / N100 row is slower (padding adds nops).
3. Later, after 1: mixed-size rows (`memcmp` mix64: Liella 26.3 vs H 20.2 instructions, Ivy Bridge 1.021 vs 0.522).

## Open / not done
* N100 timing (queued, pending). Zen 3 not measured. Ivy Bridge counter pass is in the same log (not read here).
* The prototype's small path (n < 16) and long loop were not the subject; they are not proposed.

Jobs: mei compare-head-rethink-so, -count, -count-pad; nico compare-head-rethink-ivb; nozomi compare-head-rethink-n100
(pending; read with `~/bin/jobwait compare-head-rethink-n100`, then
`python3 -I ~/work/COMPARE-HEAD-RETHINK/analyze.py ~/logs/compare-head-rethink-n100.log`).
