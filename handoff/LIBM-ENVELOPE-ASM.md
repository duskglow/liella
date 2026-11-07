# LIBM-ENVELOPE-ASM: machine-register intrinsics in the libm envelope (D-X.44)

State: branch task/libm-envelope-asm, code head 87e08968d (cut from origin/next 70cb44d76); role mei:~/w/s2.
Scratch on mei: ~/work/LIBM-ENVELOPE-ASM/ (counts.sh, base/, plants-2.txt); RAM /mnt/zram/claude/LIBM-ENVELOPE-ASM/
{counts-1/{base,after}.out, plants-1, plants-2}.

## Done
- New file crates/boundary/src/hot/fpregs.rs, unsafe category `machine-register` (owner ruling 2026-11-07,
  #3938/#3939/#4004; decision row D-X.44): `mxcsr()` = one `stmxcsr dword ptr [{p}]` into a 4-byte local (not pure);
  `Pin::pin` for f32/f64 (pairs pin each element) = template `/* {x} */`, `inout(xmm_reg)`,
  `options(pure, nomem, nostack, preserves_flags)`. Deviation from the brief's literal `asm!("", ...)`: rustc
  refuses a template that does not use its operand ("argument never used"), so the template is an assembler comment
  naming the operand: still no instruction.
- mathret: private `peek()` (inline read + `fpu::is_default_control`) in `elem_fast` (every double and float quick-tier
  entry, elem32_fast included) and `sincos_fast`; `x.pin()` replaces `core::hint::black_box(x)` there. The cold
  paths' `black_box` on (value, Status) results and `sqrt_c`/`hypotf`'s direct ffi reads are unchanged.
- xtask/src/policy.rs unsafe audit: category registered; inline asm refused outside fpregs.rs and the tb-field site;
  any non-machine-register unsafe form in fpregs.rs refused; templates other than the MXCSR store (exactly once) and
  the barrier comment refused; barrier without `pure, nomem` refused. docs/architecture.md category table updated.
- ARCH(x86_64) tags on the three asm lines and `fn mxcsr` (arch-audit).
- New test `math_double_fast_entry_stays_out_of_place_under_an_unmasked_trap` (s10_math_symbols, s10_0_f_0): the
  fast entries in every rounding direction with DM, ZM, OM or UM unmasked, value and MXCSR after exact. Rounding and
  FTZ/DAZ/sticky-flag words were already covered by `..._in_a_non_default_mode`.

## Results (mei)
- clippy + static + `s10_math_symbols math_` (13 passed) green on the code (job libm-envelope-asm-lint5, then the
  `fn mxcsr` ARCH tag only; the gate re-runs clippy and static on 87e08968d).
- Counts (libm-envelope-asm-counts, instr/call Liella base -> after, glibc in brackets): exp 94.2 -> 88.1 (67.1),
  exp2 125.2 -> 121.2, log 182.4 -> 178.4 (63.1), log2 187.4 -> 183.4, log10 197.7 -> 193.7, logf 77.2 -> 72.2,
  sinf 90.1 -> 85.1, sin 158.4 -> 154.4, cos 156.3 -> 152.2, tan 175.9 -> 171.9; expf 47.2, exp2f 47.2 and sqrt 33.1
  unchanged (they do not use elem_fast). No row slower. -4 to -6 per call, close to the brief's ~-5.
- Timing on Ivy Bridge / N100: PENDING (timing hosts down until about midweek).
- plants-1 (head 87e08968d): extra-instruction CAUGHT (static msg), skip-mode-check CAUGHT (new mask test),
  asm-elsewhere caught only by the build (mutant lacked `#[allow(unsafe_code)]`: WARN-NO-MSG), barrier-dropped
  SURVIVED. Superseded by plants-2 (asm-elsewhere fixed; barrier-dropped marked equivalent, reason in the file).
- Gate libm-envelope-asm-gate (clippy, static, affected --base origin/next --run) and plants-2 were running at
  hand-back.

## Open / RULING NEEDED
- barrier-dropped survives: with the call gone the compiler no longer hoists the fast comparison above the mode
  branch, so no test can show the barrier working today. Recommend keeping it (it is free: no instruction, and it
  forbids a legal hoist), recorded as EQUIVALENT.

## Next step
1. Read libm-envelope-asm-gate and libm-envelope-asm-plants-2 on mei (`~/bin/jobwait <label>`).
2. Timing on nico/kotori and the N100 when the hosts are back (rows exp, log, logf, sin).
3. Optional: the same inline read in `Sqrt for f64/f32` and `Hypot for f32` (direct ffi reads today), and in
   `CSys::mxcsr` for the full envelope `elem` (counts per row first).
