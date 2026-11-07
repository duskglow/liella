# LIBM-CPLX-EXP-TIER handoff: cexp/cexpf quick tier in front of the 384-bit route

State: branch task/libm-cplx-exp-tier-2 (stacks on task/libm-cplx-exp-tier; cut from origin/next bb7f6ae1c), HEAD in
handoff/reports/LIBM-CPLX-EXP-TIER-2.json. Role mei:~/w/s2. Decisions in force: D-10.53 (design), D-10.55 (this
work's local details; renumbered from D-10.54 on the coordinator's instruction), D-10.13, D-10.25.

## Done (runs 1 and 2)

* Run 1: `crates/math/src/complex_fast.rs` (double tier: fast64 exp kernel x trig kernels, Dd product, proved part
  bound, `near` then `settle`, exponent-add scaling), `fast32/cexp.rs` (float tier), boundary `mret::cplx_fast`,
  tests, plants. Its jobs all ended rc 0 on mei: gate (11276 passed, conformance 125), plants 4/4 CAUGHT, 10^7 sweep
  (double answers per regime [1999414, 1699251, 209374, 396337, 258297] of 2·10^6), counts.
* Run 2 (profile first): one traced call (validation/wl_trace.sh, case cexp of the new probe
  validation/percall/cplx_exp.c) = 619 instructions, 26 in the export, the rest one inlined body built around nine
  Dekker products. Fix: the parts and the in-place entry come from one macro in two builds (baseline, FMA via
  `fast64::fused` and `trig::kernel_fma`), chosen at startup by `crate::hot::cexp_nearest` (boundary hot/trig.rs);
  bit-identical, tested in `math_fma_paths::cexp_fma_build_matches_baseline_bit_for_bit` (also vs the route).
* Bench gap explained: cbench_libm's 256 operands contain one genuine hard case, (-1.609657171678382,
  -1.9221928333335072), where a part lies within twice its bound (~2^-66 relative) of a rounding boundary. The
  route (~115k instructions) then runs, adding ~450 per call averaged over the set. New unit test
  `cexp_quick_tier_declines_the_bench_box_only_when_forced` checks every decline in that set is forced (384-bit
  distance to the midpoint <= 2·eps).

## Counts (instructions per call, wl_percall, Zen 1 host; glibc dynamic)

| case | glibc | run 1 | run 2 (final) | target |
|---|---|---|---|---|
| cexp (32 typical operands) | 250.8 | 636.1 | 397.0 | <= 300 |
| cexp_bench (cbench_libm's 256) | 249.8 | 1085.1 | 846.1 | <= 300 |
| cexpf | 172.6 | 182.0 | 182.0 | <= 120 |
| cexpf_bench | 170.5 | 179.0 | 179.0 | <= 120 |

Run 1's cbench_libm --counters agreed (cexp 1083.2 vs 248.2; cexpf 175.5 vs 166.9).

## Jobs (mei, chain ~/work/libm-cplx-exp-tier-2/chain.sh, sequential, final head)

libm-cplx-exp-tier-2-percall (rc 0, numbers above), then -gate (direct tier, --base origin/next), -plants
(handoff/LIBM-CPLX-EXP-TIER-2-plants.txt, 6 plants incl. the FMA dispatch and a spurious-decline plant; dir
/mnt/zram/claude/libm-cplx-exp-tier-2/plants-1), -counts (cbench_libm --compare --counters). Superseded:
check1-check6 (lint/test iterations), prof1-prof3 (profiling). check7 = lint + static + tests green on the final code.

## Next steps (in order)

1. Read gate/plants/counts (`~/bin/jobwait libm-cplx-exp-tier-2-gate libm-cplx-exp-tier-2-plants libm-cplx-exp-tier-2-counts`).
2. Timing (coordinator: Ivy Bridge and N100, booked before 19:30 UTC 11-07, hosts drain at 20:00): not booked in run 2
   (call budget). FMA changes the instruction mix, so it needs timing: kotori or nico (less busy) and nozomi.
3. cexp to <= 300: the remaining ~150 over glibc are in the double-double assembly (two trig assemblies, two Dd
   products, two eps evaluations). Next profile with `wl_trace --list` (its list output came out nearly empty in
   run 2; check the tool) before designing. A cheaper second tier for hard cases (only the undecided part, at ~2^-100,
   instead of the 384-bit route) would close most of the bench-set gap.
4. cexpf to <= 120: not touched yet; profile its 182 (export 29 + body) first.
5. Follow-ups from run 1: `|y| < 2^-30` and `|x| < 2^-60` in the tier; csqrt/clog per D-10.53.

Do not redo: the bound derivation, the visibility changes, the boundary form, the FMA macro.
