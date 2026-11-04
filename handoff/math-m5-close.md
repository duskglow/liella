# MATH-M5-CLOSE: handoff (WIP, checkpoint at budget)

State: branch task/math-m5-close, cut by the dispatcher on MATH-M5-CPOW-EXACT (origin/next a92f1fac + CPOW-EXACT's 3
commits; CPOW-EXACT was queued_ci, not landed, at the start: the ZIV-CHAIN-PLAN order says CLOSE runs after it lands).
ZIV-C (M5-4's complex f128) landed on next a92f1fac. Decisions in force: D-10.3 (mathcmp), D-10.17.

## Done (docs only, no code)
- `cargo xtask symcov --tsv docs/spec-symbol-gaps.tsv` on mei s1 (tree f7dfdbf6): 698 undefined, class m = 0.
  docs/spec-symbol-gaps.tsv regenerated (18 m rows gone); docs/spec-symbol-gaps.md counts and the (m) row updated.
- README state line: the milestone-5 libm qualifier removed.
- docs/codemap/README.md: the five M5 `*_products.rs` math tests added to the differential list (#843), special_cr.rs
  and mp_kernels.rs added to the section-10 row.

## Not done
- docs/libm-accuracy.tsv rows for the M5 real names not yet measured: erf, erfc, tgamma, lgamma, j0, j1, y0, y1 in
  format q (_Float128), modes N U D Z. Job math-m5-close-measure (mei) used format `f128`, which mathcmp reads as `f`
  (binary32: cmp.rs parses the first letter; the format letter is `q`): its rows are useless. measure2 (fixed to `q`)
  ended rc 1 after 4 s during the cosh U/D/Z step; cause not read (log ~/work/MATH-M5-CLOSE/cosh.log on mei).
  Not measurable by mathcmp at all (arity list in crates/mathtools/src/cmp.rs): sincos, jn, yn, lgamma_r and every
  complex name. Say so in the table header or extend mathcmp.
- coshf128 directed modes (#848/#854). Explanation from reading cmp.rs (NOT VERIFIED by a run):
  * max ulp is only computed on classified records where glibc or Liella is not correctly rounded, against a
    reference evaluated at 2*prec+40 = 266 bits (`oracle(.., 2 * fmt.prec + 40)`). For tiny |x| (x^2/2 below 2^-266)
    that reference cannot resolve cosh x - 1; if it falls just under 1, `ulp_error` takes the binade below 1 (unit
    2^-113), so a correctly rounded upward result nextup(1) scores 2.000. The same 2.000 appears for cosh f/d/l in mode
    U, where Liella-not-CR is also 0: a metric artefact, not an error. Fix: measure against the value that decided
    the rounding (or at the precision correctly_rounded used).
  * undetermined 98-113 per 1000: records where 4096 bits cannot decide 1 + x^2/2 (|x| below about 2^-2000) in the
    directed modes, the case the table header already names for long double. Fix: an analytic oracle rule for
    cosh(tiny) (result in (1, 1 + x^2)), or report them as decided by that bound.
  * measure2's cosh log shows tiny-x records where both return 1.0 and Liella raises inexact (0x20) and glibc does not;
    which mode those lines are from was not read. If they are mode U, 1.0 is wrong (CR is nextup(1)): check first.
- The plan item's other parts: s10_math_symbols.rs family tests, the final measurement summary in
  docs/differences-from-glibc.md, the milestone-5 Delivered entry.
- No gate, plants, clippy or static run on this head (docs only).

## Exact next step
Rerun ~/work/MATH-M5-CLOSE/m5close2.sh on mei (read cosh.log first for the rc 1), add the q rows, verify the cosh
explanation (which mode the 1.0+inexact lines are from), then the remaining plan-item parts and the direct gate.
Read: plan/06-phases/04-track-m-parallel-from-p2.md "M5-8", handoff/ziv-chain-plan.md sections 2-4.
