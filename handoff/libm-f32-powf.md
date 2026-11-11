# LIBM-F32-POWF: handoff (one-pass `powf`)

**LIBM-F32-POWF-4 (11-11): next merged; STARTUP-DATA-WINDOWS holds (13 <= 20); one gate red, a cold-text page phase
in locale code the story does not touch, held as LAYOUT-JITTER (section 0).** Branch `task/libm-f32-powf-4`, role shiki:~/w/sh2.

## 0. LIBM-F32-POWF-4 (shiki, Haswell; jobs `libm-f32-powf-4-*`)
* Merged origin/next ca8aa835a (LAYOUT-DATA-CLUSTERS-4C/-4D landed) into the branch: 4b416fc54, no conflict, no
  powf change. `-lint` (clippy + static + `cargo xtask so`): rc 0, all 12 layout contracts hold, STARTUP-DATA-WINDOWS
  13 written start-up pages (rw-budget-fast 20; POWF-3 had 21 on the unclustered layout). The POWF-3 red is gone.
* Plants (`-plants`, handoff/LIBM-F32-POWF-plants.txt): 7 CAUGHT, 0 SURVIVED, 0 WARN. Counts (`-counts2`, base
  ca8aa835a primed with validation/wl_prime_base.sh; `-counts` was rc 2, base not primed): powf_typ 78.0 / 275.3 /
  155.9 (glibc / base / new), powf_int 73.1 / 328.2 / 160.1, expf_typ 42.0 / 58.0 / 58.0: identical to POWF-2.
* Gate (`-gate`, direct tier): rc 1. 2523 passed, 2 failed (one test, run twice: plain and exports interposed),
  conformance 16/16, static ok, peak 3.8G. Failing: `hot_text_products::init_data_clusters_hold_at_every_text_phase`
  (LAYOUT-DATA-CLUSTERS): mode `tool` takes 56 faults at the `cold 0x800` pad and 55 at cold 0x2000/0x8000/0xF000 and
  lbss 0x800/0x2000 (5 equal runs each). origin/next ca8aa835a, same test in a RAM copy (`-nextphase`): 55 at every
  pad, passes. Head alone (`-headphase`): 56 at cold 0x800 again, so it is deterministic, not a flake.
* Fault map (`-faultmap`: the test's link at cold 0x800 and 0x2000, `perf record -e page-faults -d`,
  tools/hottext/faultmap.py): the one extra fault is `liella::locale::find_cached::<Files<Glue>>`, a 0x4fa-byte
  function in plain `.text` (not in the hot group: crates/libc-so/hot-text.list has `find_named` and `candidates`
  in its locale tier but not `find_cached`). At cold 0x800 it lies at 0x65fcc0-0x6601ba, across a page boundary; at
  0x2000 it fits in one page. Everything else differs only by address. In next's libc.a it is the same separate
  function, the same size: at next's text phase it happens to fit at both phases the test samples (0 and +0x800; the
  other three pads are whole pages). The story's code (powf, mathret.rs) lies before it in `.text`, so its growth
  moved find_cached's page phase. A 1274-byte function crosses a page at about 31% of phases, so any code growth
  ahead of it can flip this test.
