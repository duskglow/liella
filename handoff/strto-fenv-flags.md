# STRTO-FENV-FLAGS: float conversions raise inexact, overflow, underflow (D-8.12)

State: branch `task/strto-fenv-flags`, code head 53540d311 (handoff commit on top). Ruling #4639.

## What changed
- `crates/core/src/num/parse.rs`: `ParsedFloat.inexact`, `Signals` and `ParsedFloat::signals()`;
  `finish()`'s certain-overflow and certain-underflow bounds are integer arithmetic (`LOG10_2_E12`),
  so the conversion raises nothing by accident.
- `crates/legacy/src/num_compat.rs`: `strto_float`/`strtof`/`strtod`/`atof` take the caller's
  `Machine` (mode from MXCSR via `mode_of`, ARCH-tagged) and raise the signals once at the end
  (`raise_signals` -> `fpu::raise`). The double fast paths (`parse_f64_short`, `exact_f64`) keep
  raising inexact in hardware from their single correctly rounded operation (no added work).
- `crates/core/src/scanf.rs` collects `Scanned::signals`; `crates/boundary/src/scanf.rs` raises them.
- `crates/boundary/src/posix/s08_numeric.rs`: every strto*/wcsto*/_l/_internal/atof entry passes `&CSys`.
- Decision record `records/decisions/D-8/D-8.12.toml` (reserved id).

## Probe-settled behaviour (glibc 2.39, `crates/core/tests/oracle/strto_fenv_flags.c`)
43 inputs x 11 entry points x 4 modes; record `crates/boundary/tests/data/strto_fenv_flags.txt`.
All entry points of one format agree. Flags are IEEE's: inexact iff not exact; overflow iff the
mode's rounding exceeds the largest finite value (toward-zero/away-side truncation to MAX is I only);
underflow iff tiny after rounding and inexact; exact subnormals raise nothing; ERANGE iff O or U.
Liella's `float::round` already decided exactly these; the first run of the matrix
(`conformance/c/s08_strto_fenv_flags.c`) showed every flag equal to the reference.

## Results
- lint (clippy -D warnings, records check, static) green on 99506e00c (`strto-fenv-flags-lint4`);
  only a test file changed after it.
- Plants run 1 (`strto-fenv-flags-plants`) stopped at its baseline: scanf errno mismatches only
  (below); fixed in 53540d311 by checking scanf's flags only.
- Gate, plants, counts and hot_text_products requeued on the final head (labels in the hand-back).

## INFORMATIONAL
- The reference's scanf sets errno ERANGE on a float range error (`%f`, `%lf`, `%Lf`); Liella's
  scanf does not. This predates this story (the brief keeps errno as it is). Next step: a small story
  to add ERANGE to `crates/core/src/scanf.rs` Conv::Float (`parsed.range != FloatRange::Ok`) and
  re-enable the E code for scanf in the conformance test.
- Counts: only the strtod long path gained one branch (`raise_signals` on an exact result); the short
  path's code is the same. Not measured against origin/next (budget); the counts job gives Liella
  vs glibc for cbench_locale strtod rows.

## Next step
Read the requeued jobs; if all rc 0, the branch is ready. Plan rows: D-8.11, D-8.12.
