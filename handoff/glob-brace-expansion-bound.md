# GLOB-BRACE-EXPANSION-BOUND

Branch task/glob-brace-expansion-bound (code head 3a13fb14; the handoff commit after it changes only handoff files).

## What was done
`glob` with `GLOB_BRACE` counts the patterns the braces would expand to before expanding anything
(`brace_count` in crates/core/src/pattern/glob.rs: product over top-level groups of the sum over alternatives,
`checked_mul`/`saturating_add`, stops at the first excess). It returns `GLOB_NOSPACE` when the count passes
`BRACE_MAX_ALTERNATIVES` = 2^14 or count x pattern length passes `BRACE_MAX_BYTES` = 64 MiB. An unbalanced `{` is
still `GLOB_NOMATCH` (and is now found without expanding the groups before it).

Bound sizing: 2^14 is 14 two-way groups or four 11-way groups; shell-style patterns have a handful of alternatives.
The pinned fuzz input (fuzz/seeds/glob/timeout-5351ade2, 359 bytes) is far over it. No measurement of real-world
patterns was made (none in the repo name more than a few dozen alternatives); the bound is a judgement, cheap to move.

## Decisions in force
D-22.14 (plan/07-decisions/s14-...md; PROVISIONAL coordinator decision from #2401): needs a D-id. Differences row in
docs/differences-from-glibc.md (no compat flag: restoring means unbounded expansion).

## Tests
`s14_8_c_2_1_brace_bound` (crates/core/tests/s14_search_clauses.rs): ordinary patterns (nested, empty alternatives,
escapes, unbalanced) unchanged; edge 16384 expanded / 16385 refused; nested sums; product overflow; NOCHECK; a
pathological input of the fuzz shape under a hang guard (5 s, scaled). Plants (script ~/work/GLOB-BRACE-EXPANSION-BOUND/plants.sh
on keke): no bound, off-by-one, wrapping product, nested sums ignored.

## Jobs (keke)
glob-brace-expansion-bound-lint3 (clippy -D warnings + the test + cargo xtask static, rc 0),
glob-brace-expansion-bound-gate (affected --run, rc 0), glob-brace-expansion-bound-plants2 (3 plants CAUGHT; the wrapping-product plant of the first run, glob-brace-expansion-bound-plants rc 1, is equivalent by design and superseded).

## Next step
Coordinator assigns the D-id (replace D-22.14 in the decision row and the differences row).
Note: the fuzz harness still limits `{` to 8; with the engine bound that limit is no longer needed for time.
