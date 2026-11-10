# LIBM-SQRT-ENTRY: sqrt/sqrtf/sqrtl entry cost (D-10.71)

State: branch task/libm-sqrt-entry, code head f62017246 (cut from origin/next 2bc03ede6); role kanon:~/w/kn1
(synced at fd0a631e0; f62017246 only adds a local asm label and is NOT synced: sync refused BUSY while gate2 ran).
Scratch: kanon ~/work/LIBM-SQRT-ENTRY/ (plants.txt), local /home/claude/w/.scratch/LIBM-SQRT-ENTRY/.

## Done (D-10.71, plan/07-decisions/s10-mathematics.md)
- sqrt/sqrtf (mathret.rs `Sqrt for f64/f32`): inline MXCSR read (`fpregs::mxcsr`, D-X.44), same mode test
  (`fpu::is_sqrt_in_place`), operand pinned behind it, `hot::sqrtsd`/new `hot::sqrtss` inline (hot-kernel intrinsics,
  upper lanes +0), EDOM in a cold `sqrt_domain()`. The out-of-line `liella_machine_sqrtss` and its declarations are
  removed (no caller); `liella_machine_sqrtsd` stays (hypot uses it).
- sqrtl/sqrtf64x: new csys/src/machine/sqrtl.S: fnstcw + default-but-RC test, fldt 8(%rsp), fxam, fsqrt only for a
  positive normal; all else `jmp liella_slow_sqrtl` (the generated forwarder, now hidden; math-gen `ASM_ENTRY`,
  `MATH_SLOW`), so D-10.20/D-10.22 rules are unchanged. Cause (reasoned, not yet measured): the old path rebuilt the
  80-bit operand from two integer stores before each fldt (C forwarder LD(), Rust before liella_machine_fsqrt) and loaded
  8 bytes spanning the caller's 10-byte fstpt: store-forwarding failures at equal instruction counts.
- benchrows.rs forwarder scan also reads `MATH_SLOW(`.
- Brief item 1: cbench_libm is built with `-fno-builtin` (xtask/src/bench.rs), so the sqrt rows make a real library call
  for both libraries; no pointer change needed. The gcc-inline artifact was the separate percall/trace probes only.

## Results
- kanon:libm-sqrt-entry-gate RED (clippy: liella_machine_sqrtss unused) -> fixed in fd0a631e0.
- kanon:libm-sqrt-entry-gate2 (fd0a631e0): clippy ok, static ok, `s10_math_symbols sqrt` 8 passed / 5 ignored;
  `affected --base origin/next --run` STILL RUNNING at hand-back (log ~/logs/libm-sqrt-entry-gate2.log).
- Plants: plants (1) killed (broken tree), plants2 REFUSED (.S plant needs liella_machine_ label), plants3 REFUSED
  (errno-dropped and mode-test-skipped `lit:` match 2 sites: f64 and f32). handoff/LIBM-SQRT-ENTRY-plants.txt has
  the label anchors; the two `lit:` plants still need anchoring (e.g. perl:s/(impl Sqrt for f64.*?)sqrt_domain\(\);/$1let _ = 0;/s).
- kanon:libm-sqrt-entry-counts2 (Zen counts, default + `--tunables glibc.cpu.hwcaps=-FMA`) and kanon:libm-sqrt-entry-exh
  (mathcmp sqrt f --exhaustive, modes N U D Z) started on fd0a631e0; results NOT READ. Before (base): sqrt 33.1
  instr/call (LIBM-ENVELOPE-ASM), sqrtl 13 vs glibc 13 (COVERAGE group 13).
- Timing on Ivy Bridge / N100: NOT DONE (no booking taken; budget).
- NOT VERIFIED: the 5 ignored tests (products-level fam_sqrt etc.) under the new sqrtl.S; fxam class of pseudo-denormals
  (routed slow either way: only C2-only positive goes fast).

## Next step
1. Sync f62017246 when kn1 is idle; anchor the two lit: plants; run `cargo xtask plants` (all 5 must be CAUGHT).
2. Read gate2, counts2, exh; run gate on f62017246 incl. `-- --include-ignored` after `cargo xtask so`.
3. Book IVB + N100 timing for sqrt, sqrtf, sqrtl (targets 1.1x / 1.1x / 1.3x).
