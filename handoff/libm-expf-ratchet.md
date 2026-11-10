# LIBM-EXPF-RATCHET: handoff (expf/exp2f 11-07 time regression, fixed forward)

State: branch `task/libm-expf-ratchet` (cut from origin/next e6f60a3ff), code commit 9e6a674c6, docs/plants/report on
top. Role keke:~/w/n1. Decision in force: D-10.67 (amends D-10.63). Plan rows to read: D-10.63, D-10.67.

## Cause (isolated by code and disassembly, not by bisecting builds)
* Culprit: `task/libm-f32-onepass` (3efdd33c2, D-10.63). Its expf/exp2f hot path (libm.so.6 at origin/next) starts
  `movd %xmm0,%eax; ...; cvtss2sd %xmm0,%xmm1` with no write to xmm1 before it. `cvtss2sd` writes only the low lane,
  so it waits for xmm1's previous writer; the function's last write to xmm1 is `mulsd %xmm4,%xmm1` (y), so call n+1's
  conversion waits for call n's whole kernel: back-to-back calls form one latency chain. Fewer instructions (73 -> 47
  per bench call) but slower time, exactly the sweep's pattern. The LIBM-F32-ONEPASS handoff had named this as the
  likely cause (its next step 0); this story confirmed it in the disassembly and timed it.
* The other two candidates do not touch the expf code: `task/libm-f64-exp-quick-wire-2` changed only the double exp
  path (crates/boundary/src/hot/exp_log.rs, fast64/quick.rs; expf's code above calls nothing), and
  `task/machine-jcc-pad` is an xtask/build-time padding check (jcc_pad.rs, xtask/src/jcc.rs); expf and exp2f start
  256-byte aligned in both builds and the timing below shows the code change alone restores the rows.
* Isolation used the brief's candidate list and `git diff --stat` of those three merges (file lists only).

## Fix (D-10.67)
* `fast32::onepass_scaled(x)` builds x·2^-896 from x's bits (sign to bit 63, the 8-bit exponent as the low byte of
  the 11-bit field, the fraction on top) and the kernel multiplies by c·2^896: both scalings exact, z = x·c normal,
  so every rounding direction gives the same z and D-10.63's proof holds unchanged. The `double` arrives by a
  full-register `movq` from a GPR: no partial write. expf/exp2f hot path 40 -> 48 instructions (no cvtss2sd left).
* Not a placement issue, so no placement contract: the plant `cvtss2sd-restored` (the exact revert) is bit-identical
  by design (EQUIVALENT); the timing proof below is what catches it (the base sweep is that code).

## Results
* Counts (keke:libm-expf-ratchet-counts, cbench_libm --compare --counters, instr/op Liella vs glibc): head expf
  55.1 / exp2f 55.1 / expf_upward 55.1 vs base 47.1 (glibc 39.1 / 37.1): +8 per call, the disassembly's +8. exp10f
  64.2, logf 72.2, expf_qnan 28.1 (base 27.1) otherwise unchanged. Instructions go up; time is the arbiter here (a
  dependency chain, not an instruction cost), so instructions alone would mislead for this change.
* Timing (booked, idle hosts, staged trees: head = 9e6a674c6 code (libm.tar sha 84f466da...), base = origin/next
  e6f60a3ff (libm.base.tar sha 6e367f32...); `xtask-bin sweep run --only cbench_libm`, base, head, base, head;
  script ~/work/LIBM-EXPF-RATCHET/run.sh and rowcheck.sh on each timing host):
  - N100 (nozomi:libm-expf-ratchet-n100, rc 0): time ratio vs glibc, base -> head, rounds 1 / 2:
    expf 3.109 -> 1.662 / 3.107 -> 1.664; exp2f 3.132 -> 1.673 / 3.138 -> 1.650; expf_upward 2.905 -> 1.551 /
    2.901 -> 1.555; expf_qnan 0.994 -> 0.994. Spreads <= 0.018. Morning target 2.60 / 2.59: met with margin.
  - Ivy Bridge (kotori:libm-expf-ratchet-ivb, rc 0): expf 2.514 -> 1.241 / 2.516 -> 1.242; exp2f 2.754 -> 1.366 /
    2.752 -> 1.515; expf_upward 2.770 -> 1.216 / 2.477 -> 1.365. Spreads wide on this class for base and head alike
    (0.007-0.75, the brief's caveat), but the medians repeat round to round.
  - Every other cbench_libm row (166 per round): no row more than 1.5% slower in both rounds on either class; median
    change 1.000 / 1.000 (N100), 0.999 / 1.000 (IVB). Single-round >1.5% moves (N100 5 and 12, IVB 3 and 3) do not
    repeat: noise.
* Gate: keke:libm-expf-ratchet-gate2 (clippy, static, affected --base origin/next) on the final head.
* Plants: keke:libm-expf-ratchet-plants2 (4 plants; `cvtss2sd-restored` EQUIVALENT by design, timed above).
  Superseded: libm-expf-ratchet-gate (static tables-fresh: KERNELS.md not regenerated, fixed by 560c931e7),
  libm-expf-ratchet-plants (older tree without the KERNELS.md commit; stopped by me after sign-dropped CAUGHT, so
  the final head's run is the record).

## Not done / next
* A deterministic contract for this class (an xtask check over libm.so.6: no scalar SSE partial write - cvtss2sd,
  cvtsd2ss, cvtsi2s[sd], sqrts[sd], rounds[sd] - into a register not written earlier in the same function's hot path)
  would catch a regression without timing hosts; not built here (budget). Recommended as its own story.
* exp10f/logf/trig one-pass conversions (LIBM-F32-ONEPASS next steps 1-4) should start from `onepass_scaled`.
