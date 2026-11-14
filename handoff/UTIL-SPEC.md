# UTIL-SPEC (UTIL-0): spec §31 "Utilities", series D-31, view s31

**State.** Branch `task/util-spec` (from `origin/next` fa251fbb0). Code head `2e0ccf51e`; this note and the report
are the commit after it. Stopped at the context checkpoint (~150k of the rules; the brief's 280k not reached):
everything in the brief is written and checked except the plants run (started detached, see below).

## What is on the branch

* `glibc-functional-spec.md` §31 (Contents entry too): 31.1 common conventions (paths table, `--help`/`--version`,
  usage errors), 31.2 ldconfig, 31.3 ldd, 31.4 getent, 31.5 getconf, 31.6 iconv, 31.7 locale, 31.8 localedef,
  31.9 zic, 31.10 zdump, 31.11 tzselect, 31.12 gencat, 31.13 pldd, 31.14 not provided. Every item cites POSIX, a
  man page, RFC 8536, Appendix D or a probe case. **Liella rule** items: identity strings (Q3), `-c old|compat`
  refused (Q5), ldd never runs the operand or another interpreter and `-d/-r/-u` refused until DL-10 (Q6), `iconv -l`
  lists what converts, not provided (Q4), locales commands (Q8), ldconfig determinism/atomic replace/`-r` mechanism.
* Probe: `tools/probe/util_spec.sh` (runs the commands only; scripts are run, never read), record
  `spec/behaviour/utilities.glibc.txt` (Ubuntu 24.04.5, libc-bin 2.39-0ubuntu8.9, tzdata 2026e), document
  `spec/behaviour/utilities.md` (case-to-clause map, what the probe corrected, gaps).
* `xtask/src/spec.rs`: §30 no longer ends the clause body; it is skipped (`NO_CLAUSES`), so §31 is read; `SLUGS` has
  32 entries. Unit test `the_catalogue_gives_no_clauses_and_the_section_after_it_does`.
* `cargo xtask stubs` output: 107 clauses `s31_*` in `spec/clauses.tsv` and `crates/core/tests/s31_utilities_clauses.rs`.
* `spec/schedule.tsv`: owner `UTIL` (track, from VAL, join POST, started 2026-11-14) and `assign §31 UTIL`.
* `records/views.toml` view `s31` (series D-31, `plan/07-decisions/s31-utilities.md`) and `records/views/s31.head.md`;
  the train renders the view (not committed here, per the rules). `records/pending/UTIL-SPEC-1..9.toml` (D-31):
  1 spec home and tooling, 2 shape (Q1, Q2), 3 formats stay compatible (owner's ruling), 4 identity strings,
  5 not installed, 6 `ldconfig -c`, 7 ldd, 8 locales commands, 9 ldconfig determinism/atomicity/untrusted input/`-r`.
* `docs/differences-from-glibc.md`: new section "Utilities (spec §31)", five rows, each with its reason for no
  compatibility option.

## Results (keke, role n1)

* Probe jobs `util-spec-probe`, `-2`, `-3` rc 0 (the record is from `-3`).
* On `881942dde`: `util-spec-stubs` rc 0 (1377 clauses across 31 sections; trace clean), `util-spec-clippy` rc 0,
  `util-spec-static` rc 0 (codemap: 1 file pending regeneration by the train); `util-spec-xtest` rc 101: the new
  test's fixture used a 2-byte clause text, which the generator drops by design; fixed in `2e0ccf51e`.
* On `2e0ccf51e`: `util-spec-stubs-2`, `util-spec-xtest-2`, `util-spec-clippy-2`, `util-spec-static-2` (see the
  hand-back for their status); plants `util-spec-plants` (`handoff/UTIL-SPEC-plants.txt`, 2 plants on the parser).

## Probe-settled (differs from the design note)

* A non-PIE static program: `\tnot a dynamic executable`, exit 1; only static-pie gives `\tstatically linked`
  (`plan/utilities.md` §2.3 had both as `statically linked`).
* `ldconfig -r ROOT` works for an ordinary user on the reference, so `-r` without privilege is no difference.

## Open

* RULING NEEDED (small): the schedule track `UTIL` (join POST) and §31 stubs living in `crates/core/tests`
  (recommend both as written; a later `liella-tz`/`liella-locale` crate can take its clauses with a stubs mapping).
* The `implementation-plan.md` index row for `plan/07-decisions/s31-utilities.md` waits until the train renders it.
* `spec/behaviour/README.md` has no row for `utilities.md` yet (one line).
* Gaps for the stories: pldd output on success (UTIL-15), getconf padding of long names (UTIL-5), `ldconfig -p` hwcaps
  label (UTIL-3), `locale -a -v` for built-in `C.utf8` (UTIL-8), localedef exit 1 with warnings (UTIL-12), messages in
  other locales.

## Next step

Check `util-spec-plants` (both CAUGHT expected) and the `-2` checks; add the README row; then land. Do not redo the
probe; extend `tools/probe/util_spec.sh` if a story needs a case.
