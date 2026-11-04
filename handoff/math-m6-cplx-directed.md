# MATH-M6-CPLX-DIRECTED (M6-10): handoff (one page)

State: branch task/math-m6-cplx-directed from origin/next 442429ab (CLOSE landed, per ZIV-CHAIN-PLAN 2-4). Code head
d254f3c4 (test + D-10.17 text only; no product code changed). Role kanon:~/w/kn1. Decisions: D-10.17.

## Done
- crates/math/tests/complex_directed.rs: the 16 two-operand families against the oracle (Ziv loop to 2^14 bits) on a
  grid of part exponents over each format's range (smallest subnormal and normal, -emax/2 and -emax/8 capped at 4096/600,
  +-2p, +-p/2, 0, those positive, emax where <= 4096), x significand 1 and 1+ulp, both signs, all four modes. Pure Rust
  (no products build). float and double: 0 differing parts, 0 oracle-undecided (probe job math-m6-cplx-directed-probe).
  So D-10.17's milestone-4a known limit (directed modes in the asymptotic regimes) is covered by the ladder for float
  and double; D-10.17 now says so.
- Plants (complex.rs ladder): noladder (384 only) CAUGHT, nowider (stop at 768) CAUGHT (plants2 on 94dd8cba; plants3 on
  d254f3c4 started detached).

## Open findings (the follow-up story; grids are #[ignore]d with the reason, run with --ignored)
Probe on 42d46240 (oracle `complex` for all; wide formats should use `complex_wide`, fixed after the probe, not rerun):
- long double and _Float128: many parts reach the 3072-bit residual path (casin/casinh/cacos/cacosh/catanh/catan, cexp,
  clog, ccos/ccosh at p64), e.g. catan p113 x = 2^-16494, y = 2^4096 in Up/Down/Zero. That contradicts ZIV-C's
  "no binary128 input needs more than 3072 bits" on this grid. Counters were global under parallel threads, so counts
  are inflated; the per-call lines still show it. NOT diagnosed: exact/Dev part missed or bound too loose.
- catan 102 / catanh 126 differing parts at p64 and p113; ctan/ctanh 576 each (all modes incl. nearest, x = +-2^-4096:
  likely the oracle without `complex_wide`). 36-182 operands per family beyond the oracle's 2^14 bits.
- f64 ctanh reached the residual path at x = +-2^511 (parts still correct).

## Next step
Run `cargo test -p liella-math --test complex_directed -- --ignored --nocapture --test-threads=1` on this head, list
the differing and residual operands per family, classify (oracle, exact part missed, loose bound) and fix as ZIV-C
Step 0 did. Then drop the #[ignore]s. RULING NEEDED: whether M6-11 may start before the wide-format findings close
(recommend yes: M6-11 is float/double, whose fallback is now checked in every mode).

## Jobs (kanon)
probe (rc 101, findings above), gate rc 4 / gate2 rc 4 (clippy in my test: macro depth, needless_range_loop; fixed),
plants rc 3 (baseline did not compile), plants2 rc 0 (94dd8cba). Final: gate3, plants3 (d254f3c4), started detached.
