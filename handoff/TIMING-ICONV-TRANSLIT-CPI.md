# TIMING-ICONV-TRANSLIT-CPI (-3): iconv UTF-8 -> ASCII//TRANSLIT, more cycles than instructions

State: branch `task/timing-iconv-translit-cpi-3` (stacked on -2), code = -2's code head `2679c686` (memo f221e5a2,
D-6.14 naming 37f05369, hot-text name fix 2679c686); -3 adds only docs (saturation row, this handoff, report).
Role keke:~/w/n2 (Zen 1 build host). Verdict: DO NOT LAND. The utf16 time loss is now explained (front end, code
layout of the inlined fused loop) but not fixed; the two fixes tried measured worse and were reverted.

## Decisions in force
- D-6.14: transliterations kept for one `Conv::convert` call (`iconv::Memo`, 8 slots). D-6.11 (fused UTF-8 -> wide).

## What -3 did (timing host nico, Ivy Bridge E3-1220L v2, booking 42, load ~0.0-0.4)
Stages: A = base (memo reversed) and B2 = -2 head, both from -2 (built on shiki); C and D built on keke.
1. `nico:timing-iconv-translit-cpi-3-perf` (perf stat, Liella only, 2 rounds A/B2/C interleaved, groups of <= 4
   events): utf16 B2/A cycles 1.055, instructions 1.000, idq.mite_uops 10.5x (2.5% of uops), dsb2mite penalty 15x,
   ild_stall.lcp 8.5x, idq_uops_not_delivered 1.12x; branch misses, store forwards, resource stalls not worse.
   Cycles per run: A 2.844-2.883G, B2 2.995-3.027G. Translit B2/A cycles 0.355.
2. Hot IPs (cycles, idq.mite_uops, dsb2mite_switches.count; `nico:timing-iconv-translit-cpi-3-perf2`): all in the
   ASCII step of the fused UTF-8 -> UTF-16LE loop (`fast::run::<2,false,false>`), which is inlined into
   `Conv::convert` (`fast::utf8_to_wide` has no symbol: its hot-text key matches nothing). Same instructions in
   A/B2/C; only the offset in 32-byte windows differs (A loop top 0x452083, C 0x453249). In C the switches sit on
   `mov $1,%ecx` crossing a 32-byte boundary (975 samples) and on the `cmp $7`/`ja` run check.
3. C = `#[inline(never)]` on `utf8_to_wide` (own 64-byte-aligned hot-text function, after convert). D = it on each
   `fast::run` instead. `nico:timing-iconv-translit-cpi-3-ivb`, xtask bench A/B vs A (A1 B A2 B, 5 runs):
   C: translit 0.363, utf16 1.017 (spread 0.6%), 8859-15 **1.182, instr 1.178**; D: 0.362 / 1.021 (0.4%) /
   **1.120, instr 1.178**. Both reverted (saturation row in docs/saturation/06). perf stat, same job: C cycles
   about 1.018x A on utf16.
4. Gate on the final head: see the report (handoff/reports/TIMING-ICONV-TRANSLIT-CPI-3.json).

## Open
- The 8859-15 +18% instructions in C and D is unexplained. Both stages were built on keke from the same source,
  while A/B2 were built on shiki. Control first: build B2 on keke and compare its 8859-15 instructions with
  shiki's B2 (`cargo xtask bench --compare --counters` on the role is enough; no timing host needed).
- RULING NEEDED (still): land the memo with utf16 1.05x of base in time on Ivy Bridge, or not. Recommendation:
  not yet; the next step below targets the layout directly.

## Exact next step (-4)
1. The control build above (instructions only).
2. Make the ASCII step of `run` layout-robust instead of moving it: a tighter ASCII inner loop (room checked once
   per run, not per byte; no shared tail with the multibyte step, which today makes LLVM materialise `mov $1`/`mov
   $2` per byte) cuts its uops and the windows it spans. Check the loop's 32-byte windows with objdump before
   timing (no instruction or macro-fused pair across a boundary), then time A vs head on the three rows (time3.sh
   recipe in /home/claude/w/.scratch/TIMING-ICONV-TRANSLIT-CPI-3/; stages A/B2 stay on nico ~/work/timing-iconv-
   translit-cpi-2, C/D on ~/work/timing-iconv-translit-cpi-3).

## Not to redo
The memo and its plants; the hot-text name fix; out-of-line `utf8_to_wide` or `run` (both measured above).
Mangled names of `run` back-reference repeated const args (`Kj2_Kb0_KBL_` = <2,false,false>), so a grep for
`Kb0_Kb0_` finds nothing. The bench row's text (16-byte pattern "abcdefg é h €.") never reaches `ascii_run`.
Plan rows: D-6.11, D-6.14; docs/saturation/06.
