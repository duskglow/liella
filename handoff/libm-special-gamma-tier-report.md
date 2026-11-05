# LIBM-SPECIAL-GAMMA-TIER report (runs 1 and 2)

## Commits (branch task/libm-special-gamma-tier-2)
* 18ad79de (run 1) Add the double lgamma/tgamma fast tier; 68657bc9 (run 1) checkpoint handoff.
* 53924e7f Regenerate the tables with the lgamma/tgamma rows (`cargo xtask tables` on the role).
* f86209f0 Test the gamma fast tier at lgamma's negative zeros.
* 8c8d32ad merge of origin/next 4c33d4c8; e104133f plants file; then this report.

## Tests (crates/math/tests/fast_gamma.rs, job libm-special-gamma-tier-2-t3 on the pre-merge head, all 4 green)
* `double_gamma_fast_tier_rounds_correctly`: 300 random operands + slot edges, zero windows, both sides of every
  pole in (-16, 0); fast tier and entry against the oracle in all four modes; settle rate >= 97%.
* `double_gamma_fast_tier_rounds_correctly_at_the_negative_zeros` (new): the 28 zeros of lgamma in (-16, -2) found
  by bisection on the oracle's sign; the bracketing pair, one more double each side, z(1 +- 2^-s) for s = 12, 24,
  36, 48; four modes. lgamma settled 959 of 1344 calls, tgamma 1344 of 1344; no wrong rounding.
* `double_gamma_fast_tier_matches_the_accurate_path`: 1500 operands x 4 modes, result and status identical.
* Kernel bounds (3000 random + hard + zero operands): worst error / bound lgamma 0.631, tgamma 0.050.

## Fallback rate (Ziv refusals to the 384-bit path)
* Uniform on [-10, 10], 10^5 calls each (Nearest/Up alternating): lgamma 5, tgamma 0.
* Near lgamma's negative zeros, 2000 operands per band |x/z - 1| < 2^-s over the 28 zeros (lgamma, Nearest):
  s = 2, 4, 8: 0; s = 12: 6 (0.3%); 16: 73 (3.7%); 24: 304 (15%); 32: 513 (26%); 40: 678 (34%); 48: 883 (44%).
  The refusal is by design (absolute bound from |L| and |ln D|, `settle` refuses on cancellation): no
  double-double step was added. First-principles residual: within 2^-s relative of a zero the result is about
  |z psi(z)|·2^-s while the bound is ~2^-75·|L| (|L| <= 30 here), so the share that settles falls as s grows.

## Gate, plants, counts
See handoff/reports/LIBM-SPECIAL-GAMMA-TIER-2.json for the final labels and results (gate direct tier; plants via
`cargo xtask plants`, file handoff/plants/libm-special-gamma-tier-2.plants: gamma-bound, gamma-table-entry,
gamma-region-edge, gamma-near-zero-refusal; counts `cargo xtask bench --counters-only --only cbench_libm --rows
cbench_libm/lgamma,cbench_libm/tgamma`, run in a RAM copy of the role, with clippy -D warnings and static first).
Before (recorded in docs/benchmarks.md on origin/next, Ivy Bridge instructions per call): lgamma 586,184
(1429x glibc), tgamma 624,832 (951x glibc). No fresh before job (a second tree would have meant a build off the role).
After (`libm-special-gamma-tier-2-counts`, head e104133f, keke Zen 1, rc 0 after clippy -D warnings and static):
lgamma instructions 1.729x glibc, cycles 1.972x (391.4 / 198.5 cycles per op); tgamma instructions 1.231x,
cycles 1.589x (552.4 / 347.7). Target 1.05x NOT reached; no row slower. Next: profile one call per phase.
Plants (`libm-special-gamma-tier-2-plants`, dir /dev/shm/claude/libm-special-gamma-tier-2/plants-1): 4 CAUGHT, 0
SURVIVED, no WARN. Gate (`libm-special-gamma-tier-2-gate`): escalated to FULL by the tool (generated sources
changed); still running at hand-back. Stubs and workspace clippy steps ok.
Not done: merge of origin/next past 4c33d4c8 (per-family tables, see the handoff step 0).

## Process notes
* Slips: one no-op Bash call with a heredoc and a stdin-waiting `cat` (killed by PID, nothing written); `sed -i`
  on my own new plants file instead of the Edit tool.
* Time went to the keke slot queue: every job waited 15-60 min for one of 4 slots.

## RULING NEEDED / INFORMATIONAL
* None needing a ruling. INFORMATIONAL: the emitted doc line on the LG_* tables names module `fast_special`.
