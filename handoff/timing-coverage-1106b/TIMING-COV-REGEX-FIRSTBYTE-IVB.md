# TIMING-COV-REGEX-FIRSTBYTE-IVB: regexec's first-byte scan is slower in time on Ivy Bridge with fewer instructions

Id: TIMING-COV-REGEX-FIRSTBYTE-IVB
Kind: code
Priority: 45
Size: S
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

## The change

`cbench_patterns/regexec_class_nomatch_1k` (`[[:digit:]]+`, REG_EXTENDED, on a 1 KiB line with no digit) is slower
than glibc in time on Ivy Bridge only (1.145) while Liella runs fewer instructions (0.946). On the N100 it is 0.862 and on
Zen 1 0.87. The time is in one loop: `ProgRef::first_start` (crates/core/src/pattern/regex/program.rs), the scan for the
first byte a match can begin with. Make that loop cheap in cycles on the Ivy Bridge class without making it slower
elsewhere.

## Rows

| row | instr IVB | time IVB | instr N100 | time N100 |
|---|---|---|---|---|
| cbench_patterns/regexec_class_nomatch_1k | 0.946 | 1.145 | 0.940 | 0.862 |

## Evidence (TIMING-COVERAGE-1106B, keke = Zen 1, next c101c4c6, job keke:tc1106b-profile; raw: profile-keke-c101c4c6.txt)

`cargo xtask bench --compare --counters --runs 3 cbench_patterns` on keke: glibc 801 ns, Liella 695 ns (0.87), instr/op
8,155 vs 8,669 (0.94). Liella `perf record` with `CBENCH_ROWS=regexec_class_nomatch_1k`: `first_start` 51.6% of
instructions:u and 53.4% of cycles:u; nothing else above 5% except program setup outside the row. The loop
(PERF-REGEXEC-BACKREF, saturation register section 15 row 6) tests four bytes per branch, each through
`(first[b >> 6] >> (b & 63)) & 1`: a 64-bit shift by a variable count per byte.

## Hypothesis

The variable-count shift is the Ivy Bridge cost: without BMI2 (`shrx`; Ivy Bridge has none and the non-AVX2 build
cannot use it) the compiler emits `shr r64, cl`, which is several micro-ops with a flags dependency on that class
and one on Zen 1 and the N100. That fits a time-only gap on one class with fewer instructions. Candidates, in order:
(a) a 256-entry byte table (`[u8; 256]` or `[bool; 256]` built at compile time next to `first`), one load and test per
byte, no shift; (b) `bt`-style bit test (check what the compiler emits; safe Rust only, `crates/core` has no `unsafe`);
(c) for sets that are one contiguous byte range (here `0-9`), a range test (`b.wrapping_sub(lo) <= hi - lo`), which also
vectorises. Check the generated loop for each (`cargo asm` or `objdump` of Liella's own object; never of glibc) and pick
by cycles on the IVB timing host, not by instruction count. The four-at-a-time structure and the first-byte shortcut of
PERF-REGEXEC-BACKREF stay.

## Tests / plants

Regex tests stay green (crates/core regex tests, conformance regex programs, the sed corpus). Add a table-driven test
of `first_start` over all 256 byte values for a few sets (class, range, single byte, empty-ok) against
`can_start_with`, name the §14.9 clause. Plants through `cargo xtask plants`, msg:<regex>: table off by one at 0x3f/0x40,
range upper bound exclusive, the tail loop after the 4-byte chunks skipped.

## JOBS

gate (direct tier, base origin/next), plants, counts before/after (`cargo xtask bench --compare --counters --runs 3
cbench_patterns` default and with `--tunables glibc.cpu.hwcaps=-AVX2`; no regex or fnmatch row slower), and a booked
timing check on Ivy Bridge (the deciding measurement): `nico:<story>-timing-ivb` =
`~/bin/timing <label> cargo xtask bench --compare --no-build --runs 5 cbench_patterns`.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
