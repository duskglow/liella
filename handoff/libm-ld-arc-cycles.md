# handoff: libm-ld-arc-cycles (LIBM-LD-ARC-CYCLES; follows libm-ld-trig-inverse-int.md)

**State.** Branch `task/libm-ld-arc-cycles` (cut from origin/next b7c8f799a), role shiki:~/w/sh1. Code commit
c48e6e677: `atan_approx` (atanl) now calls an `#[inline(always)]` copy of the arc kernel (`atan_le_in`) with its constant
operand `ONE`; `atan_le` (the shared, outlined copy for atan2l/asinl/acosl) is a one-line wrapper over the same body.
No rule changed, so no decision record (D-10.80 not used). Decisions in force: D-10.52, D-10.56, D-10.19, D-10.73, D-10.75.

**1. Cycle timing (cbench_libm, `xtask-bin sweep run --only cbench_libm` x3, Liella and glibc interleaved on the same host,
staged tarball of c48e6e677 sha256 fc742e9a...). Ratios Liella/glibc, three runs agree within 2%.**

| row | instr ratio | cycles IVB (E3-1220L v2) | cycles N100 | COVERAGE grp 8 (IVB) |
|---|---|---|---|---|
| asinl  | 9.5  | 7.90 / 7.91 / 8.00 | 5.37 / 5.41 / 5.39 | 817x |
| acosl  | 23.0 | 3.91 / 3.97 / 3.94 | 3.14 / 3.16 / 3.15 | 396x |
| atan2l | 16.7 | 2.80 / 2.77 / 2.81 | 2.50 / 2.51 / 2.50 | 449x |
| atanl  | 26.6 | 2.41 / 2.37 / 2.38 | 2.25 / 2.25 / 2.24 | 457x |
| tanl   | 0.68 | 0.80 / 0.82 / 0.80 | 0.92 / 0.92 / 0.93 | 117x |

Jobs: nico:libm-ld-arc-cycles-ivb (load 1.25 at end), nozomi:libm-ld-arc-cycles-n100 (load 0.00 at start); bookings
159/160 cancelled after the runs. tanl is under 1x; the four arcs are still above 1.5x.

**Cause of the rows above 1.5x (not profiled per phase; a reading, NOT VERIFIED).** The reference runs 26-125
instructions per call, Liella's tier 632-1066 (64x64 multiplies, 128-bit shifts), so the cycle ratio (2.2-8x) sits far
under the instruction ratio (9-27x): the reference's few instructions are long-latency ones. What is left on Liella's
side is a serial chain: asinl/acosl = `sqrtsd` seed -> `TWO116/s0` reciprocal (divsd) -> Newton step -> table index
(divsd) -> `div` reciprocal (divsd) -> correction -> polynomial; atanl/atan2l have two of those divsd. asinl's ratio
(7.9x IVB) is higher than acosl's (3.9x) with near-equal Liella instruction counts (1029 vs 1066), which points at the
reference's asinl being cheaper than its acosl rather than at a Liella phase. Next lever: shorten the chain (one
reciprocal shared by the table index and `div`; the index from the top bits by an integer compare instead of divsd)
and measure in cycles.

**2. Ratchet (atanl 607 -> 657).** Disassembly of the current head (products of `cargo xtask so`): atanl's
`arc::finish<F80, atan>` (159 insns, 5 pushes) called the outlined `arc::atan_le` (362 insns, 7 pushes) twice
(y <= 1 and the reflected 1/y), with x = 1 passed at run time; the earlier head was not checked out (the rules forbid
old checkouts of another story's work), so "inlined before" is inferred from the count, not seen. Fix: the inlined copy.
Counts (shiki:libm-ld-arc-cycles-count2, instructions per call, glibc / base b7c8f799a / new):
atanl 26 / 657.1 / 632.0; atanl_big 26 / 529.6 / 498.4; asinl 1029.0 / 1029.0; asinl_near1 1036.5 / 1036.5;
acosl 1066.0 / 1066.0; acosl_near1 1037.6 / 1037.6; atan2l 869.3 / 869.3; tanl 768.7 / 768.7; tanl_1e300 773.5 /
773.5 (no row slower). **Target <= 607 / 480 NOT reached** (25 / 18 instructions left).

**Accuracy (shiki:libm-ld-arc-cycles-accuracy, `arc_tier_agrees_with_the_wide_route_long`, 10^7 operands x 4 modes per
function vs the 384-bit route):** passed, 0 refused; undecided 0/0/4/8/0 (atan, atan2, asin, acos, tan), worst 2^-103.38,
2^-103.40, 2^-103.41, 2^-103.87, 2^-96.85: identical to the landed head.
**Gate** shiki:libm-ld-arc-cycles-gate (`affected --base origin/next --run`): 183 passed, 0 failed, 31 ignored, peak 1.7G.
**Plants** shiki:libm-ld-arc-cycles-plants (handoff/LIBM-LD-ARC-CYCLES-plants.txt): 3/3 CAUGHT, no WARN.
clippy and static clean on the role (foreground).

**Next step.** (a) The last 25 instructions: rebuild products with `cargo xtask so` (the bench stage overwrites
target/liella with products whose local symbols are gone, so disassemble before staging), split atanl's inlined path
per phase (validation/wl_trace.sh) and compare with the 480-instruction budget of atanl_big; candidates: the two
inlined copies keep `atan_small` outlined (2 calls), `finish`'s format check, the `(y.e, y.m) <= (1)` tuple compare.
(b) The cycle gap: the divsd chain above, measured in cycles on IVB and N100.

**Do not redo.** Timing above (same head). Constants pinned by `arc_constants_are_the_rounded_values`.
