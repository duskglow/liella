# LIBM-RETHINK-HYPOT(-2, -3): f64 hypot, one correctly rounded path for every finite input

State: branch `task/libm-rethink-hypot-3` (= -2's branch + -3's commits, from origin/next c32adc349). Code head
f2c324769; then the D-10.104 record, this note and the report. -3 stopped at its context checkpoint (180k) with the
final-head jobs started on keke role r5: `libm-rethink-hypot-3-gate3`, `-plants3`, `-percall3`, `-lint2`
(gate2/plants2/percall2 were stopped and lint1 was red: the record's body must be one line, a table cell).

## Decisions in force

D-10.104 (records/decisions/D-10/D-10.104.toml, amends D-10.13): method (Borges 2021, Ziv 1991), wiring, the
rulings #4959 (software root `sqrt_soft` in the slow kernel) and #4960 (ERANGE = `float::range_errno`), counts.

## Done (-3)

* Step 0: clippy + static green on the inherited head (`libm-rethink-hypot-3-lint0`).
* -2's gate (shiki) red, cause found: the fast entry's gap exit returned `a * (1 + 2^-60)`, and `1 + 2^-60` is
  exactly 1.0 as a double, so no inexact (boundary fast-tier test: hypot(1, 2^-30); elem_products: 40 pairs like
  hypot(2^-1074, 0.5)). Fix 05a464eee: `a + a·2^-60`, the term built from `a`'s bits (field − 60; exact, normal,
  < ulp/128); a field <= 60 refuses (the cold kernel sets flags). The three failing tests pass on keke (`-t1`).
* The third red test, `init_data_clusters_hold_at_every_text_phase` (D-X.64), was layout, not this change: shiki,
  -2's head: tool 55 at nine pads, 54 at `data 0x3000`; keke, -3's head: tool 54 at all ten pads (pass). No start-up
  code touched; not chased (START-UP LAYOUT JITTER rule).
* Counts (`libm-rethink-hypot-3-percall`, d82210a90, same products code as the final head; Zen 1 build host, instr/call;
  glibc column identical to -1's): typ 110.0 -> 106.0 (glibc 65.9), near 2306.9 -> 106.0 (68.8), bits 3703.4 -> 64.1
  (41.5), new row `hypot_near_up` (FE_UPWARD) 355.6 (68.8). Target (near, bits within 2x) met: 1.54x / 1.54x.
* Plants run 1 (`libm-rethink-hypot-3-plants`, superseded head): gap-27-26, mid-2ym, tie-pick CAUGHT;
  tiny-edge and ziv-60 SURVIVED -> f2c324769 adds the hard cases (largest subnormal beside B = 2^25 + k·2^22 across
  the tininess edges; hypot(s^2 − 1, s) / hypot(s^2, s), roots ~2^-107 off a midpoint); gap-exit-floor caught by
  debug "subtract with overflow" (regex fixed). Plants file: `~/work/LIBM-RETHINK-HYPOT-3/plants.txt` on keke.
* The "f64 sweeps and hard points" are the kernel test's classes (bit patterns, near-binade, gaps 25-29, ties,
  exact roots, DBL_MAX, subnormal edges, fields 27-64, the new two) and elem_products' reference probe, all in the gate.

## Next step (if a final-head job is red)

1. `gate3` red in invtrig's kernel test on the NEW pairs: a real kernel defect at those hard cases (fix the kernel,
   not the test). Other red: read `~/logs/libm-rethink-hypot-3-gate3.log` (keke) with test-summary.
2. `plants3`: all CAUGHT or EQUIVALENT expected (eta-half/-double, shl-wrap declared equivalent with reasons);
   commit its results.txt as handoff/LIBM-RETHINK-HYPOT-3-plants.txt.
3. Follow-up (not this story): the directed path (355.6) is the slow closure + `sqrt_soft`; a boundary `sqrtsd`
   root for `hypot_rounded` (needs a ruling: #4959) would cut it.

Do not redo: design, kernel, -1 trace, counts. Plan rows: D-10.13, D-10.22, D-10.25, D-10.104.
Report: handoff/reports/LIBM-RETHINK-HYPOT-3.json.
