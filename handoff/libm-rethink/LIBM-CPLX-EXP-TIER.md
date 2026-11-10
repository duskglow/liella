# LIBM-CPLX-EXP-TIER: cexp and cexpf from the real quick kernels, the 384-bit route as the last tier

Id: LIBM-CPLX-EXP-TIER
Kind: code
Priority: 88
Size: L
Host: -
Model: opus
Effort: high
BUDGET: 100 tool calls / 200k

## The change

Decision row: D-NEW "Complex functions from the real quick phases with a per-part rounding test". Today every finite
`cexp`/`cexpf` operand off the axes runs the 384-bit route (`crates/math/src/complex_body.rs` `cexp`, 101 KB: read by
`grep -n` and ranges only; put new code in a new file), 116 554 / 116 569 instructions per call against glibc's 162 /
239 (DESIGN.md §2, §5.2). Add a tier 1 in front of it:

- `double`: e^x as (double-double, scale 2^m, bound) from the fast64 exp kernel, sin y and cos y as double-double
  with bounds from the fast64 trig kernels (find their double-double + bound form by `grep -n`; if the trig tier only
  returns rounded doubles, factor out a kernel returning (double-double, bound) first, as its own commit).
- Each part: the double-double product of e^x with cos y (sin y), relative bound ε_exp + ε_trig + the product's; one
  rounding test per part with that bound (`fast64::near`'s form); scale by 2^m only inside the normal range (near
  overflow or underflow: fall back).
- `float`: expf, sinf, cosf kernels in `double` (bound about 2^-48) and each part rounded to `float` with an online
  test (DESIGN.md §3.1 B).
- Fallback: today's route, unchanged, also for every special and degenerate case (the axes already call the real
  functions; keep that code).
- Modes: round-to-nearest in place first; directed modes keep today's envelope.

## Tests

The existing complex suites (all four modes, special values, exact cases); a seeded random comparison with the 384-bit
route over >= 10^7 operands per type; y near π/2 + kπ (cos y tiny: the trig kernel's RELATIVE bound must hold there),
large |y| (the Payne-Hanek path), x near the overflow and underflow thresholds; both builds identical.

## Plants (`cargo xtask plants`, `msg:` per plant)

ε_trig dropped from a part's bound; sin and cos swapped; the scale applied twice; the float path without its test.

## Counts and timing

`cargo xtask bench --compare --counters cbench_libm` complex rows: targets cexp <= 300 instructions, cexpf <= 120.
Timing required (the instruction mix changes completely): book an Ivy Bridge and the N100.

## JOBS

lint, gate, plants, counts, `<ivb-host>:<label>`, `nozomi:<label>`.

Hand-back first line: `COMPLETE: cexp/cexpf quick tier: <n>/<m> instructions (glibc 239/162), Ivy Bridge <x>x`
