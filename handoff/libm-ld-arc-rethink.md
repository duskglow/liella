# handoff: libm-ld-arc-rethink (LIBM-LD-ARC-RETHINK, -2; follows libm-ld-arc-speed.md) — COMPLETE (planning)

**State.** Branch `task/libm-ld-arc-rethink-2` (run 1's two WIP commits + run 2), role shiki:~/w/sh1. Planning
story: harness, measurements and report only, no product code. Full report: `handoff/libm-ld-arc-rethink/report.md`;
implementation brief: `handoff/libm-ld-arc-rethink/LIBM-LD-ARC-DIRECT.md`; data: `.../results/`. Decisions in force:
D-10.19, D-10.52, D-10.56, D-10.73, D-10.75 (unchanged). Reserved D-10.88 unused (no rule changes).

**Result.** Ivy Bridge cycles per call (reference / Liella): asinl 103 / 644 (6.25x), acosl 211 / 669 (3.17x),
atan2l 176 / 489 (2.78x). asinl's 2.5x target (258) is half of Liella's own atan2l, so it needs a route without the
atan2 kernel. Named cause of the asinl/acosl stall: the MXCSR read `liella_machine_mxcsr_get` (`stmxcsr`) on the
path that takes the boundary's `sqrtsd` root seed: 87% of the path's microcode uops (~58/call), 36% of its idle
cycles, and successive calls stop overlapping (throughput 644 ~= fenced latency; atan2l overlaps ~150). Second
part: the serial root and `arc::finish` chain. Recommended: design B (direct asin table kernel, Taylor in
d = |a| - j/128, fixed-point low terms + `double` tail, half-angle above 1/2, no division), step 1 the stall removal.

**Harness.** `arcprobe.c`, `run_probe.sh` (perf matrix + fenced TSC + latency chains), `run_ms.sh` (perf record
attribution), `summ.py` (log summary; never read raw logs), `build_probe.sh` + `instrument.py` (phase marks applied
and restored inside the job; falls back to math-crate marks when the boundary marks break the product build).

**INFORMATIONAL (tooling).** `xtask/src/coldbss.rs::state_sections` misses a static that LLVM both splits and
promotes across codegen units: with the boundary phase marks the section was
`.data.<SYSINFO_EHDR>.0.llvm.4629512534329216030`; the numbered-split suffix is stripped only when no `.llvm.<n>`
follows, so the check counts 0 sections and `cargo xtask so` fails. Any boundary change that re-partitions codegen
units can trip it. Fix: strip `.llvm.<digits>` before the numeric split suffix (unit test with this name).

**Do not redo.** The IVB matrix and attribution (results/), reading run 1's three handoffs, the host/PMU survey.

**Next step.** Submit LIBM-LD-ARC-DIRECT (step 1 stall removal, step 2 kernel prototype with go/no-go); a tooling
story for the coldbss suffix; later story C (fixed x87 boundary cost, ~240 fenced cycles of every x87 call).
