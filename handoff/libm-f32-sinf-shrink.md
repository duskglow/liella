# LIBM-F32-SINF-SHRINK(-4): handoff (sinf, cosf, tanf fast path)

State: branch `task/libm-f32-sinf-shrink-4` = `task/libm-tables-per-family` (f207fed6, Stacks-on) + merge of
`task/libm-f32-sinf-shrink-3` (d15fe0c1) as `3ad240da`; this note and `handoff/reports/LIBM-F32-SINF-SHRINK-4.json`
on top. Role mei:~/w/s2 (Skylake-SP). Ruling #2906 applied: no row slower; nearest rows still above the 1.05x
target (see "Next").

## -4 (move onto the per-family layout, no design work)
* LIBM-TABLES-PER-FAMILY had NOT landed in origin/next at the start of this run (f207fed6 not in next), so origin/next
  was not merged (the coordinator note allows that merge only after it lands). The branch is stacked on the tables
  branch instead, and -3 merged into it; when the tables branch lands, the remaining merge of next is the release
  manager's (or a -5's) job. -3's base next de5102e3 is an ancestor of the tables base f8b77d70: no extra next commits.
* Conflicts: `coeffs.rs` (per-family `mod`s + -3's `fast32_sin`/`fast32_cos`) and `fast32.rs` (took the tables side,
  then re-applied -3's own diff `de5102e3..d15fe0c1`): the kernel, its proof text and the margins/bounds
  (TRIG_DELTA 40, TAN_DELTA 80, TRIG_BOUND, TAN_BOUND, moved from the core into `fast32/trig.rs`, as inv_trig keeps
  its own) in `fast32/trig.rs`; the Sin/Cos/Tan in-place arms and `sin/cos/tan_nearest` in the core's `eval_nearest`
  (next to the other families' arms and `nearest_fn!`s); the tests (margin check, nearest-entry agreement, per-function
  exhaustive jobs) in `fast32/tests.rs`.
* Generated files: no `coeffs/` table file touched by -3. Job `libm-f32-sinf-shrink-4-check` (rc 0) ran `math-gen`,
  `tables`, `stubs` (checksums of crates/boundary/src/posix, crates/math/src/coeffs, spec identical before/after),
  `remez --check` (all fits up to date), clippy -D warnings all targets/features, `cargo xtask static`
  (`tables: up to date (5 files)`).
* Jobs on mei (see the report json for results): gate `libm-f32-sinf-shrink-4-gate` (direct tier as the brief's
  command; may escalate to FULL as -2/-3 did), plants `libm-f32-sinf-shrink-4-plants` (mei:~/work/LIBM-F32-SINF-SHRINK-4/
  plants.txt: -3's six with paths moved + `sin-poly`, `trig-margin` on fast32/trig.rs; dir
  /mnt/zram/claude/LIBM-F32-SINF-SHRINK-4/plants-1), counts `libm-f32-sinf-shrink-4-counts` (head only, after the gate,
  compared with -3's base/head tsvs by -2's cmp.py; the tables change was proven .text-identical by its own story, so
  -3's base stands as the base).
* Plants result: rc 0, 8 CAUGHT, 0 SURVIVED, 0 EQUIVALENT, 0 WARN. Gate and counts were still running at hand-back.

## -3 (merge only, no design work)
* CI could not merge -2: `crates/math/src/fast32.rs` conflicted with next's hyperbolic and inverse-trig families.
  Resolved keeping both sides: `nearest_core`'s refusal arm = next's list (with Sinh..Atanh) minus Sin/Cos/Tan (answered
  in place here); tests = -2's `trig_nearest_entries_match_the_accurate_path` and per-function trig exhaustive jobs next
  to next's hyperbolic tests. The rest of next auto-merged; `math-gen` and `stubs` regenerate nothing.
* Counts (mei, cbench_libm, base = origin/next `de5102e3`, head = `dbbe3d1b`, both `git archive` in RAM): 169 rows,
  **0 slower**, 10 faster: sinf 167.4 -> 90.1, cosf 167.4 -> 90.2, tanf 173.4 -> 98.3, sinf_upward 246.7 -> 221.7,
  expf_upward 242.6 -> 205.4, exp/log/pow_upward 407.2/388.6/914.7 -> 367.8/349.4/872.7 (same as -2's). Every
  hyperbolic / inverse-trig row (sinhf..atanhf, asinf, acosf, atanf, atan2f and their double/long/f128) unchanged.
  Jobs `libm-f32-sinf-shrink-3-cbase`, `-chead`; tsvs in mei:~/work/LIBM-F32-SINF-SHRINK-3/{base,head}.tsv.
* Plants `libm-f32-sinf-shrink-3-plants` (file mei:~/work/LIBM-F32-SINF-SHRINK-3/plants.txt): -2's four on the
  elem_fast directed route + `sin-as-cos`, `tan-inverted` on the resolved fast32 trig arms. Gate
  `libm-f32-sinf-shrink-3-gate`. Results: plants 6 CAUGHT, 0 SURVIVED, 0 WARN; gate rc 0 (affected escalated to
  FULL on the branch's generated files, as -2's did): 32 suites ok, 10842 passed / 0 failed, conformance 117 / 0,
  peak 16G, 67 min. clippy (-D warnings, all targets/features) and `cargo xtask static` clean on the merged head.

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
