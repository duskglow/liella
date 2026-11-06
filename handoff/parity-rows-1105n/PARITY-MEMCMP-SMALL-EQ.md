# PARITY-MEMCMP-SMALL-EQ: memcmp family small equal sizes and 128-byte mismatch tail, instructions:u

Id: PARITY-MEMCMP-SMALL-EQ
Kind: code
Priority: 70
Size: S
Host: -
Model: opus
Effort: high
BUDGET: 70 tool calls / 140k

Counter run: label parity-rows-1105n-counts, `cargo xtask bench --compare --counters --runs 3`, origin/next d1a7f653 (counter host). Per call: label parity-rows-1105n-percall2 (validation/percall/memcmp_bands.c, memcmp_eq32.c; deterministic, no perf annotate).

## Rows (instr ratio, time ratio)
__memcmpeq_tail_32 1.55 / 1.13; bcmp_tail_32 1.29 / 1.00; memcmp_diff8_64 1.25 / 1.13; memcmp_diff8_127 1.21 / 1.25; __memcmpeq_head_128 1.20 / 1.00;
memcmp_diff8_96 1.20 / 1.25; __memcmpeq_eq_32 1.19 / 1.00; __memcmpeq_head_1k 1.19 / 0.19; bcmp_eq_32 1.15 / 1.00.

## Profile (percall, instructions per call, Liella vs glibc)
- equal inputs of 17-32 bytes: meq_eq_32 34 vs 27 (1.26), meq_eq_24 and _17 34 vs 28 (1.21), bcmp_eq_32 34 vs 28; sizes 12/16 are at or below parity (26 vs 28). The 17-32 class costs 7-8 more than the 12-16 class: a second overlapping load pair plus a longer join.
- mismatch-in-first-block: memcmp_hd_96/128/256/1024/2048 25 vs 32 (1.28) and meq_hd_128 21 vs 27 (1.29); sizes 17-64 are fine (24-25). The class entered above 64 pays 6-7 extra instructions before the first 32-byte block (class gate, head block setup).
- larger equal sizes (1024-2048) are already below parity (0.93).
Hypothesis: the surplus is fixed entry/join cost in two classes (17-32 equal, >64 first-block mismatch), not scan width. PERF-MEMCMP-TAILS (handoff/perf-memcmp-tails.md) reshaped the 65-2048 gate; check what it left and do not redo it.

## The change
Files: csys/src/machine/fold.S (labels liella_machine_compare and liella_machine_equal; grep -n the 17-32 and >64 classes, read about 80 lines each), crates/boundary/src/dispatch.rs only if a gate value changes (grep -n cmp_long). Tests: grep -rn "machine_scanners\|small_copy" crates/boundary/tests for the existing size-sweep tests.
Shorten the 17-32 equal path (two overlapping 16-byte loads, one combined test) and move the first-block mismatch for sizes above 64 so it is reached with the same instruction count as the 33-64 class. Results unchanged (sign and zero for memcmp; zero/non-zero for bcmp and __memcmpeq): exhaustive test over every size 0-300 and every first-difference position, both dispatch paths. Every path meets the spec; run-time dispatch for wider paths, test both. Target: all rows above at most 1.05x in instructions, memcmp_diff8_* at most 1.0x in time. If a remaining gap is below 3 instructions, record it in the saturation register with the numbers. PLANTS ON .S FILES: the affected tier selects nothing for csys/src/machine/*.S; name the tests explicitly (-p liella-boundary --test machine_scanners) and anchor each perl plant to the function label.

## JOBS (label each with the story id in lower case)
- gate: direct-tier gate (`cargo xtask affected --base origin/next --run`), clippy and `cargo xtask static` clean first.
- plants: `cargo xtask plants <file>` (docs/plants.md), one plant per changed branch, each with a msg:<regex>.
- counts before/after: `cargo xtask bench --compare --counters --runs 3 cbench_memref` on the unchanged and the changed tree; no row of the same bench slower.
- percall: `bash validation/wl_percall.sh --base none --spec validation/percall/memcmp_eq32.c --locale en_US.UTF-8 .` before and after (instructions:u per call).
Context discipline: read only the named ranges (grep -n first); never whole large files. Profile first (rules/developer-flow.md): write where one call's instructions go before designing.