* Not done (rulings #4347/#4521): no hot-list regen, no section move or alignment, no change to the test. RULING NEEDED
  in the hand-back: the hot list should take `find_cached` (tool/locale start-up text), a layout story's change.

POWF-3 (11-11, no code): STARTUP-DATA-WINDOWS 21 > 20 on the unclustered layout, caused by rustc's emission order in
.lbss (heap::RESERVE_MEM moved across a page); next ranged 17-20 and the head 18-21 over 8 .lbss phases. Fixed by
LAYOUT-DATA-CLUSTERS (13 pages above).

State before POWF-3: branch `task/libm-f32-powf-2` (LIBM-F32-POWF-2), which holds all of `task/libm-f32-powf` (code 8e913f324, plants
33b3c36c6, record fix dc05891b6, handoff 67729b223) plus a merge of origin/next 8253a107c (99116e35d: the only conflict
was the train-owned docs/codemap/symbols.tsv, taken from next; never commit it) and the two odd-y plants (21ec45efa),
then a second merge of origin/next f1f5358d5 (9357c30cf: next's exp10f/logf-family one-pass entry points next to
`pow_onepass_any` in fast32.rs, both kept; powf code unchanged). On 9357c30cf: clippy + static rc 0
(keke:libm-f32-powf-2-lint2); gate2, plants2 and counts2 (same commands as below) were PENDING at hand-back.
Role keke:~/w/n2. Decision: D-10.78 (extends D-10.63 under D-10.67). Plan rows to read: D-10.78, D-10.63, D-10.67, D-10.68.

## 1. Profile before the change (mei, Skylake-SP; wl_trace powf_typ, call 11; job mei:libm-f32-powf-trace2)
| part | instructions | what |
|---|---|---|
| `powf` + `mret::elem` entry | 34 | MXCSR read through a call (`liella_machine_mxcsr_get`), `black_box` spills |
| `fast32::pow` before the kernel | 32 | bit tests, two `cvtss2sd` (partial writes), indirect call through the GOT to `pow_core` |
| `pow_core` | 98 | log (7-deep Horner), Fast2Sum double-double, exp; one `cvtsi2sd` (partial write) |
| `fast32::pow` after (Ziv `round`) | 48 | `cvtsd2ss` -> `cvtss2sd` round trip, a jump table on the rounding mode, status |
| `elem` exit + `mathret::exit` | 50 | flag merge, push/pop; no `ldmxcsr` in the default mode, no division |
| total | 262 | 4 calls/returns, 4 partial-register writes, one serial chain from x to the result |
No division and no MXCSR write in the default mode: the latency is the call chain, the MXCSR read and the long
serial log/double-double/exp chain with partial-write merges. Cycles on the booked timing hosts: NOT RUN (budget).

## 2. Done
* `fast32::exp_log::pow_onepass` (+ `fast32::pow_onepass_any`), `mret::elem2_32_onepass`, tsv row `pow f`
  (powf, powf32). Bit-decided before any FP operation: y = ±0 (1, x not NaN); special operands (envelope); y = 1 (x);
  y = 2 for 2^-63 <= |x| < 2^64 (x² exact in double, one conversion); x < 0 integer-y test with parity; exact
  candidates to the envelope (|x| a power of two; 0 < y < 16 with 8y an integer: the only exact cases, proof in the
  doc comment). The rest: one double evaluation, operands built from bits (no cvtss2sd/cvtsi2sd: partial-write check
  12 findings, all pre-existing ALLOW rows, none in powf), answer only for y0 in [2^-126, largest float) outside a
  4096-ulp margin.
* Error bound (proof in the doc comment of `pow_onepass`): in every direction ln|x| to 3.9·2^-52 relative (split ln 2,
  r^8 series, cases A/B1/B2 of log_core), z = (y·L)·RN(128/ln 2) with |η| <= 2^-49.32, exp kernel 2^-44.51 (|r| < 1),
  |ln f| <= 88.73 for answered results: ε <= 2^-42.45, < 1499 ulp of the double; margin 4096 >= 1.5·1499 + 1.
  Every intermediate is a normal double or an exact zero, so only inexact is raised, and only for inexact results.
* Bug fixed on the way (also in `eval_pow`): x < 0 with an odd y in [2^23, 2^24) returned a positive result.
* Found by the new test before commit: the upper bound must be the largest float, not 2^128 (above it RN and RU give
  infinity); fixed, and the plant `overflow-threshold-ulp` holds it.

## 3. Results (LIBM-F32-POWF-2, merged head 21ec45efa; jobs on keke unless named)
* Counts (libm-f32-powf-2-counts, wl_percall math_f32.c, instructions/call incl. loop, base 8253a107c):
  | case | glibc FMA build | glibc hwcaps=-FMA | base | new | new/glibc (FMA, -FMA) |
  |---|---:|---:|---:|---:|---|
  | powf_typ | 78.0 | 93.0 | 275.3 | 155.9 | 2.00, 1.68 |
  | powf_int | 73.1 | 86.5 | 328.2 | 160.1 | 2.19, 1.85 |
  | expf_typ (control) | 42.0 | 46.0 | 58.0 | 58.0 | 1.38, 1.26 |
  Liella's count is the same in both builds (the one-pass path has no FMA dispatch: products and sums, no fused op).
* Timing (cbench_libm powf row, staged 21ec45efa, `--compare --no-build --runs 5`, ns/op median):
  Ivy Bridge (kotori:libm-f32-powf-2-ivb, booking 161): glibc 26.45, Liella 48.34, **1.83x** (11-07: 4.47x).
  N100 (nozomi:libm-f32-powf-2-n100, booking 162; started in the booking's last settling minutes, host idle, load 0.00):
  glibc 12.54, Liella 22.79, **1.82x**. Target <= 1.5x glibc time: NOT reached.
* Gate (libm-f32-powf-2-gate, direct tier): 2435 passed, 0 failed, conformance 16/16, static ok, peak 2.0G, rc 0.
  Lint on the merged head before any change (libm-f32-powf-2-lint): clippy + static rc 0, partial-write 12 findings,
  all pre-existing ALLOW rows (logf, sinf, tanf ...), none in powf.
* Plants (libm-f32-powf-2-plants, handoff/LIBM-F32-POWF-plants.txt): 7 CAUGHT, 0 SURVIVED, 0 WARN: the five of the
  first run plus `onepass-odd-y-2p23` and `eval-pow-odd-y-2p23` (#4449: x < 0, odd y in [2^23, 2^24)).
* Random pairs (first run, mei:libm-f32-powf-pairs, code unchanged since): 4·10^9 calls in 4 directions (2.88·10^9
  answered by the one-pass path) + 62607 hard pairs x 16 modes: 0 mismatches. First run's gate2 (mei): 2408/0, rc 0.

## Open / next (in order)
0. (POWF-4) Held for the `find_cached` hot-list fix (section 0). When it lands: merge next, rerun the gate (and
   plants and counts only if the merge changes powf code). Plants and counts on 4b416fc54 are green and need no redo.
1. Time is 1.82-1.83x glibc on both timing hosts (target 1.5x). The log is the long pole (128-entry LOG_INV/LOG_C,
   8-term series, needed for the 2^-50 relative bound at |t| up to 88). A log2-domain table (t = y·log2 x with e
   exact), or a 2^-46 bound with a wider margin, would shorten it; y = 3, 4 and 0.5 still go to the envelope (exact
   candidates): cube/square products or sqrt could take them on the bits. Profile (POWF-4, wl_trace powf_typ call 11, `-trace`): 138 instructions, all inlined in `powf`, a
   leaf: bit tests and exact-case screening 37, log reduction + degree-8 series + combine ~50, y*L*(128/ln 2) and the
   range test 14, exp_z ~24, rounding test + sign + cvtsd2ss 13. glibc's 78.0 per call includes ~18 of loop, so the
   1.5x target (117 per call) needs ~38 fewer: a shorter log series under a looser bound (a margin of 2^18 in the 28
   low bits refuses ~0.2%) and cheaper exact-case screening are the two largest phases.
Not to redo: the profile in section 1; the exact-case proof; the SSE2 count; the 10^9-pair run (rerun only if the
kernel changes).
