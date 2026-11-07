# BENCH-CASECMP-ALIGN (INCOMPLETE: not layout alone; do not land as is)

State: branch task/bench-casecmp-align, code commit c90ece4dc on origin/next 5872ad47e (handoff commit on top).
Role mei:~/w/s1 (Skylake-SP, Xeon Platinum 8151).

## What was done
- bench/c/cbench_locale.c: case_a/case_b moved into a page-aligned block `case_pages[3][4096]`
  (case_a = page 0 offset 0, case_b = page 0 offset 0x800, neither crosses a page); iconv buffers text/out
  `_Alignas(4096)`. New row `strcasecmp_c_256_xpage`: strcasecmp(case_a, case_x), case_x = copy of case_b at
  page 1 offset 0xf00 (terminator on page 2). bench/budgets.tsv row added for it.
- Other static operand arrays in bench/c/ whose address can drift, NOT changed (out of this bench's before/after):
  cbench_patterns line[1025], aaaa[4097] (regexec operands); cbench_hot rbuf/wbuf[8192] (stdio buffers, the
  position cycles through the whole buffer), line[10001]; cbench_netdb text[64], addr4/addr6, query/msg[512],
  expanded; cbench_aio buf[16][4096], block[64K]; cbench_fstree path buffers; cbench_libm af..bq (scalar inputs).

## Counters (mei, `cargo xtask bench --compare --counters cbench_locale`, instructions per call, Liella / glibc)
| row | before (next 5872ad47e, unaligned) | after (c90ece4dc, offsets 0 / 0x800) |
|---|---|---|
| strcasecmp_c_16 | 53.0 / 45.0 | 53.1 / 45.1 |
| strcasecmp_c_256 (= utf8_256, en_256) | 250.2 / 202.1 | 262.4 / 197.3 |
| strcasecmp_c_256_xpage (b at 0xf00) | - | 297.4 / 199.3 |
| strncasecmp_c_64 | 70.0 / 60.0 | 103.1 / 60.1 |
| iconv_* 4k (3 rows) | within 0.1% | within 0.1% |
Jobs: mei:casecmp-align-cnt-before, mei:casecmp-align-cnt-after (both rc 0).

## Verdict
Pinning the operands did NOT bring the rows back to the pre-WCTYPE-L-2 level (about 250 - 35 = 215 by the
+35 in #3856). With neither operand crossing a page, Liella's strcasecmp_c_256 costs 262.4 and strncasecmp_c_64
costs 103.1 (+33). glibc stays flat (197-202, 60). So Liella's case-compare cost depends heavily on operand
addresses even when no page is crossed. Mutually aligned operands (same offset modulo 2048) are the slow case
here. That is a defect in the code (placement sensitivity), not a flake and not only page crossing. The page-
crossing case costs +35 over the aligned case (297.4 vs 262.4).
Not done: gate (clippy/static/affected), plants (bench-only change), Ivy Bridge timing (PENDING; booked timing not
started, budget).

## Next step
1. Black-box sweep: Liella vs glibc instruction count for strcasecmp/strncasecmp over (offset a, offset b) in a page
   (e.g. a, b in {0, 1, 7, 8, 15, 16, 31, 32, 63} x page-end offsets 0xfc0..0xfff), with validation/wl_percall.sh
   or a small probe. Find which placements take the slow path and profile it (casecmp_ord_avx2 / its prologue).
2. Fix the code so the cost is flat across placements. Then pick representative bench offsets (e.g. a=0x0, b=0x13)
   so the rows are not on a best or worst case by accident, and keep the _xpage row. Then gate and time.
Plan rows: D-5.5, D-5.6 (casefold design); ruling #3856.
