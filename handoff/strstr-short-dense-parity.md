# STRSTR-SHORT-DENSE-PARITY handoff (after -4: dense_4k at 0.85x on Zen 1; Haswell-EP short rows open)

State: branch task/strstr-short-dense-parity-4 (stacks on -3's branch, which is on origin/next e4370040). Role keke:~/w/n3
(Zen 1, EPYC 7551, AVX2). Commits: 01763a07 (first-byte stream after a dense last-byte switch, + test), 77b73980 (needle
copy after the terminator in that test), then docs/report. Report json: handoff/reports/STRSTR-SHORT-DENSE-PARITY-4.json.
Decisions in force: owner ruling #2686 (adaptive filter), rulings #2813/#2856 (pinned measurement; find_next's +4 on
strstr_hit_4k is a code cost, accepted), #2857 (parity per CPU class, both sides on one host). No new D-row (local kernel
change, behaviour unchanged).

## 1. Pinned re-measure, both classes (instructions:u per call, wl_percall, every haystack 16 past a 4096 boundary)
Zen 1 = keke, this run (jobs -zen1-cyc / -zen1-gl for the old head, -cnt1 / -cnt2 for the new). Haswell-EP = shiki, glibc
pinned by -3 (job -counts there); Liella's AVX2 path runs the same instructions on both classes (-3 showed equal counts),
so the Liella columns are from keke. Not re-run on shiki this run (no role there; the glibc side is already pinned).
| row | Liella before | Liella now | Zen 1 glibc | Zen 1 ratio | Haswell-EP glibc | Haswell-EP ratio |
|---|---|---|---|---|---|---|
| strstr_dense_4k | 947 | 706 | 830 | 0.85 (was 1.14) | 2253 | 0.31 |
| ss_hit_1k | 333 | 328 | 348 | 0.94 | 661 | 0.50 |
| ss_hit_64 | 138 | 138 | 167 | 0.83 | 102 | **1.35** |
| strstr_hit_short | 126 | 126 | 153 | 0.82 | 87 | **1.45** |
| ss_miss_64 | 143 | 143 | 340 | 0.42 | 176 | 0.81 |
| ss_hit_4k / ss_pairmiss_4k / ss_miss_4k | 666 / 659 / 925 | same | 828 / 947 / 7519 | 0.80 / 0.70 / 0.12 | 2245 / 2254 / 6446 | <= 0.30 |
| strstr_scan / verify / miss / hit _4k | 672 / 926 / 919 / 1227 | same | 830 / 18526 / 10158 / 10167 | 0.81 / 0.05 / 0.09 / 0.12 | 2253 / 29145 / 4833 / 4861 | <= 0.30 |
Zen 1 glibc pinned vs the old unpinned reference: dense 818 -> 830, ss_hit_64 163.2 -> 167, ss_hit_1k 355.2 -> 348.
Every strstr row is at or below 1.05x on Zen 1. Open: ss_hit_64 and strstr_hit_short on Haswell-EP.

## 2. strstr_dense_4k (profile from -3: pair loop 738 of 947 = 32 groups x 23; kernel fixed ~117; Rust 81)
Why this design: the gap sat in the pair loop, and no pair-loop reshaping reaches 1.05x (8-block groups floor 21.5 per
128 B = -48; even unbounded groups -98). The one-stream cost (16 per 128 B) applies to any needle byte: a candidate at q
needs c0 at q - off, so a one-stream filter on c0 at q - off plus the 0 test of q is exact as a filter. Change (scan.S,
liella_machine_strpair AVX2): at a DENSE one-stream stop (c1 within 1024 bytes of the last stop) the group gets the pair
test (hit or 0: block loop, as before) and, when clean, a c0 test from the pair test's L ^ c0 vectors (+6); no c0 ->
loop128c (one stream on c0; a stop goes to the block loop with r8 = pair mode, so a c0 that is no candidate continues in
pair mode); c0 present -> the pair loop as before. Rows reaching the switch: only strstr_dense_4k and ss_hit_1k (pair hit
in the switch group: -5, the old jump and mode store are gone). Rows whose head holds c1 enter pair mode at avx_mode and
never see it. Measured: dense 947 -> 706, ss_hit_1k 333 -> 328, all others 0.

## 3. Haswell-EP short rows (open, no bound proved)
strstr_hit_short 126 vs 87, ss_hit_64 138 vs 102: 1.05x needs -35 / -31. About 77 of a short call is Rust before/after
the kernel (-3's trace: 46 before, 31 after), the kernel ~40-50. Reaching 91 needs the Rust part roughly halved or the
kernel entry and the head merged; not attempted (budget). Trace one call first (validation/wl_trace.sh --list).

## Jobs (keke)
Final code head c95bb92a (+ docs). Named: -gate2 (direct tier), -plants2 (5 plants: 4 CAUGHT, 1 WARN-NO-MSG, 0 SURVIVED;
file ~/work/strstr-short-dense-parity-4/plants2.txt), -cnt2 (counts vs 449936ea, as in the table), -final (cargo clippy
--workspace --all-targets --all-features -D warnings + cargo xtask static on the final head).
WARN: c0loop-ypos (c0 stream block 1 tests Y instead of L) is caught by strstr_filter_switch_keeps_every_candidate, not by
the new test: its needle is followed by the terminator, whose stop in that group resolves the missed candidate. Next test
change: put 'a's between the needle and the terminator when found.
Superseded: -check1 (01763a07, rc 0), -cnt1 (01763a07, same counts), -plants (c0loop-nozero SURVIVED: the needle copy sat
in the terminator's group; fixed by c95bb92a), -gate (killed rc 143 when the test changed). Diagnostic: -zen1-cyc,
-zen1-gl (pinned re-measure of 449936ea, --base none).

## Next step
1. Haswell-EP short rows (hit_short 1.45x, ss_hit_64 1.35x): profile one call on the current head, then aim at the
   Rust fixed cost (46 before the kernel) or a combined entry/head.
2. Plan rows to read: docs/saturation/03 strstr rows (row "dense last byte: first-byte stream").
Not to redo: -1's DENSE hand-back and SSE2 prefix step; -2's Rust restructure; -3's pinning; 8-block pair groups for
dense (superseded by the c0 stream: it could not reach 1.05x alone).
