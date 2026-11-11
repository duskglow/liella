# LIBM-LD-ARC-RETHINK(-2) report: why x87 asinl/acosl stall, and the design for <= 2.5x

Host: Ivy Bridge timing host (nico, Xeon E3-1220L v2, load 0.0-0.4), user-mode perf, pinned to one CPU; probe
`arcprobe.c` (this directory), summaries in `results/` (`summ.py` derives per call = (count(n) - count(0)) / n).
Run 2026-11-11: jobs nico:libm-ld-arc-rethink-2-ivb (matrix) and nico:libm-ld-arc-rethink-2-ms (perf record
attribution). Both started while booking 171 was still "settling" on an already idle host (0 jobs, load 0.00).

## 1. Measured (cycles per call, throughput loop, 64 operands uniform on [-1, 1])

| row | reference | Liella | ratio |
|---|---|---|---|
| asinl | 103 | 644 | 6.25x |
| acosl | 211 | 669 | 3.17x |
| atan2l (asin's operands) | 176 | 489 | 2.78x |
| asinl, \|a\| < 1/2 | 70 | 629 | 9.0x |
| asinl, \|a\| >= 1/2 | 140 | 667 | 4.8x |

Targets at 2.5x: asinl <= 258, acosl <= 528. The preliminary arithmetic (#4567) holds: asinl's target is about half
of Liella's own atan2l (489), so no route that contains the full atan2 kernel can reach it; acosl can.

Liella asinl against Liella atan2l on the same operands (per call): cycles 644 / 489 (+155) with instructions
736 / 842 (-106); uops issued 1132 / 1034; microcode-sequencer uops (idq.ms_uops) 67 / 9; cycles with nothing
executing 176 / 70; front-end slots not delivered 406 / 149; recovery cycles 15.8 / 3.0; divider active 61 / 47;
store-forward blocks 4.0 / 3.0; FP assists 0 / 0; AVX-SSE transitions 0 / 0; x87 ops 9.9 / 5.0.
Fenced latency (lfence;rdtsc, math-crate phase marks, TSC/1.042): asinl 687 = call-to-tier 115 + root 157 (one
marker ~38 included) + kernel 292 + tail (rounding, packing, exit, return) 127; atan2l 652 = 196 + 0 + 307 + 107.
So serially asinl costs only ~35 more than atan2l, but in the throughput loop asinl gains nothing from overlapping
successive calls (644 thr vs ~660 fenced) while atan2l overlaps ~140-160 cycles (489 thr vs ~626 fenced).
Primitive latencies (perf cycles per chained op): divsd 22, sqrtsd 25, x87 fsqrt 27, fdiv 25, fpatan 146,
fyl2x 83, 128x128 high product 22, f80 integer round trip 16, cvt round trip 11; an ldmxcsr or fldcw in a sqrt
chain adds ~0.

## 2. Named cause

**The MXCSR read on the asinl/acosl path (`liella_machine_mxcsr_get`, an `stmxcsr`)**, reached on the path that takes
the boundary's `sqrtsd` root seed. perf record (IVB): 87% of asinl's microcode-sequencer uops (~58 per call) and
36% of its idle cycles (~63 per call) are in that helper; in atan2l the helper holds ~0 MS uops and 2% of idle
cycles. Instruction class: an MXCSR access that runs as a microcode flow and does not overlap: it is what keeps
successive asinl calls from overlapping (the throughput-vs-latency gap above). Not the cause (measured zero or
equal): integer division (none on the path, objdump), FP/denormal assists, AVX-SSE transitions, store forwarding
(+1 block), x87 transitions (+5 x87 ops). Second part: the serial root (sqrtsd 25 + exact residual + Newton
correction, ~120 cycles of the fenced 157) and the `arc::finish` chain (24% of cycles, 25% of idle cycles).
NOT VERIFIED: the exact call site of the `stmxcsr` on this path (the boundary-crate phase marks could not be built,
see INFORMATIONAL in the handoff) and why it costs a microcode flow here and not in atan2l (reading: it waits for
the in-flight SSE operations whose flags it must report). Fixed boundary cost: call-to-tier + tail = ~240 fenced
cycles, 37% of asinl's latency, the same order for atan2l.

## 3. Designs (IVB cycles, from the measurements above; thr = 0.75 x fenced, atan2l's measured overlap)

A. Current structure, stall removed (no MXCSR read on the arc path: seed the root without SSE flag traffic or fold
   the flags without `stmxcsr`). asinl ~= atan2l 489 + root ~50 = 520-540 (5.1x: misses); acosl 530-550 (2.5-2.6x:
   at the line). Cheap, keeps D-10.75's proof; not enough for asinl.
B. Direct asin kernel, no division, no root for |a| <= 1/2 (Tang's table idea applied to asin: Tang, ARITH 10,
   1991; derivative recurrence from asin' = (1 - x^2)^(-1/2): (1 - c^2)(n+2)(n+1) t_{n+2} = (2n+1)(n+1) c t_{n+1}
   + n^2 t_n; Brent and Zimmermann, Modern Computer Arithmetic ch. 4). c_j = j/128 on [0, 1/2], d = |a| - c_j,
   |d| <= 2^-8; asin(c_j + d) = asin c_j + sum t_n d^n; the singularity at 1 gives a ratio ~2^-7 per term at
   c = 1/2, so ~15 terms: n <= 7 in 128/64-bit fixed point (Horner, terms shrink, words shrink), n >= 8 in `double`
   (Estrin, depth 4, off the integer chain). |a| > 1/2: asin a = pi/2 - 2 asin sqrt((1 - |a|)/2), (1 - |a|)/2 exact,
   one root (double seed without the MXCSR read, one exact integer residual: D-10.75's root, proved 2^-101.4),
   then the same kernel. acos from the same kernel (pi/2 - asin a; 2 asin sqrt((1 - a)/2); pi - that).
   Estimate: kernel 60-80, root 50-60 (|a| > 1/2 only); with today's fixed cost asinl ~= 0.75 x (115 + 80 + 125)
   = 240 (|a| < 1/2) and ~285 (|a| > 1/2), mean ~260 (2.5x: at the line); acosl ~265 (1.3x).
   Accuracy: table values to 2^-128, coefficients to 2^-110 of their term, truncation <= 2^-108 absolute for
   j >= 1 (relative 2^-100 since |a| >= 2^-8), j = 0 is the odd series at 0 (relative by construction); the
   half-angle doubles the kernel error and adds the root's 2^-101.4 times 2/sqrt(1 - s^2) <= 2.31: total below
   2^-98, under the 2^-92 the rounding test assumes (D-10.75's test and 384-bit fallback unchanged).
   Cost: table 65 x ~128 B ~= 8 KiB (two lines per call; cold misses must be measured).
B3. Same table idea through the addition formula asin a = theta_j + asin(a cos theta_j - sin theta_j sqrt(1 - a^2))
   (small table, 64 x 48 B = 3 KiB; reduced argument |r| <= 2^-8, odd series 6 terms) - needs the root on every
   call: kernel ~130 + root: asinl ~330 (3.2x). Rejected for asinl's target, kept as the fallback if B's table
   misses cost too much.
C. B plus the fixed boundary cost: the call-to-tier and tail phases (~240 fenced) are the largest remaining block
   after B and are shared by every x87 function; needed for margin under 2.5x. Their split needs the boundary
   phase marks (blocked, INFORMATIONAL).

## 4. Recommendation

B (with A's stall removal as its first, separately measured step), then C as its own story. Brief:
`LIBM-LD-ARC-DIRECT.md` here. Not done in this story: a prototype of B's kernel (budget); the brief makes it
step 1 with a go/no-go gate.
