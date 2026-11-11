# STRTO-FENV-FLAGS: float conversions raise inexact, overflow, underflow (D-8.12)

State: branch `task/strto-fenv-flags-3` (continues `-2` and `-1`), merge of origin/next at
25e0bdd9e, plant commit 03af294ad, handoff/report on top. Ruling #4639. Plan rows: D-8.12,
D-12.26, D-10.95.

## Run -3 (STRTO-FENV-FLAGS-3): CI merge conflict
- CI's merge of -2 stopped on `crates/core/src/num/parse.rs`: next (LIBM-INVALID-CONVERT-SCAN,
  D-10.95) put a `RANGE:` comment on `finish()`'s float-based certain-overflow/underflow bounds;
  this branch had replaced that computation with integer arithmetic (`LOG10_2_E12`, D-8.12).
  Kept the integer version: no float op (no stray inexact) and no float-to-int cast (nothing to
  raise invalid, so the `float-casts` static check has nothing to annotate). Comment updated.
- Lint on the inherited head (`strto-fenv-flags-3-lint0`) and the merged head
  (`strto-fenv-flags-3-lint`: clippy, static incl. float-casts 13/13, records check) green.
- New plant `float-range-bounds` (the float multiply back in `finish()`, `black_box`ed so it is
  not folded): the matrix's exact 27-digit input `1.000...0` must report a stray I.
- First gate/plants on the merge (`strto-fenv-flags-3-gate` rc 124, ended by me;
  `strto-fenv-flags-3-plants` rc 2 baseline) failed in `cargo xtask so`: 3 locale hot-text keys
  (`find_cached`, `attach_charmap` <Files, Glue>, a `find_named` closure of `name::candidates`)
  match no fast-profile section. locale source and the key files equal next's, and next's own
  products build (wl_prep on mei, plain `xtask so`), so the cause is codegen: this branch's code in
  `liella_boundary` (the instantiating crate) changes its inlining. Fix: `tools/hottext/regen.sh .
  --build --allow-drop` once per profile (job `strto-fenv-flags-3-regen`, rc 0, 12/12 layout
  contracts); kept only `hot-text.list` (4 named drops incl. intl `record_for`, `builtin` now
  fast,release), left the rewritten startup-data/probe key files as next has them (commit 4b71ff47f).
  If a later merge of next re-breaks keys the same way, the same one-shot regen is the fix.
- Not redone: counts (no product code changed beyond the conflict resolution, which keeps -2's
  code byte for byte apart from the comment). D-10.95's body still says the strto* converters
  raise no O/U/I "today"; D-8.12 is that separate change (record text left as it is).

## What changed (run -1, unchanged in -2)
- `crates/core/src/num/parse.rs`: `ParsedFloat.inexact`, `Signals` and `ParsedFloat::signals()`;
  `finish()`'s certain-overflow and certain-underflow bounds are integer arithmetic (`LOG10_2_E12`),
  so the conversion raises nothing by accident.
- `crates/legacy/src/num_compat.rs`: `strto_float`/`strtof`/`strtod`/`atof` take the caller's
  `Machine` (mode from MXCSR via `mode_of`, ARCH-tagged) and raise the signals once at the end
  (`raise_signals` -> `fpu::raise`). The double fast paths (`parse_f64_short`, `exact_f64`) keep
  raising inexact in hardware from their single correctly rounded operation (no added work).
- `crates/core/src/scanf.rs` collects `Scanned::signals`; `crates/boundary/src/scanf.rs` raises them.
- `crates/boundary/src/posix/s08_numeric.rs`: every strto*/wcsto*/_l/_internal/atof entry passes `&CSys`.
- Decision record `records/decisions/D-8/D-8.12.toml`.

## Run -2 (STRTO-FENV-FLAGS-2)
- -1's gate went red only on hot_text_products (start-up layout jitter, owner ruling #4347). The
  coordinator held the story until LAYOUT-DATA-CLUSTERS-4C landed (it did).
- Merged origin/next (allowed by the brief). One conflict: next added scanf's ERANGE (D-12.26,
  `Scanned::range`) beside this branch's `Scanned::signals`; both are kept. The flags matrix
  (`conformance/c/s08_strto_fenv_flags.c`) checks scanf's errno again (run -1 had dropped the E code
  for scanf because Liella's scanf did not set ERANGE then): all 11 entries now check flags AND errno.
- Jobs on mei:~/w/s3 (merged head): lint (clippy, static, records check) green; hot_text_products
  21 passed (`strto-fenv-flags-2-hottext`); gate, plants and percall in the hand-back.
- Counts vs origin/next: `validation/wl_percall.sh` with a strtod-family spec (exact/inexact short,
  long, overflow, underflow, strtof, strtold, sscanf %lf; each check also prints the flags raised).
  The spec is in scratch (~/work/STRTO-FENV-FLAGS-2/strtod_flags.c on mei). Instructions per call,
  glibc / next a551cd8c2 / this head: strtod_exact_short 693/235/231, strtod_inexact_short
  619/241/237, strtod_long 1684/9458/9576, strtold 663/6142/6248, strtof 614/3609/3717,
  overflow 239/970/1090, underflow 229/898/1017, sscanf %lf 1377/2074/2117. The cbench strtod rows
  parse "3.14159" (short path: 4 fewer). The long path pays ~110 for the flags the ruling requires
  (not profiled further). Self-check: next raises no O/U on 1e400/1e-400 (glibc does); this head matches.
- Results: gate `strto-fenv-flags-2-gate` 7552 passed / 0 failed, conformance 16/0, peak 14G;
  plants `strto-fenv-flags-2-plants` all CAUGHT (the msg regex matched; "caught by no test the
  log names" because the catcher is a conformance program, not a cargo test).

## Probe-settled behaviour (glibc 2.39, `crates/core/tests/oracle/strto_fenv_flags.c`)
43 inputs x 11 entry points x 4 modes; record `crates/boundary/tests/data/strto_fenv_flags.txt`.
All entry points of one format agree (flags and errno). Flags are IEEE's: inexact iff not exact;
overflow iff the mode's rounding exceeds the largest finite value (toward-zero/away-side truncation
to MAX is I only); underflow iff tiny after rounding and inexact; exact subnormals raise nothing;
ERANGE iff O or U.

## Plants (`handoff/strto-fenv-flags.plants`, through `cargo xtask plants`)
drop-overflow, exact-raises-inexact, drop-underflow, scanf-drops-signals; all must be CAUGHT by the
flags matrix.

## Next step (run -3)
None for this story once the jobs are green. Plan rows to read: D-8.12, D-12.26.
