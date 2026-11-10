# LIBM-SQRT-ENTRY: sqrt/sqrtf/sqrtl entry cost (D-10.71)

State: branch task/libm-sqrt-entry-3 (continues -2 and -1), merged with origin/next (8a144bd61). Role mei:~/w/s2
(Skylake-SP). Scratch: local ~/w/.scratch/LIBM-SQRT-ENTRY-3/, mei ~/work/LIBM-SQRT-ENTRY-3/ (plants.txt).
Product code is unchanged since -2's 0d64c7f00; -3 added a test, fixed the plants and regenerated the code map.

## Decisions in force
- D-10.71 (records/decisions/D-10/D-10.71.toml; the s10 view belongs to the train): sqrt/sqrtf inline MXCSR read, the
  mode test, inline root, cold EDOM; sqrtl/sqrtf64x in csys/src/machine/sqrtl.S (fnstcw test, fldt, fxam, fsqrt for a
  positive normal only, everything else `jmp liella_slow_sqrtl`).

## Done in -3
- Why -2's plants3 was red (keke, rc 1: 4 CAUGHT, 3 SURVIVED):
  - sqrtl-mode-skipped and sqrtl-negative-in-place survived because no test called the exported `sqrtl`: the Rust tests
    reach the root through `liella_sqrtl_bits` (the software path and `liella_machine_fsqrt`), and `fam_sqrt` checks
    only the exports. Fix: conformance/c/s10_sqrtl_entry.c (static link, called through a volatile pointer so the
    compiler cannot expand it to an inline `fsqrt`) checks the in-place root at nearest and upward, the EDOM, invalid
    and NaN result for -4, -0, and that 53-bit precision control still gives the 64-bit root. It runs from the new
    test `math_sqrtl_entry_tests_its_word_and_class_in_c` (s10_math_symbols).
  - inexact-lost saved MXCSR, ran the root and restored MXCSR. The root is an intrinsic that LLVM treats as having no
    side effects, so it can move it after the restore call, and the plant could not model a lost flag. This is an
    inference: the mutant's disassembly was not read. The plant now replaces the in-place root with the software root's
    value and drops its status, so the hardware root's flags are lost.
- The code map was regenerated after the merge (`cargo xtask codemap`). It lists the `.S` macro labels `SQRTL` and
  `SQRTF64X` as symbols, because it reads the labels as written; the exported names are sqrtl/sqrtf64x.
- Plants file: handoff/LIBM-SQRT-ENTRY-plants.txt (copy on mei: ~/work/LIBM-SQRT-ENTRY-3/plants.txt).

## Jobs (mei)
- libm-sqrt-entry-3-lint2 (clippy + static on c214cf606): rc 0. libm-sqrt-entry-3-check (so + run.sh s10_sqrtl_entry +
  the sqrt tests): rc 0, 10 passed.
- libm-sqrt-entry-3-gate (affected --run --base origin/next) and libm-sqrt-entry-3-plants (7 plants, dir
  /mnt/zram/claude/LIBM-SQRT-ENTRY-3/plants-1), both on tree c214cf606 (code identical to the final head, which adds
  only handoff files): gate rc 0 (32 suites ok, 11428 passed, 0 failed, 1257 ignored; conformance 127 passed; peak
  16G, oom_kill 0); plants rc 0, 7 CAUGHT, 0 SURVIVED, no WARN (results in the plants file).
- Superseded: libm-sqrt-entry-3-lint (rc 128: its `git status` step ran in a role directory that is not a git
  checkout; the codemap it wrote was copied back and committed).

## Counts (from -2, Zen 1, 0d64c7f00, cbench_libm, which is built with -fno-builtin, so both libraries make real calls)
sqrt 6.11 / 12.77 ns = 2.09x (33.1 vs 17.1 instr/op); sqrtf 2.08x (31.2 vs 17.1); sqrtl 0.95x (24.2 vs 30.2).
Target 1.1x / 1.1x / 1.3x: sqrtl reached, sqrt/sqrtf NOT reached. No timing on Ivy Bridge or N100 yet.

## Profile of `sqrt`'s in-place path (objdump of target/liella/libm.so.6 on mei, -3): 26 instructions
push rbp; push rbx; sub rsp (a frame for the slow path's call; not shrink-wrapped); `movl $0,(%rsp)` plus `mov %rsp,%rax`
(the zeroed `stmxcsr` slot); stmxcsr; 2 movs; and; cmp; jne; movapd; ucomisd x,x; setp; xorpd; ucomisd 0,x; mov; jb
(LLVM turned `r.is_nan() && !x.is_nan()` into an x<0 and NaN test before the root); xorps; sqrtsd; test; je; add; pop;
pop; ret.

## Next step (in order)
1. A cheaper sqrt/sqrtf in-place path (the design to try; fit it to D-10.71 as an amending record): (a) move the
   software branch (else arm) into a `#[cold] #[inline(never)]` function so the hot function needs no frame (6
   instructions); (b) `stmxcsr` into a slot that is not zeroed first (MaybeUninit or the red zone; 2); (c) root first,
   then one `ucomisd r,r; jp cold`, with the cold path testing x for NaN to decide EDOM (about 5). Expected hot path:
   about 10 instructions. Keep the `pin` barrier (the unmasked-trap test and the mode-test-skipped plants guard it).
   Gate, plants (this file plus a plant on the new cold split), and counts before/after on one host.
2. Timing on Ivy Bridge (nico or kotori, the less busy) and N100 for sqrt, sqrtf, sqrtl (the instruction mix changes:
   the row verdict needs timing). Not done in -1, -2 or -3: hand it to the timing sweep if a story's budget is short.
3. INFORMATIONAL: `hot::sqrtsd` in the family kernels (invtrig, hyperbolic) may merge into another register where the
   operand stays live; check their disassembly.
