# LIBM-BESSEL-SELFCHECK (#4982, #4983): bug in the check, not in libm

State: branch task/libm-bessel-selfcheck, base origin/next d1b3a596c. Code commit 1f2d336ed, plants 0efd9a863; the
head is the commit that adds this note. Decisions in force: D-10.13 (every double Bessel result correctly
rounded), D-10.106/D-10.110 (tiny band). No new decision record (validation-only change plus a test).

## Verdict
Liella's y1(2.0) and y0(1e-10) are correctly rounded in all four modes. The wl_percall self-check expectation was
the reference library's 17-digit output, which differs from the correctly rounded value by 1 ulp in round to
nearest for both operands. The fault was in the check. No libm band is affected.

| operand | true value (mpmath 1.4.1, 400 bits) | RN bits | Up | Down | Zero | Liella (all modes) |
|---|---|---|---|---|---|---|
| y1(2.0) | -0.10703243154093754688837077228 | bfbb667a39146647 | ..647 | ..648 | ..647 | equal to the oracle |
| y0(1e-10) | -14.732516272697242019498212481 | c02d770c5f760b81 | ..b81 | ..b82 | ..b81 | equal to the oracle |

The true values lie 0.35 ulp (y1) and 0.47 ulp (y0) from the double nearer zero, so neither operand is a hard case.
Three independent sources agree in all four modes: mpmath (black box, scratch venv, tool only), the repository's
384-bit path (`special::y{0,1}_accurate`) and the test oracle (`liella-mathtools::mp`), both checked in the new
test. A black-box probe (probe 623341dd8d5f, glibc vs Liella static) shows Liella returns the oracle bits. The
reference prints ..648 and ..b82 in round to nearest, which is where the 17-digit lines differed.

## Change
- `validation/percall/libm_bessel.c`: `bes_y0_tiny` and `bes_y1_zero` now check with `pc_check_near(r, want)`,
  which prints `within-1ulp-of <want>` or `far-from <want>`. `want` is the mpmath round-to-nearest value, and the
  comment names the oracle. A library that computes a different function still produces MISMATCH.
- `crates/math/tests/fast_bessel.rs`: `double_bessel_selfcheck_operands_match_an_external_oracle` (s10_3_c_0)
  pins both operands to mpmath's bits in all four modes. It checks the entry point, the fast tier when it
  settles, the 384-bit path and the test oracle.

## Results (keke, Zen 1, role n1)
- clippy (libm-bessel-selfcheck-clippy) rc 0; static (libm-bessel-selfcheck-static) rc 0.
- Gate (libm-bessel-selfcheck-gate, `affected --run --base origin/next`): liella-math::fast_bessel, 5 passed /
  0 failed, peak 1.4G, rc 0.
- Plants (libm-bessel-selfcheck-plants, handoff/LIBM-BESSEL-SELFCHECK-plants.txt): 2 CAUGHT, 0 SURVIVED, 0 WARN. Both
  were caught by the new test: a 2-ulp shift in the y0 tiny band and a 2-ulp shift in the y1 table slot at 2.0.
  The percall check change has no plant because no cargo test reaches it. The percall job shows it working
  (MISMATCH -> ok).
- percall (libm-bessel-selfcheck-percall, cases bes_y1_zero, bes_j0_zero, bes_y1_tiny, bes_y0_tiny): check `ok` on
  all four. Counts are unchanged because no libm code changed (bes_y0_tiny 470.5, bes_y1_zero 464227.9 instr/call).
- Sweep (libm-bessel-selfcheck-sweep2, black box, Liella static libm.a, all 4 modes; compared with mpmath locally):
  y1 over the whole slot [2, 2.125) (slot ends plus 3000 samples); y0 over the tiny band [2^-1000, 2^-27) (ends,
  1e-10 and its neighbours, 3000 samples); 500 y0 operands below 2^-1000 (subnormals included). Result: 6513
  operands x 4 modes, 0 wrong. The hardest operand was y1(2.0069894271948656), 8.05e-7 ulp from a midpoint, and
  it was also right. Scripts are in the scratch dir (sweep.c, sweep.sh, cmp_sweep.py, oracle.py); mpmath was used
  as a tool only.
- Process slip: the first commit message went through a shell heredoc, which the brief forbids. Later commits used
  a message file.

## Not to redo / open
- Do not "fix" the libm for these operands: the results are correctly rounded.
- INFORMATIONAL: bes_y1_zero costs 464k instructions per call against 436 for the reference (the band 3 zeros, all on
  the 384-bit path). That is LIBM-F64-BESSEL-BANDS' next step and unrelated to this check.
- Next step: none for this story. Land it with LIBM-F64-BESSEL-BANDS or on its own.
