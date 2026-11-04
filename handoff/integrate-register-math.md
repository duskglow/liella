# INTEGRATE-REGISTER-MATH: f64 gap chain A ported onto next

The four stories are one chain: breakdown is contained in assembly-2, in exp-reduce-2, in round-test-2
(checked with `git merge-base --is-ancestor` on the historic refs). The 18 non-merge commits of round-test-2 ahead of
historic next were cherry-picked in order onto next; the only conflicts were in the old single register file.
- MATH-M6-F64-GAP-BREAKDOWN: PORTED (register section, as docs/saturation/16-*.md)
- MATH-M6-F64-GAP-ASSEMBLY-2: PORTED
- MATH-M6-F64-GAP-EXP-REDUCE-2: PORTED
- MATH-M6-F64-GAP-ROUND-TEST-2: PORTED
Register: historic section 13 clashed with next's section 13, so it is section 16 (`docs/saturation/16-...`);
code and D-10.13 references to "register 13.5" now say 16.5. Rows 1 and 6 carry the original DONE notes; rows 2 and 5 never
got one (their counts were pending in the original handoffs); they stay open for a continuation.
Counts NOT re-measured on next (no timing host booked, budget): the recorded counts are the originals' (keke/shiki).
Plants NOT re-run (code identical to the originals' plants-proven code); sweep ran inside the FULL gate.
Gate: irm-gate (FULL, escalated because the diff touches xtask) rc 0; irm-lint (clippy -D warnings + static) rc 0.
