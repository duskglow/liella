# STRCMP-SSE2-ENTRY-RECOVER (handoff; INCOMPLETE: layout is not the cause, no product change)

State: branch task/strcmp-sse2-entry-recover, stacked on task/timing-cov-strcmp-short-avx2-2 (9b2f9902). No product
change; this branch adds only the plants file handoff/STRCMP-SSE2-ENTRY-RECOVER-plants.txt and this handoff. Role
keke:~/w/n4. No D-row (no decision taken).

## What was tested
The predecessor's -2 head already makes the SSE2 head the entry's fall-through (6e563d2c), so the SSE2 class runs
lea, and, cmp vlim(%rip), jb (not taken), then the head. Disassembly: in the landed layout the SSE2 head's first block
sits one byte earlier than before the change (movdqu at +0x36 instead of +0x37); everything else on the SSE2 path is
the same code. Hypothesis tested: the +0.6 cycles come from the head's code alignment.
Timing trees (keke:~/work/SSE2R/, stage2.sh, mkscan.py in the scratch dir): origin/next 60cdbdce with scan.S =
before: next's own (the pre-change strcmp); ctl: next + this branch's strcmp function, vlim data and vlim store
(= the landed state); s1: ctl + one 0x3e prefix byte on the entry's lea (head blocks at exactly the pre-change
addresses); s3: ctl + {disp32} lea (head +3). o16 (.nops 16 before the entry) refused by the bench's own check:
exported entries must be 64-byte aligned.
Run: `~/bin/timing strcmp-sse2-entry-recover-t1 bash ~/work/SSE2R/run.sh out1 5 4 before ctl s1 s3` on nico (Ivy
Bridge, SSE2) and nozomi (N100, AVX2), 4 alternations x 5 rounds, interleaved; an.py = median of round-paired ratios.

| row | IVB ctl | IVB s1 | IVB s3 | IVB L cycles/call before / ctl / s1 / s3 | N100 ctl | N100 s1 | N100 s3 |
|---|---|---|---|---|---|---|---|
| strcmp_16_mix | 1.032 | 1.031 | 1.016 | 20.20 / 20.55 / 20.40 / 20.35 | 0.949 | 0.968 | 0.939 |
| strcmp_names | 1.009 | 1.016 | 1.007 | 26.40 / 26.80 / 26.90 / 26.60 | 0.971 | 0.970 | 0.970 |
| strcmp_64_mix | 1.006 | 1.009 | 1.007 | 33.45 / 33.55 / 33.90 / 33.90 | 0.992 | 0.985 | 0.983 |
(ratio = Liella / before; glibc drift within 0.99-1.03 on these rows.) Instructions per call equal on all four
trees (IVB 56.95 / 66.5 / 94.25; N100 51.8 / 64.5 / 71.9 for ctl, s1, s3).
Verdict: s1, which restores the pre-change head addresses exactly, keeps the +0.2-0.5 cycles: the head's layout is
not the cause. s3 is within noise of ctl (better on 16_mix and names, worse on 64_mix cycles): not committed.
The remaining difference is the entry itself: before, the SSE2 class's level test was a load-compare with no input
from a (`cmp $0, level; jne`); now the not-taken branch depends on lea/and of a plus the vlim load.

## Next step (needs a ruling)
Recovering this needs a different SSE2 entry, which changes the shared entry and so the AVX2 path the brief keeps
fixed: e.g. a level-specific `strcmp` entry chosen at dispatch time (IFUNC or a patched jump: public-interface /
loader design, maintainer ruling), or an SSE2-first order that costs the AVX2 class instructions. Recommendation:
accept the +3% on the SSE2 class (within the 1.05 parity band, AVX2 class 3-6% faster) and close, unless a dispatch-
time entry is wanted for other functions too.

## Jobs
keke: sse2r-stage (rc 1: this branch's xtask has no `bench --stage`), sse2r-stage2 (before/ctl/s1/s3 staged; o16 refused,
rc 1), strcmp-sse2-entry-recover-plants-a (refused: .S plants must name their liella_machine_ label),
strcmp-sse2-entry-recover-plants-b (4/4 CAUGHT, 0 WARN, baseline machine_scanners ok on the landed tree).
nico, nozomi: strcmp-sse2-entry-recover-t1 (rc_all=0). Bookings 66, 67 cancelled. Gate not run: no product change.

## Not to redo
- Head alignment shifts (+1, +3) at the entry; .nops before the entry (refused, entries are 64-byte aligned).
- The predecessor's shapes (AVX2 block as fall-through).
