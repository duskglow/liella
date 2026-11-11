# LIBM-INVALID-CONVERT-SCAN-4: report (stacks on -3; supersedes the next steps in handoff/libm-invalid-convert-scan.md)

State: branch task/libm-invalid-convert-scan-4, head e7605b8db (code), then this report commit. Decision in force:
D-10.95 (records/decisions/D-10/D-10.95.toml, not yet on next; its lgammaf sentence is corrected here).
Verdict: INCOMPLETE. Correctness is settled and pushed. The last sign form's counts and all jobs on the final head were
still running at hand-back (budget).

## 1. lgammaf(-2.5): Liella is right, the reference is one ulp off
- Exact oracle: |Gamma(1/2 - n)| = 4^n n!/(2n)! sqrt(pi), so lgamma(1/2 - n) = n ln 4 + ln n! - ln (2n)! + ln(pi)/2,
  evaluated at 90 digits (Python decimal, pi by Machin), rounded to binary32 exactly (scratch oracle.py).
  lgamma(-2.5) = -0.0562437164976740506725945...; RN = -0x1.ccbfap-5 (0xbd665fd0).
- Probe on mei (glibc 2.39 dynamic vs Liella static, lgammaf, lgammaf_r and signgam at -0.5..-39.5 and +-4 ulp around
  -2.5): Liella matches the oracle in value AND sign at all 40 half-integers (lgammaf and lgammaf_r). The reference
  returns 0xbd665fcf at -2.5 (the next float up: faithful, not correctly rounded) and differs by one ulp at 18 of the 40
  (-2.5, -5.5, -6.5, -10.5, -11.5, -15.5..-19.5, -24.5, -29.5..-31.5, -33.5, -34.5, -38.5, -39.5); signs agree
  everywhere. -17.5 lies 0.0002 ulp from a rounding boundary; Liella still rounds it right.
- Tests: crates/math/tests/s10_math_clauses.rs (s10_3_c_r6) pins nine half-integers (value and sign) against the exact
  table and checks the oracle (`cr`) against the same table; -2.5's +-4 ulp neighbourhood in all four modes against the
  oracle. validation/percall/lgammaf.c: the lgammaf_neg check reads "faithful" only at -2.5 and only for either
  neighbour of the exact value (pinned in the comment); every other result prints its value. On head 2ab... (first
  commit) the count run's check column read `ok` for all three rows.

## 2. Ratchet (wl_percall --spec validation/percall/lgammaf.c, base = merge base with next, instructions/call)
| form | lgammaf_pos | lgammaf_neg | lgammaf_r_neg |
|---|---|---|---|
| base (next) | 315.8 | 8411.0 | 8419.0 |
| -3 bit read of trunc parity | 318.8 | 8413.1 | 8421.2 |
| -4 commit 1: sign = Gamma(x) < 0 (bool + select) | 317.6 | 8401.0 | 8409.0 |
| -4 commit 2 (e7605b8db): (bits(Gamma) as i64 >> 63) as i32 OR 1 | NOT MEASURED (job h2-counts) | | |
Commit 1 measured on mei (label libm-invalid-convert-scan-4-counts, superseded). The sign now comes from the Gamma(x)
the fast path computes anyway (exact sign: every factor x + j exact and nonzero, P > 0; other branches have x > 0), so no
float-to-integer conversion and no parity code remain; trunc_is_odd is gone.

## Jobs on the final head (mei, started detached, NOT read)
libm-invalid-convert-scan-4-h2-clippy, -h2-static, -h2-accuracy, -h2-counts, -h2-hottext, -h2-gate (sequential, one
script ~/work/libm-invalid-convert-scan-4/jobs2.sh), -h2-plants (handoff/LIBM-INVALID-CONVERT-SCAN-plants.txt, RAM dir
plants-2; new plants gamma-sign-flip and lgammaf-oracle-ulp replace gamma-parity-shift).
Superseded (commit 1, rc 0): -clippy, -static, -accuracy (s10 56 passed, gamma lib 5, no_invalid 2), -counts. Killed
on commit 1: hottext, gate, plants (the code changed).

## Next step
Read the h2 jobs. If h2-counts shows lgammaf_pos <= 315.8, the story is COMPLETE. If not, revert to commit 1's bool form
only if it measured lower, and look at the disassembly of lgammaf (Liella's libm.a) for where the pos path's extra
1-2 instructions sit (the select at the merge, or the movq of Gamma). Then the scratch disasm check from -3 (step 2 of
handoff/libm-invalid-convert-scan.md) and Ivy Bridge timing stay the coordinator's call.

## INFORMATIONAL
- lgammaf on negative operands is 21x the reference by instructions (8401 vs 395): LIBM-F32-REST-NEXT's (not started).
- handoff/libm-invalid-convert-scan.md (area page) was not rewritten this run (budget); this file supersedes its
  "Next steps".
