# TIMING-COV-REGEX-BACKREF-SCRATCH: per-call scratch of the back-reference matcher

State: branch task/timing-cov-regex-backref-scratch, code b9d6936c + test fix 7b2ff012 (cut from origin/next cc39389c).
Status: PENDING JOBS (Ivy Bridge timing on nico not run; everything else green on 7b2ff012).

## Measured first (instrumented scratch copy on mei, not the product)
`\(b[a-z]*\)@\1` on `xx bob@bob yy`: nodes 8, ring 8, room threads 25 / buckets 16 / bests 16; needs 800 u32 + 128 buckets
(does not fit `Small`: uses the lent thread scratch, no per-call zeroing of a stack block). Per call 7 frontiers prepared,
2,072 bytes filled: 296 per frontier, 256 of it the 64-slot bucket table; only 17 buckets / 17 threads used in all.

## Change (crates/core/src/pattern/regex/exec.rs)
- A frontier's bucket table is built only when it holds `LAZY_TABLE` = 8 buckets (`Frontier::build_table` fills it, or
  just exposes it when an earlier clear left it all zeros, and enters every bucket with its slot). Below that a lookup
  walks the node's bucket chain (`head`/`next`, already kept), charging the budget one unit plus one per bucket, as the
  table's probes did. At most one bucket per (node, key) exists either way, so answers are unchanged.
- `clear()` empties a built table through the buckets' slots (as before), marks it unbuilt and `table_zeroed`.
- Tables are carved after all frontiers' other buffers (`take_table`), so the touched lines of a short search are contiguous.
- Debug invariants: the table length is a power of two; a table reused without a fill is all zeros.

## Results (all on 7b2ff012, mei role s4)
- tcrb-gate: clippy + static + `cargo xtask affected --run` (direct tier, .changed vs origin/next): 4 suites ok,
  3,468 passed, 0 failed, 74 ignored; conformance 2/0; peak 2.9G, rc 0.
- tcrb-plants (`cargo xtask plants handoff/TIMING-COV-REGEX-BACKREF-SCRATCH-plants.txt`): skip-reused-clear,
  stale-between-calls, table-one-short: 3 CAUGHT with their msg, 0 WARN-RC-ONLY / WARN-NO-MSG (one WARN-DEV-SHM note, harmless here).
- Counts, `cargo xtask bench --compare --counters --runs 3 cbench_patterns` (tcrb-counts-before on cc39389c, tcrb-counts-after):
  | row | instr before | instr after | glibc | ratio before -> after | ns before -> after |
  |---|---|---|---|---|---|
  | regexec_backref | 22,932.0 | 22,029.9 | 21,958.4 | 1.05 -> 1.00 | 2,420.8 -> 2,364.9 |
  | regexec_literal_1k | 1,120.6 | 1,120.9 | | | 118.5 -> 119.3 |
  | regexec_submatch_1k | 32,217.8 | 32,226.1 | | | 2,998.6 -> 3,010.5 |
  | regexec_class_nomatch_1k | 8,155.6 | 8,160.5 | | | 940.9 -> 1,732.9 (range 948-1,767: the gate ran beside it) |
  | regexec_pathological_4k | 1,177.1 | 1,177.1 | | | 83.5 -> 83.9 |
- tcrb-percall (perf stat on the staged binaries, CBENCH_ROWS=row minus `none`, ~100k calls; after only, mei):
  Liella ~1.2 L1-dcache-load-misses and ~5.0 branch-misses per call, glibc ~0.13 and ~5.3. Neighbour check
  (class_nomatch_1k minus none): Liella 3.1k, glibc 8.0k misses in total, so the subtraction holds. The L1 event printed
  without `:u` on this VM (may include kernel). The brief's 35 L1 misses/call (keke, Zen 1) was not reproduced on mei;
  there is no before-perf-stat on mei (NOT VERIFIED).

## INFORMATIONAL
- submatch_1k +8.3 and class_nomatch_1k +4.9 instr/call (0.03 % and 0.06 %). These rows do not take the back-reference
  insert path. `run()` now has one extra `if indexed` loop (take_table) and the binary layout moved. Time for
  class_nomatch_1k is noise from the concurrent gate. The nico timing run below should confirm these rows.
- No D-row was added for `LAZY_TABLE` (local detail, documented at the constant). If one is wanted, reserve it with `ops reserve`.
- Saturation register section 15 row 6 (PERF-REGEXEC-BACKREF) could note: per-call table clearing cut, ratio 1.00 on mei.

## Next step
Book nico and time it: `~/src/liella-ops/bin/ops timing-book nico 30 --story TIMING-COV-REGEX-BACKREF-SCRATCH --reason "timing"`,
then label `nico:timing-cov-regex-backref-scratch-timing-ivb` =
`~/bin/timing <label> cargo xtask bench --compare --no-build --runs 5 cbench_patterns` on products from 7b2ff012.
Not to redo: the instrumentation (counts above), the plants, the gate.
