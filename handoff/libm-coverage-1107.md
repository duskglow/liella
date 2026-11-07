# LIBM-COVERAGE-1107: libm row coverage and the next libm briefs (planning, no code)

State: branch task/libm-coverage-1107, cut from origin/next acc8cfcd; commits 5e4ff7da (coverage table + seven briefs)
and the hand-off commit after it. Everything is in handoff/libm-coverage-1107/: COVERAGE.md (every slower row ->
story/re-time/group, the profile of the uncovered groups, ranking, method column) and seven code briefs.

## Decisions in force
D-10.51 (two-phase `double`), D-10.52 (integer fixed point for x87/_Float128), D-10.53 (complex from real quick
phases), D-10.25/D-10.50 (FMA builds, load-time dispatch), D-10.19 (x87 elementary functions). No new decision rows:
the briefs say "the coordinator reserves the D-id" where one may be needed.

## Results
- 166 rows; 163 slower on at least one class (sinhf, tanhf, tgammaf are not). Covered by open stories: exp + 4 exp
  variants (LIBM-F64-EXP-QUICK/-WIRE), sinl/cosl (LIBM-LD-TRIG-INT), cexp/cexpf (LIBM-CPLX-EXP-TIER). Re-time: the 9
  reopen rows. NONE: 145 rows in 16 ranked groups.
- No libm story landed between 66b8d3ba and acc8cfcd (only LIBM-RETHINK-2, planning), so no row needs re-timing for a
  landing.
- Profile (keke, Zen 1, per call): f64 acosh/asinh/atanh ~1000 instructions vs ~100 (the dd log on every call);
  jn/yn/jnf 122k-471k (384-bit route; jnf costs what jn does); log_upward 371 vs 66 (envelope); SSE2 builds add 40-180
  instructions per f64 call (asin 330 vs 82, tan 359 vs 129).
- Briefs (rank order): LIBM-F64-LOG-QUICK (1), LIBM-F64-TRIG-QUICK (2), LIBM-F64-POW-QUICK (3), LIBM-F32-ONEPASS (4),
  LIBM-F64-EXP-FAMILY-QUICK (5), LIBM-F64-INVHYP-QUICK (6), and LIBM-F64-EXP-IVB-REGRESS (the exp regression; the
  rethink keeps the envelope and today's exp as tier 2, so it does not replace exp outright).
- exp regression evidence: exp_small (same function, [-0.5, 0.5]) 2.92 -> 2.91 while exp 1.86 -> 2.90; the 11-05 exp
  figure was below its own small-argument row, so the brief first settles "landing or 11-05 outlier" by counts and a
  booked re-time of a6fc34c2, then bisects seven candidate merges (listed in the brief).

## Jobs (keke, role n3, tree acc8cfcd clean)
keke:libm-coverage-1107-percall (rc 0), keke:libm-coverage-1107-percall-sse2 (rc 0), keke:libm-coverage-1107-trace
(rc 1: the `sqrt` case has no library call because gcc inlines `sqrtsd`, a probe artifact; the 19 other traces done).
Spec and scripts: ~/work/LIBM-COVERAGE-1107/{cov.c,trace.sh} on keke (copies in the local scratch).

## Open questions / informational
- `ops` was not on PATH on this machine (`type ops`: not found); open stories were taken from ~/state/briefs and
  handoff/libm-rethink instead.
- bench/ has no libm-heavy workload (only `ceil` outside cbench_libm): commonness was judged from general program use.
- MISMATCH lines in the percall log (atanl, csin, clogf, yn, sinhl, jn, jnf, log_upward) are last-place differences
  between glibc and Liella's correctly rounded results (the recorded accuracy difference), not defects.
- The f64 hot kernels are inlined into the exported symbol, so wl_trace cannot split them by function.

## Not to redo
The coverage mapping and the three profile jobs at acc8cfcd. Groups 7-16 are ranked with their method in COVERAGE.md;
their briefs are the next coverage step.

## Next step
Dispatch LIBM-F64-EXP-IVB-REGRESS beside LIBM-F64-EXP-QUICK-WIRE; then LIBM-F64-LOG-QUICK (stacks on WIRE), which
unblocks POW-QUICK and INVHYP-QUICK; LIBM-F32-ONEPASS and LIBM-F64-TRIG-QUICK are independent. Then brief groups 7-8
(long double exp/log and inverse trig on LIBM-LD-TRIG-INT's kernels).
