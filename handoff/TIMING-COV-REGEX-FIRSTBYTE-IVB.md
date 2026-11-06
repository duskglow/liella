# TIMING-COV-REGEX-FIRSTBYTE-IVB(-2): regexec first-byte scan, range path

**State.** Branch `task/timing-cov-regex-firstbyte-ivb-2`: code commit `fbe4e079`, D-14.21 assigned (`cef85ca3`), origin/next
merged (`e92c9d5d`, for the staged `bench --compare` fix, XTASK-BENCH-COMPARE-STAGED), then this handoff commit. Role kanon:~/w/kn1.
Code unchanged in -2. Verdict: target met in time; two off-target rows slower in time by 1-2% -> RULING NEEDED (below).

**Change (fbe4e079).** `ProgRef::first_start` (crates/core/src/pattern/regex/program.rs): when the first-byte map is ONE
contiguous range (`[0-9]`, `[a-z]`, one byte, `(cat|dog)`), regcomp stores `lo` and `hi - lo` in spare header flag bits
(bit 5, bits 8..24) and the scan tests `b - lo <= span` in 16-byte chunks (SSE2 compares) instead of a variable shift per
byte. Other maps keep the bitmap loop. Decision D-14.21 (plan/07-decisions/s14). Test
`class_tests::first_start_agrees_with_can_start_with_for_every_byte` (§14.9).

**Why the -1 timing failed.** nico job `timing-cov-regex-firstbyte-ivb-timing-ivb` ended rc 1 in 1 s ("cbench_patterns is not
a benchmark"): the tree was hand-staged without Cargo.toml / bench-stage.manifest, so xtask used the build host's path.
Fixed by staging with `cargo xtask bench --stage` (whole tarballs) after merging next.

**Ivy Bridge timing (-2).** Staged on kanon by job `timing-cov-regex-firstbyte-ivb-2-stage2`: head.tar (tree bbc3eb13) and
base.tar (= head with origin/next's program.rs, tree fe5394ad; restored with `cat`, fresh mtime, cmp-checked). On nico
(booking 81, cancelled after): `~/bin/timing timing-cov-regex-firstbyte-ivb-2-timing-ivb bash ~/work/TIMING-COV-REGEX-FIRSTBYTE-IVB-2/run.sh 3 3`
(script: handoff/TIMING-COV-REGEX-FIRSTBYTE-IVB-2-run.sh; 3 alternations base/head, each `./xtask-bin bench --compare
--no-build --counters --runs 3 cbench_patterns`), rc 0, 13 min. glibc's ns per row stayed within 0.2% over all 6 runs:
all medians come from one clock state (2.3 GHz from glibc cycles/ns), so no per-speed split was needed.
| row | Liella cycles base -> head | time ratio to glibc base -> head | instr head |
|---|---|---|---|
| regexec_class_nomatch_1k (target) | 2714 -> 626 | 1.158 -> 0.263 | 8266 -> 1812 |
| regcomp_regexec_regfree_email | 63211 -> 64635 (+2.3%) | 0.337 -> 0.342 (+1.3% ns, every head run above every base run) | +134 |
| wordexp_arith | 2021 -> 2072 | 1.494 -> 1.525 (+2%, all 3 runs) | equal (4186) |
| literal, submatch, backref, pathological, fnmatch x4, glob x2, wordexp_tilde_var | head/base 0.99-1.01 | within spread | +-30 |
Email: +134 instructions (regcomp's one-off `byte_range`) but +1100 uops and +1400 cycles: more than those instructions
explain; wordexp_arith runs no regex code and has equal instructions and uops. Both read as code-layout / branch-address
effects of the larger regex module (NOT proven: no branch-miss counter was read).

**Gate and plants on the final code head (e92c9d5d), kanon.** Clippy (`--all-features -D warnings`) and `cargo xtask static`
clean. Gate `timing-cov-regex-firstbyte-ivb-2-gate` (direct, --base origin/next): rc 0, 3509 passed / 0 failed, conformance
2/2, peak 3.3G. Plants `timing-cov-regex-firstbyte-ivb-2-plants` (handoff/TIMING-COV-REGEX-FIRSTBYTE-IVB-plants.txt): 7/7
CAUGHT, no WARN.

**Instruction counts (from -1, Skylake-SP, unchanged code).** class_nomatch_1k 8163.9 -> 1701.2 (glibc 8655.1), -AVX2
8266.1 -> 1810.4; email +35 / +121 (ruling #3496 accepted).

**RULING NEEDED.** Land despite email +1.3% and wordexp_arith +2% in time on the Ivy Bridge class? Recommend accept: the
target row is 4.3x faster (1.158 -> 0.263 of glibc); email stays at 0.34 of glibc; wordexp_arith's shift has no
instruction or uop change. If not accepted: next step is a branch-miss/layout check of the email row (perf
`branch-misses` base vs head on a timing host) before any code change.

**Not to redo.** The codegen comparison (shift vs table vs range); the staging recipe above works. Optional follow-ups:
contiguity test without `count_ones` (cuts the email row's +134 instructions); 256-byte table for gapped maps.
Saturation register: docs/saturation/15 row 6 follow-up paragraph added.
