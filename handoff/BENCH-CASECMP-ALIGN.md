# BENCH-CASECMP-ALIGN (-2): strcasecmp placement sensitivity fixed in the kernels; bench pinned

State: branch task/bench-casecmp-align-2 (stacks on task/bench-casecmp-align), role keke:~/w/n3 (Zen 1, EPYC 7551).
Code commits: 105e122c1 (AVX2 kernel), 61842017f (SSE2 back-up), c73c5d06f (bench rows), e574c147c (plants),
922616e50 (byte loop to page end). Plants file: handoff/plants/BENCH-CASECMP-ALIGN-2.plants.

## Cause (profiled from the kernel and an offset sweep)
`liella_machine_casecmp_ord_avx2` (csys/src/machine/scan.S):
1. Its head page test looks at the byte BEFORE each start (folds the NULL test in), so a page-aligned start failed it
   and took the generic loop: +12 (256 bytes) / +36 (strncasecmp 64). The previous run pinned case_a at offset 0,
   which is exactly that case: the 262 / 103 it measured.
2. A 32-byte block that would cross a page fell to byte steps (about 30 instructions each, up to 31 bytes) whenever
   the larger page offset was above 4064: 554-1949 instructions where glibc stays at 211-270.

## Fix
- Head: after the NULL tests an exact test (offset <= 4032 for both) sends page-aligned starts back to the head.
- `.Lcord_back`: the block backs both strings up by k = maxoff - 4064 so it ends at the page end (when rax >= k the
  k bytes are already passed). At a start in the last 31 bytes (rax < k) the block starts before the strings, and
  its lanes before the starts are cleared, if the other string's page offset is >= k. Otherwise the kernel takes byte
  steps in a tight loop to the nearer page end (no page tests per byte). SSE2 kernel: the rax >= k back-up and the
  tight byte loop; its start-near-page-end case still byte-steps (next step).
- Test: casecmp_avx2.rs `casecmp_reads_neither_neighbour_page_nor_the_bytes_before_the_strings_at_any_start_offset`
  (pages fenced on both sides; NUL before a, '#' before b).

## Sweep (instructions per call, keke, validation/wl_percall.sh, spec in ~/work/BENCH-CASECMP-ALIGN-2/casecmp_sweep.c
on keke; copy in the hand-back). glibc / before (sweep0, 49457b455) / after (sweep2, 922616e50)
| case (a off, b off, len) | glibc | before | after |
|---|---|---|---|
| s256 0x000/0x800 (= strcasecmp_c_256) | 233 | 266 | 262 |
| s256 0x000/0xf00 (= _xpage) | 235 | 301 | 297 |
| s256 ordinary (0x001/0x013, 0x010/0x013, 0x040/0x840 ...) | 233-241 | 254 | 254 |
| s256 0x013/0xfc8 | 231 | 1129 | 318 |
| s256 0x013/0xfe9 | 232 | 1058 | 298 |
| s256 0xfd0/0xfe8 | 258 | 1949 | 329 |
| s256 0xff0/0x013 | 211 | 802 | 289 |
| s256 0xff0/0x005 (byte fallback) | 211 | 802 | 537 |
| s256 0xffc/0xff1 | 256 | 766 | 512 |
| s256 0x013/0xff7 | 233 | 554 | 427 |
| s16 0x000/0x800 | 53 | 91 | 65 |
| s16 0xff8/0x013 | 161 | 386 | 273 |
| n64 0x000/0x800 (= strncasecmp_c_64) | 71 | 107 | 82 |
| n64 0x013/0xfe0 | 166 | 111 | 119 (+8: the exact head test when it fails) |
| n64 0xffa/0x013 | 164 | 294 | 219 |
Jobs: keke:bench-casecmp-align-2-sweep0 (before), -sweep1 (intermediate, superseded), -sweep2 (final).

## Bench
cbench_locale: operands stay pinned (case_a 0, case_b 0x800, xpage at 0xf00); new rows strcasecmp_c_256_odd
(0x13 / 0x421) and strcasecmp_c_256_pageend (start at 0xff0); every operand's page offset printed to stderr.

## Gate / plants
keke:bench-casecmp-align-2-gate2 (clippy, static, affected --base origin/next) and keke:bench-casecmp-align-2-plants2
(9 plants, 1 declared equivalent) on 922616e50: see the report json for results. Superseded: -lint0, -t1 (green,
older head), -gate and -plants (killed: head changed).

## Open / next step
1. Remaining gap at the start-near-page-end byte fallback (one string in the last 31 bytes of a page and the other
   within k bytes of its page start): 427-537 vs glibc 211-233. Design: realign the backed-up block of the page-end
   string through a 64-byte stack buffer (store, reload at +k), valid lanes < 32 - k; no byte steps at all.
2. The SSE2 kernel's start in the last 31 bytes still byte-steps (tight loop); port the AVX2 back_start masking.
3. Ordinary placements are still 254 vs glibc 233 (+9%): the per-64-byte step cost, not placement.
4. `cargo xtask bench --compare --counters cbench_locale` on this head for the two new rows, and Ivy Bridge timing:
   NOT RUN (budget) -> PENDING.
Plan rows: D-5.5, D-5.6; rulings #3856, #3902.
