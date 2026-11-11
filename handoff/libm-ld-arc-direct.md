# handoff: libm-ld-arc-direct (LIBM-LD-ARC-DIRECT, follows libm-ld-arc-rethink/report.md)

**State.** Branch `task/libm-ld-arc-direct`, role kanon:~/w/kn1. Step 1 of the brief (the asinl/acosl stall) done,
committed, gated; steps 2-4 (direct asin table kernel B, wiring, proof, D-10.93) NOT started (budget). Decisions in
force: D-10.19, D-10.52, D-10.75 (unchanged; step 1 changes no value and no bound). D-10.93 still reserved, unused.

**Step 1: the cause, found and removed.** The rethink named `liella_machine_mxcsr_get` (`stmxcsr`) on the path with the
`sqrtsd` seed, call site NOT VERIFIED. Objdump (Broadwell build): the MXCSR read is the ordinary one in
`mathret::elem`'s entry, the same instruction atan2l runs. What differs is the root (`arc::root_seed`, `arc::sqrt`):
`(r * TWO37) as u64` (the seed s0) and `vd as u64` (its reciprocal 2^90/r) both take values in [2^63, 2^64] on every
call. Rust's `as u64` there first tries a signed `cvttsd2si`, which overflows and raises **invalid** in MXCSR in
hardware. The envelope's exit (`fpu::settle`, hw = HW_ANY) then writes MXCSR without that flag. So every asinl/acosl
call flips a sticky MXCSR flag 0 -> 1 -> 0, and the next call's `stmxcsr` runs a microcode flow (atan2l's conversions
stay below 2^63, so its flags are already sticky). Fix: `arc::big_u64` reads the same integer from the double's bits
(exact on [2^52, 2^64], saturating at 2^64 like `as`). Values, proof and flags seen by the caller are unchanged.
`root_one_minus_sq` is now `#[inline(always)]` (LLVM outlined it after the change: +32 instructions a call).
Not done: the brief's suggested "fold the flags" variant. Flags were never the problem, so its plant
("a lost FE_INEXACT") does not apply.

**Measured (per call, user-mode perf, 400000 calls, 64 operands on [-1, 1], throughput loop).**
Broadwell-EP build host (kanon, jobs libm-ld-arc-direct-ms0 base / -ms2 step 1):

| row | cycles base -> step 1 | idq.ms_uops | instructions |
|---|---|---|---|
| asinl | 637 -> 439 | 77.2 -> 6.2 | 736.0 -> 740.5 |
| acosl | 654 -> 459 | 77.3 -> 6.3 | 763.4 -> 768.6 |
| atan2l (asin's operands) | 441 -> 444 | 9.5 -> 9.5 | 840.2 -> 840.2 |

(The step-1 commit message says 77.4 for asinl's base microcode uops; 77.2 is the recomputed value.)
| glibc asinl / acosl / atan2l | 86 / 204 / 168 | | |

Ivy Bridge (kotori, E3-1265L v2, job kotori:libm-ld-arc-direct-ivb1, run_probe.sh on base then step-1 binaries,
load 0.08-0.59; booking 174 was still settling, host idle, `~/bin/timing` started it at once). Throughput cycles:

| row | glibc | base | step 1 | ratio now | ms_uops base -> step 1 | thr / fenced TSC, same run |
|---|---|---|---|---|---|---|
| asinl | 104 | 641 | 497 | 4.78x | 67.4 -> 6.2 | 1.05 -> 0.75 |
| acosl | 213 | 668 | 511 | 2.40x | 64.7 -> 6.1 | 1.05 -> 0.73 |
| atan2l (asin's operands) | 174 | 489 | 486 | 2.79x | 9.1 -> 9.2 | 0.84 -> 0.75 |

The brief's step-1 checks pass: ms_uops ~67 -> 6.2 (<= 10), and successive calls overlap again (0.75 < 0.85).
acosl reaches the 2.5x target (511 <= 528). asinl does not (497 against 258): step 2 is still needed. The fenced
TSC ticks moved ~12% between the two runs for the same atan2l code, so only ratios within one run are compared.
Counters (kanon, `bench --compare --counters cbench_libm`, job libm-ld-arc-direct-counters, step-1 head):
asinl 661.1 instructions / 4.90x glibc's ns, acosl 692.1 / 2.15x, atan2l 739.7 / 2.54x.

**Tests.** `big_u64_is_the_saturating_conversion` (every binade of the domain, 2^64). The existing
`quotient_and_root_meet_their_bounds` (its ends include the saturated root) and `arc_tier_agrees_with_the_wide_route`
cover the root. Plants: handoff/LIBM-LD-ARC-DIRECT-plants.txt (3 functional plants plus 1 marked equivalent: the
revert to `as u64` changes no value, only the hidden hardware flag. The counts above catch it, not a test).

**Gate.** kanon libm-ld-arc-direct-gate (affected --run --base origin/next on 718ba725f): 188 passed, 0 failed,
32 ignored; peak 1.2G. Lint: libm-ld-arc-direct-lint (clippy + static) rc 0. `liella::hot_text_products` on the
final tree (job libm-ld-arc-direct-hottext): 14 passed.

**Do not redo.** Locating the stall (above). The IVB attribution in libm-ld-arc-rethink/results/.

**Next step.** Continuation for steps 2-4: prototype kernel B (brief step 2, go/no-go <= 100 cycles, table <= 8 KiB)
on Ivy Bridge. Brief targets: asinl <= 258, acosl <= 528 IVB cycles. Expected after step 1 on IVB: around the
rethink's design A, ~520-540 asinl. The fixed boundary cost (story C) stays. The probe bundle is
kotori:~/work/LIBM-LD-ARC-DIRECT/ivb1 (base/ and step1/ static binaries, run_probe.sh, summ.py).
Plan rows to read: D-10.75, D-10.52; handoff/libm-ld-arc-rethink/report.md section 3.
