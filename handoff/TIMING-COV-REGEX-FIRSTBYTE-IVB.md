# TIMING-COV-REGEX-FIRSTBYTE-IVB: regexec first-byte scan, range path

**State.** Branch `task/timing-cov-regex-firstbyte-ivb`, code commit `fbe4e079` (on origin/next 7e9c5bb0), then this
handoff commit. Role mei:~/w/s1. Verdict: INCOMPLETE until the Ivy Bridge timing (the deciding measurement) is read.

**Change.** `ProgRef::first_start` (crates/core/src/pattern/regex/program.rs). Codegen check (my own probe, rustc -O,
baseline x86-64): the four-byte chunk loop compiled each byte to `shr r64, cl` (several micro-ops on the Ivy Bridge
class); a 256-byte table compiles to load + `or` per byte; a range test `b - lo <= span` over 16-byte chunks compiles
to SSE2 (`psubb`, `pminub`, `pcmpeqb`, `pmovmskb`), about 7 instructions per 16 bytes. Chosen: when the first-byte map
is ONE contiguous range (`[0-9]`, `[a-z]`, one byte, `(cat|dog)` = c-d), regcomp stores `lo` and `hi - lo` in spare
bits of the header flags word (bit 5, bits 8..24; no format bump, a block never outlives the process) and
`first_start` scans by range in 16-byte chunks after the one-byte shortcut. Other maps keep the four-byte bitmap loop
unchanged. Decision row `D-NEW` in plan/07-decisions/s14 (placeholder: no id was reserved in the brief).

**Test.** `class_tests::first_start_agrees_with_can_start_with_for_every_byte` (§14.9, s14_9_c_1_2): every byte value
at every position of windows of 1..50 bytes, against `can_start_with`, for range, 0x3f/0x40 word-boundary, single-byte
(0x01, 0x7f, 0xff), gapped (`[[:alpha:]]`, `(cat|eel)`, `[^0-9]`, ICASE) and can-be-empty maps, C and C.UTF-8; asserts the
detected range in C.

**Results (mei = Skylake-SP, jobs on fbe4e079).**
- Gate `timing-cov-regex-firstbyte-ivb-gate` (direct tier, --base origin/next): rc 0, `affected: ok`, peak 2.7G.
- Plants `timing-cov-regex-firstbyte-ivb-plants2` (`cargo xtask plants`, file handoff/TIMING-COV-REGEX-FIRSTBYTE-IVB-plants.txt):
  7 of 7 CAUGHT, no WARN. Superseded run `-plants`: `base += 16 -> 15` SURVIVED; it is an equivalent mutant (a short
  stride only rescans bytes already known not to start a match), replaced by `-> 17` (skips bytes), caught.
- Counts `cargo xtask bench --compare --counters --runs 3 cbench_patterns`, Liella instr/call before -> after (glibc, ratio after):
  | row | default | `-AVX2` |
  |---|---|---|
  | regexec_class_nomatch_1k | 8163.9 -> 1701.2 (8655.1, 0.20; time 0.21) | 8266.1 -> 1810.4 (8734.7, 0.21) |
  | regexec_literal_1k | 1121.6 -> 1120.6 | 1627.5 -> 1627.7 |
  | regexec_submatch_1k | 32229.5 -> 32224.5 | 32763.0 -> 32767.7 |
  | regexec_backref | 22033.0 -> 21985.4 | 22334.7 -> 22304.6 |
  | regcomp_regexec_regfree_email | 145516.8 -> 145551.8 (+35, 0.02%) | 146552.4 -> 146673.2 (+121, 0.08%) |
  | fnmatch_star/bracket/pathname/pathname_nomatch | within 0.6 | unchanged |
  Before = RAM copy of the role with this commit's program.rs diff reversed (jobs `-before`, `-before-noavx2`);
  after = `-after`, `-after-noavx2`. The email row's rise is regcomp's `byte_range` (popcount of 4 words; without
  POPCNT in the SSE2 build it is a software popcount); its ratio stays 0.28.

**Pending.** nico (Ivy Bridge E3-1220L V2): `timing-cov-regex-firstbyte-ivb-timing-ivb` =
`~/bin/timing <label> bash -c "cd ~/work/TIMING-COV-REGEX-FIRSTBYTE-IVB/tree && ./xtask-bin bench --compare --no-build --runs 5 cbench_patterns"`
on the staged after-products (queued for all slots at 07:40Z). No booking was made: `~/bin/timing` takes every slot itself.

**Exact next step.** `tail ~/logs/timing-cov-regex-firstbyte-ivb-timing-ivb.log` on nico. Read class_nomatch_1k's
time ratio (target < 1.0; brief: 1.145 before) and check that no regex/fnmatch row got slower in time. If it is
slower, revert fbe4e079 and record a NEGATIVE row in docs/saturation/15 row 6. Otherwise: COMPLETE; add the result to
docs/saturation/15 row 6 (not done in this run, budget).

**Open / rulings.** Email-row +35 / +121 instructions per call (0.02% / 0.08%): recommend accept (one-off regcomp cost,
the target row is 5x fewer instructions). It could be cut by testing contiguity with word masks instead of `count_ones`.
Not to redo: the codegen comparison (above); the 256-byte table (needs a new section in the serialised program for
gapped maps; a follow-up if a gapped-class row matters).
