# sprintf / __sprintf_chk ls nlink line (one page; PARITY-SPRINTF-CHK-LS-2)

State: branch task/parity-sprintf-chk-ls-2 on origin/next ff125cce. Code commit 35ad1c95 (kept); ae05a3fe (printf_text
inline) reverted by the commit that carries this note. Role keke:~/w/n1. PERF-SPRINTF-CHK-LS (Out::room) is on next.

## Profile before (job parity-sprintf-chk-ls-2-trace0, wl_trace spr_ls sprintf, call 11, ff125cce)
TOTAL 809 instructions (glibc percall 714.5 spr_ls / 731.5 chk_ls; Liella percall 825.5 / 828.5 = 1.16 / 1.13).
Per phase (from --list): entry sprintf + vsnprintf6 + strlen(fmt) 82; vsnprintf frame + plan zeroing ~35; parse
(simple_into: two simple_conv, simple_star, the `%` scans, recording) ~300; pair_s extraction ~45; three va_long 30
+ two printf_text 82 (strlen 22 of it) + glue ~20; **in-place render 226**; epilogue + NUL ~38.
The render was the largest frame: each of the 6 writes paid a saturating offset, a range re-check and mem::copy's
32/16/8/4/2/1 size steps (5 compares for a 1-byte piece) plus spill reloads.

## Change (35ad1c95)
`place` / `pad` walk the lent room with `split_at_mut_checked` (one length test per piece); pieces of 0-2 bytes
(padding of 0-1) are stored directly, longer ones still go through mem::copy / mem::fill. Trace 809 -> 738.
Bytes and order unchanged; the piecewise-put path (no room, cut snprintf, object too small, > INT_MAX) is untouched.
Coverage: conformance/c/s12_printf.c clause s12_6_c_r4 (widths -40..40 via `*`, fixed widths, left/right, text
around/none, multibyte, string longer than its field, precision cuts, sprintf / __sprintf_chk / snprintf at every
bound); the plants show it reaches every new branch. No new test needed.

## NEGATIVE: printf_text #[inline] (ae05a3fe, reverted)
Trace 738 -> 729 on spr_ls, but cbench_opt counters (counts1 vs counts0) had snprintf_ls_line 2403.6 -> 2415.2,
snprintf_s_d_colon 956.4 -> 958.2, snprintf_s_path 228.2 -> 229.2 (other %s paths). Not worth 9 instructions.

## Jobs (keke)
Before (ff125cce): -trace0, -percall0, -counts0 (sprintf_chk_ls_nlink 854.2 vs glibc 730.2 instr/op, 1.17).
On ae05a3fe (SUPERSEDED by the revert): -lint rc 0; -plants 7 CAUGHT + 1 EQUIVALENT; -gate rc 0 (affected ok,
conformance 109 passed, peak 12G); -percall1 spr_ls 749.0 (1.05), chk_ls 752.0 (1.03), chk_cpc 375.0, chk_lu 437.4;
-counts1 sprintf_chk_ls_nlink 749.8 (1.03), rows above slower.
On the final head (started detached at hand-back, ~/work/parity-sprintf-chk-ls-2/after2.sh): -gate2, -percall2,
-counts2, -plants2 (plants2.txt: place-one, place-two, place-long, place-advance, pad-one, pad-long, pad-advance, each
msg `conformance: 0 passed, 1 failed`; all CAUGHT on ae05a3fe with the same code).

## Open / next step
- Read -percall2 / -counts2: expected spr_ls ~758 (1.06x, target 1.05x probably NOT reached by ~1%), chk_ls ~1.04x;
  check that counts2 has no row above counts0 beyond the noise seen on untouched rows (iconv/regcomp/strcoll +2..+13).
- Next frames: the parse (~300: simple_conv out of line returning through memory, simple_star 37 for one `*`) and
  strz's per-string frame. A plan cache keyed by the format pointer is NOT safe for sprintf (bytes at an address
  can change). Inlining printf_text is a measured negative (above). Do not redo byte-by-byte padding / double strlen.
- Timing (the row's verdict) is left to the periodic sweep; cbench_opt 98.61 vs 97.27 ns (1.01) on counts1.
