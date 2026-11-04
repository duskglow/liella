# MATH-M6-F64-GAP-LOG-TABLES-3

State: branch task/math-m6-f64-gap-log-tables-3. Merged origin/next into the -2 branch; the only conflict was
docs/saturation-register.md (split by REGISTER-SPLIT).

Done: this branch's register section (f64 gap breakdown, ranked candidates with the ASSEMBLY/EXP-REDUCE/ROUND-TEST/LOG-TABLES
results) is now docs/saturation/16-f64-exp-log-pow-gap-breakdown.md (heading `## 16.`, subsections 16.1-16.5; next already
uses 13-15) and has an index line in docs/saturation-register.md, which is otherwise next's version. No code changed.
Plants were caught on -2 and no code differs, so none re-run.

Gate: job math-m6-f64-gap-log-tables-3-gate rc 0 (register --check, static, affected: 32 suites ok, 11312 passed, 0 failed;
conformance 114 passed). Clippy --workspace --all-targets --all-features -D warnings clean.

Not to redo: comments in crates/math/src/fast64.rs, crates/mathtools/src/tables.rs and handoffs still say "register 13.5 #n";
that is now 16.5 (left as is: no other change allowed here; a docs/comment follow-up can renumber).
