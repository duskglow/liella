# LIBM-SQRT-ENTRY: sqrt/sqrtf/sqrtl entry cost (D-10.71)

State: branch task/libm-sqrt-entry-2 (continues task/libm-sqrt-entry), code head 0d64c7f00, cut from origin/next
2bc03ede6. Role keke:~/w/n4 (Zen 1). Scratch: keke ~/work/LIBM-SQRT-ENTRY-2/ (cx.sh, mxb.c), local ~/w/.scratch/LIBM-SQRT-ENTRY-2/.

## Decisions in force
- D-10.71 (records/decisions/D-10/D-10.71.toml; the s10 view is the train's): sqrt/sqrtf inline MXCSR read, the
  mode test, inline root, cold EDOM; sqrtl/sqrtf64x in csys/src/machine/sqrtl.S (fnstcw test, fldt, fxam, fsqrt for a
  positive normal only, all else `jmp liella_slow_sqrtl`). The handoff .txt copy is removed.

## Done in -2
- Record D-10.71 written (`records check` ok: 476 decisions, 0 pending).
- New test `math_sqrt_stays_out_of_place_under_an_unmasked_trap` (s10_math_symbols): no test called sqrt/sqrtf with
  an exception unmasked, so dropping the in-place mode test was invisible. Unmasked denormal-operand + subnormal
  operand traps in place; ZE/OE/UE masks must give the same answers.
- Profile of the after-build (Skylake-SP counts from -1: sqrt 9.29 ns vs glibc 4.17 = 2.2x at 27.1 vs 17.1 instr/op;
  sqrtf 1.44x; sqrtl 0.52x): the disassembly showed `sqrtsd %xmm0,%xmm2`. x stays live for the EDOM test, so the
  scalar root wrote another register and merged its upper lane, a dependency on that register's last writer (the
  previous call's root in a call loop): one root latency per call. Fix 0d64c7f00: the root takes its own pinned copy
  of x (`x.pin()`); the build now emits `xorps %xmm0,%xmm0; sqrtsd %xmm1,%xmm0` (zeroing idiom, no dependency).
  Same for sqrtf. Ruled out first by a hardware microbench (mxb.c, Skylake-SP): `stmxcsr` next to independent
  roots costs nothing (7.57 vs 7.63 ref-cycles per sqrtsd; 4.09 vs 4.14 sqrtss).
- #4193: cbench_libm is built with -fno-builtin (xtask/src/bench.rs), so its sqrt rows make a real call for both
  libraries; only the percall/trace probes inline. The verdict uses the cbench rows.

## Jobs on 0d64c7f00 (keke)
- libm-sqrt-entry-2-lint2: so + clippy + static rc 0.
- libm-sqrt-entry-2-gate2 (affected --base origin/next), libm-sqrt-entry-2-plants2 (7 plants, plants file below),
  libm-sqrt-entry-2-counts (cx.sh: cbench_libm sqrt,sqrtf,sqrtl default and `--tunables glibc.cpu.hwcaps=-FMA`, then
  `mathcmp sqrt f --exhaustive` modes N U D Z), libm-sqrt-entry-2-plants3 (plants file installed as
  ~/work/LIBM-SQRT-ENTRY-2/plants.txt, same as the handoff copy): all RUNNING at hand-back; read ~/logs/<label>.log.
- plants2 BASELINE FAILED (rc 2, superseded): the filter `sqrt --include-ignored` also ran the ignored fam_csqrt and
  sym___sqrt{,f,l,f128}_finite tests, which fail on the unmutated tree (cause not examined: outside this story);
  plants3 adds `--skip fam_csqrt --skip _finite`.
- Counts on Zen 1, 0d64c7f00 (time ns glibc / Liella, ratio, instr/op Liella vs glibc), default build, -FMA the same:
  sqrt 6.11 / 12.77 = 2.09x, 33.1 vs 17.1; sqrtf 5.89 / 12.26 = 2.08x, 31.2 vs 17.1; sqrtl 15.88 / 15.04 = 0.95x,
  24.2 vs 30.2. NO before on this host, so the register fix's effect on Zen 1 is unmeasured; sqrt/sqrtf stay ~2.1x:
  target (1.1x) NOT reached, and the merge dependency is not the whole surplus here. Next profile: per-instruction
  cost of the in-place path on Zen 1 (stmxcsr microbench on Zen 1 was queued, not run; prologue push/push/sub and
  the extra NaN compares are the other candidates).
- Superseded: libm-sqrt-entry-2-lint (static red on a home path here, fixed), -gate/-plants (killed: old head),
  -mxb (probe evidence, not a proof job).
- Plants file: handoff/LIBM-SQRT-ENTRY-plants.txt (each mathret.rs plant anchored to its impl).

## Open / next step
1. Read counts (Zen 1) for the after numbers; a "before" on the same host needs a base build (not done).
2. Book timing on Ivy Bridge (nico or kotori, the less busy) and N100 for sqrt, sqrtf, sqrtl (targets 1.1x, 1.1x,
   1.3x): not done in -1 or -2 (budget). The row verdict comes from that timing.
3. INFORMATIONAL: `hot::sqrtsd` in the family kernels (invtrig, hyperbolic) may have the same merge dependency where
   the operand stays live; check their disassembly.
4. NOT VERIFIED: products-level tests under `--include-ignored` other than through the plants baseline.
