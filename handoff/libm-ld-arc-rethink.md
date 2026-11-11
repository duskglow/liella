# handoff: libm-ld-arc-rethink (LIBM-LD-ARC-RETHINK; follows libm-ld-arc-speed.md) — INCOMPLETE (context checkpoint)

**State.** Branch `task/libm-ld-arc-rethink` (from origin/next aad3df74d), role keke:~/w/n2. Harness only (no product
code): `handoff/libm-ld-arc-rethink/` = `arcprobe.c` (per-call perf counts in thr/lat modes, fenced per-call and
per-phase TSC, primitive latency chains), `instrument.py` (scratch phase markers lfence;rdtsc;lfence in `mathret::elem`
and `arc::{asin,acos,atan2}_approx`, stamps at a page arcprobe maps; applied and restored inside the job, never
committed), `build_probe.sh` (job: products, three binaries glibc/liella/inst into ~/work/LIBM-LD-ARC-RETHINK/bin,
fenced smoke run of asinl and atan2l_as), `run_probe.sh` (the matrix on a timing host; event groups per PMU, unknown
events dropped and printed). Job started: keke:libm-ld-arc-rethink-build. Decisions in force: D-10.19, D-10.52, D-10.56,
D-10.73, D-10.75. Reserved D-10.88 unused.

**Read so far.** libm-ld-arc-cycles/-chain/-speed handoffs, D-10.75. Facts used: Skylake-SP `--counters` asinl 662
cycles / 683 instructions, acosl 675 / 710, atan2l 424 / 739; IVB time ratios asinl 6.5x, acosl 3.1x, atan2l 2.74x.
Hosts: perf on every host, perf_event_paranoid=1, real PMUs (kotori/nico ivybridge, mei skylake, nozomi gracemont).

**Preliminary arithmetic (NOT VERIFIED, to be checked against the IVB run).** If glibc asinl ≈ 662/6.5 ≈ 100 cycles,
2.5x is ≈ 250 cycles: below atan2l's own 424. So removing the unexplained ~200 cycles would bring acosl under 2.5x
but NOT asinl: asinl needs a route that does not contain the full atan2 kernel. Candidates to cost from the run:
(A) current root + atan kernel with the stall removed; (B) direct asin: Taylor table `asin(c_j + d)` (no root, no
division) for `|a| <= 1/2`, half-angle `asin a = π/2 − 2·asin √((1−a)/2)` (exact `(1−a)/2`, existing integer root)
above, acos from the same kernel; (C) B with the high-order terms in `double` (terms needing < 53 bits) beside the
integer low-order terms. Each needs an error budget under the 2^-92 the rounding test assumes (D-10.75).

**Exact next step.** (1) `ssh keke '~/bin/jobwait libm-ld-arc-rethink-build'`; on red read `tail -30
~/logs/libm-ld-arc-rethink-build.log` (instrument.py prints any unmatched pattern; the trap restores the tree).
(2) `ops timing-book kotori 30 --story LIBM-LD-ARC-RETHINK --reason timing` (or nico, the idler); copy
`~/work/LIBM-LD-ARC-RETHINK/{bin,run_probe.sh}` keke -> kotori same path; `setsid nohup ~/bin/timing
libm-ld-arc-rethink-ivb bash ~/work/LIBM-LD-ARC-RETHINK/run_probe.sh ~/work/LIBM-LD-ARC-RETHINK >/dev/null 2>&1 &`.
(3) Copy the log to scratch and summarise with a script (per call = (count(n) − count(ident, 0)) / n per bin and
event group); never read the raw log. (4) Name the phase/class, cost A/B/C, recommend, write the brief as
`handoff/libm-ld-arc-rethink/<BRIEF-ID>.md`, D-10.88 only if a rule changes.

**Do not redo.** Reading the three handoffs (summarised above); host/PMU survey.
