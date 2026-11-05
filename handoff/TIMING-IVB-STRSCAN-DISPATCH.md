# TIMING-IVB-STRSCAN-DISPATCH (handoff)

State: branch task/timing-ivb-strscan-dispatch, code commit 9b6419cc (scan.S only), later commits handoff/ only.
Role mei:~/w/s2 (Skylake-SP, AVX2). Ivy Bridge timing NOT DONE: nico booking #40 (until 22:54Z); jobs -ivb (no root Cargo.toml in the stage: xtask fell back to the build path) and -ivb2 (bench --no-build: "libc.a is older than its" sources after tar extraction) failed. Stage exactly per handoff/timing-sweep-1105.md next time.

## Change (csys/src/machine/scan.S)
- strchr / strchrnul: the entry's `jae` (taken on every SSE2-class call) now lands directly on the SSE2 64-byte head,
  aligned to 32 bytes. Before: entry `jae` to a far block, a second mask load (`liella_machine_schr_mask`) and a
  second taken branch back to the head at a mid-window address. The head's own compare (ecx < 4032) already separates
  SSE2 in-page calls; on the AVX2 level ecx >= 4032 always holds there, so AVX2 page-end and NULL calls leave through
  the existing NULL test into the aligned body. `liella_machine_schr_mask` and its setter are removed (no other user).
  Verified in the built libc.so.6: strchr's `jae` target is strchr+0xa0, strchrnul's strchrnul+0x80 (32-aligned).
- strncmp: `.Lsnc_pt` (the entry's taken branch) is 32-aligned; the SSE2 level (short_lim 0) falls through the exact
  page test into the SSE2 steps (before: a taken `jae` to the adjacent `.Lsnc_old`); the AVX2 test of b moved out of
  line (`.Lsnc_ptb`). Two dispatch loads remain on the SSE2 strncmp path (rshort_mask, short_lim; both predicted).
- AVX2 level cost: a page-end call (s page offset 0 or 4033..4095) now runs `cmpl $4032; jae` (+2 instructions, one
  load fewer, one more taken branch); no bench row reaches it (AVX2 counts unchanged on every row below).

## Results (mei, instructions Liella/glibc, cbench_hot, counted runs; before = next ff125cce, after = this branch)
| row | AVX2 before/after | SSE2 class before/after |
|---|---|---|
| strchr_16_hit | 1.077 / 1.076 | 1.063 / 1.000 |
| strchr_16_miss | 1.077 / 1.077 | 0.750 / 0.714 |
| strchr_64_hit / _miss | 1.029 / 1.029-1.030 | 1.036 / 1.000 |
| strchrnul_16_hit | 1.042 / 1.041 | 1.069 / 1.000 |
| strchrnul_16_miss | 1.042 / 1.042 | 0.736 / 0.698 |
| strchrnul_64_hit / _miss | 1.000 / 1.000 | 1.038 / 1.000 |
| strncmp_eq16 / diff8 / names64 | 1.167 / 1.133 / 1.077 unchanged | 1.015 / 1.436 / 1.072 unchanged |
SSE2-class cycles ratios on the Skylake-SP host (loaded, not a verdict): strchr 0.99-1.02, strchrnul 1.00-1.02 (before 1.06-1.07).
Ivy Bridge time ratios: NOT MEASURED (see State).

## Jobs (mei unless named)
gate timing-ivb-strscan-dispatch-gate rc 0 (clippy, static, affected --base origin/next); plants
timing-ivb-strscan-dispatch-plants2 rc 0 (7/7 CAUGHT, 0 WARN); counts -before2 / -after rc 0; layout and stage2 rc 0.
Superseded: -plants (rc 0, 2 WARN-NO-MSG: regexes fixed), -before (stopped: full timing rounds, replaced by counters-only).

## Open / next step
1. Read the nico timing job; target time ratio <= 1.05 on the 8 strchr/strchrnul rows. If strchr is still slow, profile
   (perf stat idq_uops_not_delivered) the new layout: the SSE2 head's four 16-byte blocks each span windows.
2. strncmp_eq16 (instructions 1.015, front end ~11% of its gap per TIMING-REOPEN-1105): the remaining gap is load-use
   latency of the D-4.21 re-read of the deciding bytes after the vector compare (.Lsnc_hit -> .Lsnc_byte); a follow-up
   could decide from the loaded vector (as strchr does) - not attempted here. strncmp_diff8 (1.436 instr, SSE2) is the
   larger strncmp gap.
3. N100 (AVX2) timing for no regression not run: AVX2 instruction counts are unchanged on every row and the AVX2 fast
   path's code is byte-identical up to its first taken branch; only page-end AVX2 calls changed.
