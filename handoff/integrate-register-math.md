# INTEGRATE-REGISTER-MATH(-2): f64 gap chain A against next

State: branch task/integrate-register-math-2 (continues task/integrate-register-math). origin/next merged in.
Finding: next already contains the whole chain's code and register text (commit "Port the f64 exp/log/pow gap work and
log2/log10 pre-scaled tables onto the public tree", handoffs math-m6-f64-gap-* and log-tables-1..3, register section
docs/saturation/16-f64-exp-log-pow-gap-breakdown.md). Per story:
- MATH-M6-F64-GAP-BREAKDOWN: CONTAINED (next's section 16 keeps its number; this branch's duplicate 16-...-to-glibc.md removed)
- MATH-M6-F64-GAP-ASSEMBLY-2: CONTAINED (code and row 1 DONE note on next)
- MATH-M6-F64-GAP-EXP-REDUCE-2: CONTAINED
- MATH-M6-F64-GAP-ROUND-TEST-2: CONTAINED in code (`fast64::nearest_stage` is on next); the one thing next lacked is the
  stage-count part of `math_double_fast_entry_sweep` (miss rate cap 2^-12 per function), PORTED here.
Conflicts in fast64.rs, docs/saturation-register.md and s10-mathematics.md D-10.13 resolved by taking next whole (it is a
superset: it also has the LOG-TABLES work this branch never had). Net diff against next: that test, plus handoff files.
Not redone: no counts re-measured (no code changed vs next); comments still say "register 13.5 #n" (16.5 now), left for a
docs follow-up.
Open: register rows 2 and 5 still have no DONE note (counts pending in the originals).
