# TIMING-ICONV-TRANSLIT-CPI (-4): iconv UTF-8 -> ASCII//TRANSLIT, more cycles than instructions

State: branch `task/timing-iconv-translit-cpi-4` (stacked on -3, cut from origin/next), code head `4bef1230`
(memo f221e5a2, D-6.14 naming 37f05369, hot-text name fix 2679c686, counted ASCII step 4bef1230); later commits are
docs only. Role mei:~/w/s3 (Skylake-SP build host). Verdict: COMPLETE, land it: ruling #3254's target is met
(utf16_4k 0.887x of base in time on Ivy Bridge, translit 0.374x).

## Decisions in force
- D-6.11 (fused UTF-8 -> wide), D-6.14 (transliterations kept for one `Conv::convert` call, `iconv::Memo`).
- The counted ASCII step is a local detail of D-6.11 (no new row): `fast::run` widens an ASCII run by its own counted
  loop, room checked once for up to eight bytes, `i`/`o` advanced once per run; past eight, `ascii_run` (out of line)
  takes the rest as before.

## What -4 did
1. Code `4bef1230` (crates/core/src/iconv/fast.rs, `run`). objdump of the mei build (`Conv::convert`, where the loop is
   inlined): the UTF-16LE inner loop is 0x451720-0x451738, 24 bytes in one 32-byte window, no pair across a boundary.
2. Ivy Bridge (nico, E3-1220L v2, booking 46, load 0.0-1.3), stage E built on mei, A and B2 from -2 (built on shiki):
   - `nico:timing-iconv-translit-cpi-4-ivb`: perf stat (2 rounds) utf16 E/A cycles 0.882, instructions 0.810;
     translit E/A cycles 0.375. A/B E vs A (A1 B A2 B, 5 rounds): utf16 **0.887** (spread 0.6%, i 0.810),
     8859-15 0.999 (i 0.999), translit 0.374 (spread 1.8%, i 0.474).
   - `nico:timing-iconv-translit-cpi-4-ivb-b2`: A/B E vs B2 (the -3 memo head): utf16 0.841, 8859-15 1.002,
     translit **1.053 slower** at i 0.985 (spread 0.7%).
3. Control (-3 open item): the 8859-15 +18% instructions of -3's C/D did not appear in E (built on another host than
   A; 8859-15 i 0.999 vs A), so it came from C/D's out-of-line code, not from the build host.
4. Counts (mei, `timing-iconv-translit-cpi-4-counts-2`, all cbench_locale rows): utf16 60,492 instr/call vs glibc
   124,664 (0.49x); 8859-15 51,558 vs 160,819; translit 187,695 vs 458,232.
5. Gate `timing-iconv-translit-cpi-4-gate` (direct tier, --base origin/next): 5 suites, 1584 passed, 0 failed, peak
   3.0G. Plants `timing-iconv-translit-cpi-4-plants` (handoff/TIMING-ICONV-TRANSLIT-CPI-4-plants.txt): 4 CAUGHT,
   1 EQUIVALENT (cap of eight vs nine), 0 WARN.
6. N100 (nozomi): booking 49 was still pending at hand-back; stages A, E and ab.sh are staged in
   ~/work/timing-iconv-translit-cpi-4/ there (run: `~/bin/timing <label> bash ~/work/timing-iconv-translit-cpi-4/ab.sh
   ~/work/timing-iconv-translit-cpi-4/A ~/work/timing-iconv-translit-cpi-4/E`). See the report for its outcome.

## Open
- Translit E vs the memo head B2: 1.053x in time at 0.985x instructions: a placement effect in `Conv::convert` like
  the utf16 one (not profiled). Against base every row is faster or equal, so it is no regression of next.
  Optional follow-up: perf stat front-end events of translit B2 vs E (time4.sh in the -4 scratch is the recipe).

## Not to redo
The memo and its plants; out-of-line `utf8_to_wide` / `run` (-3, negative); the counted ASCII step and its plants.
Plan rows: D-6.11, D-6.14; docs/saturation/06 (the utf16 row records -4).
