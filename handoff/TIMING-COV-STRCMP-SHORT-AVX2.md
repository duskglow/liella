# TIMING-COV-STRCMP-SHORT-AVX2 (handoff)

State: branch task/timing-cov-strcmp-short-avx2, stacked on task/timing-ivb-strscan-dispatch-2 (883a05df). Code commits
f05d3094 + the "Serve strcmp's AVX2 page-end starts" commit (csys/src/machine/scan.S, crates/boundary/tests/machine_scanners.rs);
validation/percall/casecmp_short.c (new spec); plants handoff/TIMING-COV-STRCMP-SHORT-AVX2-plants.txt. Role mei:~/w/s3.
PERF-STRNCASECMP-64-AVX2 (#3313 ruling: merge it first): already in the base (task/perf-strncasecmp-64-avx2-2 is in next and an
ancestor of 883a05df), so no merge was needed.

## Change
- strcmp entry: `cmpl liella_machine_strcmp_vlim(%rip)` on (a - 1) & 4095 is the level test AND the AVX2 head's page test
  of a (vlim 3968 on AVX2, 0 on SSE2, set in liella_machine_set_scan_level as 4033 & 0xf80). The AVX2 first 16-byte block is
  the fall-through (was: cmpl level / jne taken / own test of a). Every other call (SSE2 level; AVX2 with a in its page's
  last 128 bytes or NULL) jumps to the SSE2 head, whose `cmpl $4031` sends page ends / NULL to the SSE2 slow path (NULL tests).
  SSE2 level: same instruction count as before (one taken jae instead of a not-taken jne: Ivy Bridge timing NOT run).
  First try had a second level-set limit plus a level test in the slow path: +2 on AVX2 page-end-of-a calls, which cancelled
  the gain on strcmp_names (half its first strings start in a page's last 128 bytes). Dropped; do not redo.
- casecmp_ord_avx2 head: NULL tests + or-test (9) -> exact per-string test of the 65 bytes from x - 1 (limit 4031, 8),
  which covers both 32-byte head blocks, so the bound <= 64 path's second or-test (5) is gone; NULL / page-aligned / page-end
  starts go to the NULL tests then the loop from 0 (`.Lcord_generic`). `.Lcord_loop_in` removed (dead).
- Test: strcmp_head_finds_every_difference_in_strings_ending_at_a_page_end (lengths 0-130 at a page end before an unmapped
  page, b at a page end or mid-buffer, difference greater/smaller/ended at every position, both orders, both levels).

## Results (mei, AVX2 class; instructions:u per call, wl_percall; glibc / before 883a05df / after)
strcmp_names 35.0 / 46.4 / 42.4; strcmp_16_mix 47.5 / 59.8 / 57.8; strcmp_64_mix 60.4 / 75.7 / 73.7;
strncasecmp_c_64 64.0 / 79.0 / 73.0; strcasecmp_c_16 49.0 / 57.0 / 56.0. No row slower; check = ok on all.
Job timing-cov-strcmp-short-gate3 rc 0 (one chain): clippy, static, machine_scanners 50/50, casecmp_avx2 6/6,
conformance x_overrides, plants 5/5 CAUGHT (0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG; WARN-NOT-RAM: plant dir not on
tmpfs), both percall tables. `cargo xtask affected` alone only lists (230 targets, FULL-scale, not run: coordinator's).

## NOT VERIFIED / open
- Timing not run (budget): nozomi:timing-cov-strcmp-short-time (N100, confirm the class first) and
  nico:timing-cov-strcmp-short-ivb (Ivy Bridge, SSE2 path: the entry's jae is now taken on every SSE2 call). Method as
  handoff/TIMING-IVB-STRSCAN-DISPATCH.md (bench --stage before 883a05df / after this head, interleaved run.sh).
- Instruction gap left on strcmp_names: 7.4 (fixed head: page test of b 4, a 4, block 7, settle 3, return 3).
- Bench --compare --counters rows not run; percall only. No D-row written (local detail; the SSE2-steps-on-AVX2 page-end
  choice follows strchr's precedent, D-4.45).

## Next step
Book nozomi and nico, stage before/after, run the strcmp_names / strcasecmp_c_16 / strncasecmp_c_64 rows (+ strcmp mix rows).
