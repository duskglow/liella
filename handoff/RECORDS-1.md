# RECORDS-1: decisions, compat flags and saturation rows become records; the tables become generated views

Branch `task/records-1` (from `origin/next` 01ff5a63). Commits: the engine and the conversion (one commit, so the
conversion proof runs against it), then D-X.55, CONTRIBUTING.md and the plants file, then this hand-off.

## What landed

* `xtask/src/records/` (no dependency): `toml.rs` (strict canonical subset, parser and writer; `parse_lenient` for
  `fmt`), `schema.rs` (kinds, key order, id shapes, sort key, cell rule), `render.rs` (views, derived
  superseded/withdrawn/amended markers, the saturation index from the section headings), `check.rs` (validation), `mod.rs`
  (loading, `records-gen [--check]`, `records check | fmt [--check] | new`). 28 unit tests.
* `cargo xtask static` gains `records` (validation, always fatal) and `records-gen` (freshness: fatal under
  `LIELLA_TRAIN=1`, else one advisory line `records-gen: N views pending regeneration by the train`). Stale message:
  ``<file> is stale: run `cargo xtask records-gen` ``.
* Readers: `trace` takes its decision-id set from the records (DUP-D removed; `records/` scanned for citations; the
  records module's own example ids are not citations, like trace.rs's); `compat-ids` reads `records/compat/`;
  `register` checks unique section numbers (gaps allowed), headings (after the `@generated` line), row ids across the
  SR records and the hand-written sections.
* `tools/records_import.py` (stdlib): conversion, `--verify [--base REF]` (the proof), `--from-diff BASE BRANCH` (rows a
  pre-cutover branch added become records; saturation rows go to `records/pending/` as `SR-NEW`).
* `records/`: 470 + 1 (D-X.55) decision, 20 compat, 85 saturation records; `views.toml` with 38 views; heads/tails in
  `records/views/`. Views regenerated: 28 decision tables, `docs/compat-flags.md`, `docs/saturation-register.md`,
  `docs/saturation/01..07-*.md`.
* `records/decisions/D-X/D-X.55.toml`: the layout rule. CONTRIBUTING.md: where a decision goes; tables generated.

## The conversion proof (`tools/records_import.py --verify --base origin/next`, full output in RECORDS-1-verify.txt)

Two implementations: Python parses the old tables at origin/next; the views on disk were rendered by the Rust
`records-gen`; Python parses those and compares. Result: 38 views; heads, tails, header and separator rows byte-identical;
row multisets byte-identical (470 decision, 20 compat, 85 saturation); order = id order; plus D-X.55, the one record
added after the conversion. Normalisations, each listed by file and line in RECORDS-1-verify.txt:

* (a) the `@generated` first line: 38 views.
* (b) id order in exactly the 10 unsorted tables (s01, s02, s04, s07, s10, s13, s14, s22, s26-29, x): 44 rows moved.
* (c) lines inside a table dropped: x lines 47 (blank) and 48 (`---`), as the brief lists, and **also s10 lines 47 and
  49 (blank)**: rows D-10.62 and D-10.66 sat after blank lines and rendered as paragraphs, like the x rows.
* (d) saturation index: the blank line (27), section 24 moved after 23, 7 titles taken from the headings (19-25).
* (e) **35 rows had a bare `|` inside a cell** (`PARSE_ARGV0|NO_ERRS`, `(a|b)`, `|n|`, ` | ` in code): GitHub
  splits a cell there and drops the text after it. Escaped as `\|` (renders the same `|`), as the cell rule requires.
* (f) **two empty cells written `| |`** (saturation 05 line 14, 06 line 7) now `|  |`, the one spelling of an empty cell.
* (g) **COMPAT-4's Liella cell held a raw 0x01 byte** where `(a*)*\1` was meant (a shell turned `\1` into the byte; the
  subset cannot store it); written `\1`.

(e)-(g) and the s10 half of (c) are beyond the brief's list: see RULING NEEDED in the report.

## Same reports before and after (rec1-baseline on origin/next, rec1-lint on the head)

* trace before: `452 plan rows, 0 defined more than once, 0 cited without a row`; after: `471 decision records, 0
  cited without a record`. 452 + the 18 D-H rows (the old scan never counted `D-H<n>` as an id) = 470, + D-X.55. The
  uncited list is empty both times. The other trace lines are identical (5484 tests ...; 336 rows; 54 documents).
* compat-ids ok before and after (20 records, contiguous); register ok before and after.

## Determinism (rec1-dev-2)

`records-gen` twice: identical view digests, second run rewrote nothing; the 618 files of `records/` copied into a
fresh tree in reverse path order render identically (`records-gen --check` clean); `records fmt --check` clean; all 577
TOML files parse with Python's `tomllib`, which reads the same ids and bodies.

## Plants (`handoff/RECORDS-1-plants.txt`)

Seven data plants caught by `cargo xtask static` (stem differs from id, duplicate id in another series directory,
supersedes to a missing id, keys out of order, a hand-edited view under `LIELLA_TRAIN=1`, a bare `|` in a cell, a
compat record with `date` and no `decisions`). The supersede cycle needs two records, which a one-site plant cannot
write: that plant disables the cycle report in check.rs and the unit test `link_problems` (which builds a two-record
cycle) catches it.

## Deviations from the design, and what is left

* `views.toml` has one `section` per view (each view is one table), not a `sections` list.
* Saturation cells other than `row` may be empty (6 converted cells are); decision and compat cells may not.
* `records new` writes empty `story`/`date`/column values that `records check` rejects until filled (no clock in the
  tool, and an unfilled skeleton must not pass).
* The view heads and tails are byte-identical to the old text (proof), so three still describe hand-editing:
  `records/views/saturation-register.head.md` ("Put a new row in the section file ... contiguous"),
  `records/views/compat-flags.tail.md` ("Rules for adding a row") and the overview prose in `records/views/h.head.md`.
  RECORDS-2 (docs) should rewrite them; they are records now, so that is an ordinary edit plus regeneration.
* Not in this story (RECORDS-2): `records list`, `records show`.
