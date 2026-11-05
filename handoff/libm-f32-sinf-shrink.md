# LIBM-F32-SINF-SHRINK(-2): handoff (sinf, cosf, tanf fast path)

State: branch `task/libm-f32-sinf-shrink-2` (stacks on `task/libm-f32-sinf-shrink`), code head `709feafb`; this
note and `handoff/reports/LIBM-F32-SINF-SHRINK-2.json` on top. Role mei:~/w/s2 (Skylake-SP, 48 threads).
Ruling #2906 applied: no row slower any more; nearest rows still above the 1.05x target (see "Next").

## Done
* -1 (3665b7ff, 6347789c): minimax sin deg 5 / cos deg 6 kernel (D-10.3 fits), in-place nearest entries
  `sin_nearest`/`cos_nearest`/`tan_nearest` through `mret::elem32_fast`; bounds and tests as in those commits.
* -2 de05d60e: `elem_fast` sends a non-default MXCSR straight to the new cold `elem_switched(x, saved, f)`, which
  switches with `fpu::switch_default` (new, rt) on the value already read: no second MXCSR read, no second
  default-word test, no `in_place` argument. `elem_fast_cold` is now the default-mode refusal only. Shared by every
  `elem_fast` user (expf.., the double exp/log/pow entries).
* -2 709feafb: `math_double_fast_entry_stays_out_of_place_in_a_non_default_mode` also runs every word with all six
  flags already sticky (0x3f, 0x803f). Found by plant `in-place-exit` (settle instead of leave on the directed
  route), which survived before: with no sticky flags settle always wrote `saved | flags` back.

## Counts (mei, cbench_libm, instructions per call; base = 617417a6 from `git archive` in RAM, head = de05d60e)
| row | base | -1 (keke) | head (mei) | glibc |
|---|---|---|---|---|
| sinf / cosf | 167.3 | 90.2 | 90.2 | 42.0 |
| tanf | 173.3 | 98.2 | 98.3 | 106.3 |
| sinf_upward | 246.7 | 258.6 | **221.8** | 42.0 |
| expf_upward | 242.5 | | 205.6 | 39.0 |
| exp_upward / log_upward / pow_upward | 406.9 / 388.5 / 913.8 | | 368.1 / 349.7 / 873.5 | |
169 rows: 0 slower, 9 faster (1% + 0.5 tolerance). Jobs `libm-f32-sinf-shrink-2-cbase`, `-chead`; compare
script `~/work/LIBM-F32-SINF-SHRINK-2/cmp.py` on mei. 709feafb changes only a test (libm unchanged).

## Per-phase split of one sinf call (objdump of Liella's own libm.so.6, head; 78 straight-line + helper + loop ≈ 90)
| phase | instr | note |
|---|---|---|
| entry/exit | ~18 | push/pop, GOT call to the out-of-line MXCSR read helper (~4 inside), mask/cmp/jne, `black_box` spills x twice through the stack (5, one dead `lea`) |
| domain test | 7 | cvtss2sd, andps, two `ucomisd` against TRIG_MIN/TRIG_MAX |
| reduction | ~12 | 3-part Cody–Waite, shift trick |
| small-r refusal | 5 | `abs(r) < TRIG_R_MIN` |
| polynomials | ~16 | sr deg 5 and cm deg 6 (both needed for sin: S + S·cm + C·sr) |
| table + combine | ~12 | two index computations, two loads, 4 flops |
| rounding test | 13 | three `movabs` constants, two compares |
The surplus over glibc's 42 sits in the envelope (~18) and the tests (domain 7 + small-r 5 + rounding 13 = 25).

## Next (the nearest rows toward 1.05x; in this order, each counted alone)
1. Inline the MXCSR read (`stmxcsr` in place of the GOT call; boundary asm, UNSAFE category) and one `black_box` instead
   of two: ~8 on every fast entry (expf too).
2. Domain test on the f32 bits (one integer subtract + unsigned compare): 7 -> 3.
3. Drop the small-r refusal by proof (exhaustive: min |r| over n != 0, |x| < 2^19 floats) or fold it into the
   rounding test: -5.
4. Cheaper rounding test (fewer 64-bit constants). Then recount; a first-principles bound if 1.05x stays out of reach.
No D-row added (local detail; the code comment cites ruling #2906). No saturation-register row written (budget).
