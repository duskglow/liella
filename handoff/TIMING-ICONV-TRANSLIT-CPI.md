# TIMING-ICONV-TRANSLIT-CPI (-2): iconv UTF-8 -> ASCII//TRANSLIT, more cycles than instructions

State: branch `task/timing-iconv-translit-cpi-2`, code head `2679c686` (memo f221e5a2 + D-6.14 naming 37f05369 +
hot-text name fix 2679c686), then docs/handoff commits. Role shiki:~/w/sh2 (Haswell-EP build host). Verdict: DO NOT
LAND until the utf16 time loss below is explained or accepted by a ruling.

## Decisions in force
- D-6.14 (plan/07-decisions/s06-...md): transliterations kept for one `Conv::convert` call (`iconv::Memo`, 8 slots).
  The placeholder D-NEW is gone from the plan row, mod.rs and the test doc (ruling #3223).

## What -2 did
1. D-NEW -> D-6.14 (37f05369). Static + clippy: `timing-iconv-translit-cpi-2-static` rc 0 (superseded by the gate job).
2. Ivy Bridge timing (nico, E3-1220L v2, load 0.00-0.3, queue empty, booking 41): stages A = head with the memo diff
   reversed (only mod.rs differs), B = head; `xtask-bin bench --no-build --ab-from A --only cbench_locale --rows <3 rows>
   --runs 5` (A1 B A2 B per round). Job `timing-iconv-translit-cpi-2-ivb` (rc 1: its second step used `--compare
   --rows`, which `--compare` refuses by design; the A/B part ran): translit 0.364, 8859-15 0.996, utf16 **1.044
   SLOWER** (spread 0.2%), instructions 0.481 / 0.999 / 0.999.
3. Cause hunted: `impl Memo` placed before `impl Conv` changed Conv's impl disambiguator (Ms_ -> Ms0_), so
   `Conv::convert` and `Conv::from_words` no longer matched their crates/libc-so/hot-text.list keys (convert moved from
   0x75f00 next to `iconv` to 0xd9dc0). Fix 2679c686: `impl Memo` moved after `impl Conv` (comment says why); B2
   puts convert back at 0x75f00. **It did not fix utf16:** job `timing-iconv-translit-cpi-2-ivb2` (A vs B2, then plain
   bench with --rows, which interleaves Liella/glibc per round, B2 A B2 A): A/B translit 0.354, 8859-15 0.997, utf16
   **1.052** (spread 0.5%), instructions 0.481 / 0.999 / 0.999. Cycles Liella/glibc, head (base): translit 0.469
   (about 1.33), utf16 0.671-0.672 (0.636), 8859-15 0.275-0.277 (0.276-0.277). Logs: nico ~/logs/<label>.log.
4. Gate `timing-iconv-translit-cpi-2-gate` (static + clippy -D warnings + affected --base origin/next) on 2679c686:
   5 suites ok, 1584 passed, 0 failed, peak 3.5G. Plants `timing-iconv-translit-cpi-2-plants`
   (handoff/TIMING-ICONV-TRANSLIT-CPI-plants.txt): 5 CAUGHT, 1 EQUIVALENT, 0 SURVIVED, 0 WARN.
5. Saturation row added (docs/saturation/06-...md).

## Open
- RULING NEEDED: utf16 1.052x of base in time on Ivy Bridge at equal instructions vs translit 0.354x. Recommendation:
  do not accept; a -3 run profiles it first (below), then fixes the layout of the fused path.
- INFORMATIONAL: hot-text.list keys depend on impl-block order (`Ms_`/`Ms0_`); tools/hottext/keys.py could strip
  impl disambiguators, or static could flag a list entry that matches no symbol in the build.

## Exact next step (-3)
Profile, do not guess: on the Ivy Bridge host (book it), `perf stat` the utf16 row of stages A and B2 (cycles,
instructions, idq.dsb_uops, idq.mite_uops, lsd.uops, dsb2mite_switches, branch-misses) with the bench binary under
CBENCH_ROWS=iconv_utf8_utf16_4k; nm-compare the fused utf8_to_wide loop inside `Conv::convert` (code size 0x43ce in A,
0x30d8 in B2: the loop moved within the function). If it is front-end (DSB/LSD), try the fused UTF-8 -> wide path out
of line in its own function so its layout does not depend on the rest of convert; re-time A vs head the same way
(time2.sh recipe: stage via a script on shiki that reverses memo.patch for A; scratch in
/home/claude/w/.scratch/TIMING-ICONV-TRANSLIT-CPI-2/).

## Not to redo
The memo and its tests/plants; the hot-text name fix; flags-merge/store-forward hunting in the old inlined body.
Staging: the tar needs target/liella, bench/, xtask/Cargo.toml, Cargo.toml, crates/, csys/ and the debug xtask as
xtask-bin; `--compare` takes no `--rows` (use the plain bench with --rows, it interleaves glibc per round).
Plan rows: D-6.11, D-6.14; docs/saturation/06.
