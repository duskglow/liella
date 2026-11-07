# handoff: libm-rethink (LIBM-RETHINK, LIBM-RETHINK-2; RETHINK story, planning)

**State.** Branch `task/libm-rethink-2` (continues `task/libm-rethink` at 71a958d7; base `next` dd2c5b88). HEAD = the
commit that adds `handoff/reports/LIBM-RETHINK-2.json` (content commit named there). **The plan is complete**: steps
1-6 of the LIBM-RETHINK brief. Nothing is wired into the product; the branch adds `handoff/` files and three `D-NEW`
rows at the end of `plan/07-decisions/s10-mathematics.md`.

**Read.** `handoff/libm-rethink/DESIGN.md` (§4.1 timing, §5 comparison with the current code, §6 recommendation),
then the four code briefs in `handoff/libm-rethink/`: `LIBM-F64-EXP-QUICK.md` (first), `LIBM-F64-EXP-QUICK-WIRE.md`,
`LIBM-CPLX-EXP-TIER.md`, `LIBM-LD-TRIG-INT.md`. Prototype and harnesses: `handoff/libm-rethink/proto/`.

**Decisions.** In force: D-10.3, D-10.13, D-10.14, D-10.17, D-10.19, D-10.25, D-10.50. Proposed (three `D-NEW` rows,
three ids to reserve; the briefs cite them by title): two-phase `double` functions (§10.2); integer fixed-point quick
tier for x87 and `_Float128` (§10.3); complex from the real quick phases (§10.5).

**Measured in LIBM-RETHINK-2.**
* Timing, ns per call, tput / lat (kotori Ivy Bridge, nozomi N100, both idle): glibc exp 13.79 / 24.35 and 7.98 /
  23.40; Liella exp today 40.79 / 53.67 (2.96x) and 27.04 / 45.53 (3.39x); prototype latency form LE (SSE2) 19.19 /
  33.58 on Ivy Bridge, E (FMA) 16.03 / 31.29 on the N100. Liella log 61.6 / 73.4 and 50.1 / 68.6 (glibc 11.66 / 26.8
  and 9.25 / 27.0).
* SSE2 builds (`glibc.cpu.hwcaps=-FMA`, both libraries, instructions per call): exp 189.0 / 80.0, log 225.6 / 81.0,
  pow 494.4 / 193.0, sin 232.0 / 126.9, atan 216.1 / 111.1. Ivy Bridge runs these, the Zen 1 counts did not.
* Current code, per call: the f64 fast tier is an accurate phase (2^-70) on every call; long double outside exp/log,
  every `_Float128` call and every complex call off the axes run the 384-bit route; `float` complex = `double` complex
  cost; an upward exp spends 128 of 363 instructions (35%) in the envelope (MXCSR switching, software flags).

**Open questions.** (1) What serialises Liella's `log` (consecutive calls barely overlap; glibc's do): find before
the log quick phase. (2) N100 throughput of the CR exp quick phase is 2.0x: the kernel brief cuts FP operations;
raise a ulp-bound question only if the wired tier stays above 2x. (3) Directed modes in place (a test in the caller's
mode needs its own proof): a later brief. (4) `sqrtl` 3.58x in Ivy Bridge time at equal instructions: not reached.

**Do not redo.** Steps 1-6; the step-2 counts and traces (keke `~/logs/libm-rethink-{percall,trace}.log`); the
prototype accuracy run (`libm-rethink-proto-accuracy`); this run's timing (`kotori:libm-rethink-2-time-ivb`,
`nozomi:libm-rethink-2-time-n100`) and SSE2 count (`shiki:libm-rethink-2-percall-sse2`).

**Next step.** Coordinator: reserve three s10 ids for the `D-NEW` rows, then cut `LIBM-F64-EXP-QUICK` (kernel) and
`LIBM-F64-EXP-QUICK-WIRE`; `LIBM-CPLX-EXP-TIER` and `LIBM-LD-TRIG-INT` can run in parallel with them.

**Process note.** The N100 timing job was started at 03:50:24 while booking 105 still showed "settling" (active from
03:54:23). The host ran nothing else (load 0.00 at the start), so the numbers stand, but the step order was wrong.
Both bookings (104, 105) are cancelled.
