# MATH-M6-FPENV-HONOR-REST-4: nothing left to port; next already carries the caller's FTZ/DAZ work

State: branch task/math-m6-fpenv-honor-rest-4, cut from origin/next at 432f1ef7. No product code changed (this
branch adds only this note and the report). Role keke:~/w/n2.

Decisions in force: D-10.22 (owner ruling 2026-10-23, #1509): libm honours the caller's FTZ/DAZ; x87 `long double`
is not reached by FTZ/DAZ. Probe record: spec/behaviour/math-ftz-daz.md (clause s10_0_f_0).

## What next already contains (step 1 of the brief)

The historic branch (liella-historic task/math-m6-fpenv-honor-rest-3) has no merge base with next, so it was compared
file by file (historic blob vs the file on next), not by commit. For each file the REST-3 work touched:

| Historic content (REST-3 and its -REST/-REST-2 parents) | On next |
|---|---|
| `mathret::Soft` (`ret`, `ret_out`, `remquo`, `intmax`, `uintmax`, DAZ operand read, FTZ result flush) | present |
| operand-returning forms `kept`, `pick`, `step`, `raw`, `stepped` (probed #1549/#1584) | present, identical text |
| x87 unit never flushed (`Soft.x87`, set by `Soft::enter(Unit::X87)`; daddl/faddl/fmull narrowing) | present |
| `Denorm` trait with `daz`/`ftz(tiny)`; tininess edge flushed under FTZ | present |
| table test `math_software_paths_honour_the_callers_ftz_and_daz` (57 rows, `Res::Kept`; fma, fdim, narrowing, ldexp/scalbn, nextafter family, min/max, rounding family, fabs/copysign) | present, 57 rows |
| spec/behaviour/math-ftz-daz.md | byte-identical |
| D-10.22 row (per-family rule, x87 note) | present |
| `s10_math_gen.rs` / mathret.rs | next is ahead: no historic-only line in mathret.rs; the only historic-only lines in the generated file are older `exp`/`exp10`/`exp2` elem calls that next has since replaced |

Result: a diff of the four files shows no historic-only FTZ/DAZ content (mathret.rs 0 historic-only lines,
math-ftz-daz.md 0, the table test: one line, the array length of an unrelated case list). INTEGRATE-MATH-5 and the
other math stories evidently carried all of it. Nothing was merged or cherry-picked.

## Proof on next (jobs on keke)

- math-m6-fpenv-honor-rest-4-gate: `cargo xtask affected --base origin/next --run` (selects nothing for an unchanged
  tree) then `cargo test -p liella-boundary --test s10_math_symbols` (the 57-row table under FTZ, DAZ and both).
- math-m6-fpenv-honor-rest-4-plants (RAM copy /dev/shm/claude/math-m6-fpenv-honor-rest-4/plants-1; script
  ~/work/math-m6-fpenv-honor-rest-4/plants.sh): kept-flushed (operand-returning form flushed), arithmetic-not-flushed
  (Soft::ret never flushes), x87-narrowing-flushed (x87 skip removed), daz-ignored (operand read skips DAZ),
  normal-flushed (f64 flush widened to a normal value). The script exits non-zero if one survives.
- clippy (--workspace --all-targets --all-features -D warnings) rc 0 and `cargo xtask static` rc 0 on keke n2.

## Open / not done

- Narrowing from `_Float128` (f32addf128 ...) was never probed against the reference; Liella applies both rules
  there as for double. Not part of this story.
- #1551 (wl_percall for ceil/floor/fmax) only if the workload gate flags the extra MXCSR read: not run.

Do not redo: the port, the probes, the table.
Next step: none for this story; the historic branch can be treated as fully integrated.
Plan rows: D-10.22 (plan/07-decisions/s10-mathematics.md).
