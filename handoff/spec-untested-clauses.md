# handoff: spec-untested-clauses

State: branch `task/spec-untested-clauses` (from origin/next). HEAD: see `handoff/reports/SPEC-UNTESTED-CLAUSES.json`.

What was done:
- `cargo xtask trace --untested [--out F]` (xtask/src/trace.rs `untested`): report only, always exits 0 unless a file
  cannot be read. It lists every clause of spec/clauses.tsv that no test names, by section, with counts. A clause is
  credited by a Rust `#[test]`'s own text (crates/*/tests and crates/*/src, split by `test_texts`), or by any file
  under conformance/. Pending `clause!` stubs and `commentary!` items are cut out before crediting: before that cut,
  a stub after a test was credited to that test, and the count read 54 instead of 63. Commentary items are left
  out of the list and counted on their own. Unit tests: `sections_sort_by_number`, `resolve_strips_appended_words`,
  `stubs_are_not_tests` (all `no spec clause`).
- List: `handoff/spec-untested-clauses/untested-clauses.md` (regenerated after the tag fix).
- Briefs for the eight worst gaps, plus SPEC GAP and informational sections:
  `handoff/spec-untested-clauses/SPEC-UNTESTED-CLAUSES.md`.
- Tag fix: `s24_5_c_3` on `sym_getdomainname`.

Not done / limits:
- A test named only `sym_<name>` credits no clause, so the list can contain clauses that untagged tests already
  cover. `s0_6_c0_6_0`, `s10_0_c2_0` and `s27_1_c_8` are suspected cases; they were not checked here.
- The plants were not run: the new code is a report and is covered by its unit tests.

Next step: dispatch the eight briefs (`DLOPEN-FAILURE-PATHS` first). Ask the owner about the `s27_1_c_8` SPEC GAP and
about emitting `Notes` items as commentary.
