# WCTYPE-TRIE-BUILTIN: two-stage class-word tables for the built-in C and C.UTF-8 views (D-5.9)

**State:** branch `task/wctype-trie-builtin-2` (stacked on `task/wctype-trie-builtin`, from origin/next `70cb44d76`).
Commits: `b7aea11b3` (tables + view), `444dd40e8` (first report), `5a6969c1d` (isw_wide argument order, cmp.py fix,
one more plant), then this report. Decision in force: **D-5.9** (plan/07-decisions/s05-character-classification.md; the
rethink's D-NEW-1, scoped to the built-in locales). Design and evidence: handoff/wctype-rethink.md sections 3-6
(rulings #3876 design A, #3877 table size).

## What changed
* `cargo xtask unicode-gen` (xtask/src/unicode.rs `generate_class_words`) also writes
  `crates/core/src/unicode/class_words.rs` (@generated): `C_UTF8_STAGE1` and `C_STAGE1` (`[u8; 4352]`, block = `c >> 8`)
  and `LEAVES: [[u16; 256]; 148]` shared by both (leaf 0 = no class). Rodata: 75.8 KB leaves + 2 x 4.25 KB stage 1.
  C.UTF-8 words = the D-5.4 `Prop.class`; C words from the POSIX locale rules (`posix_class`, XBD 7.3.1).
  The generator fails if the leaves exceed 256 (then widen stage 1 to u16).
* `unicode::class_word(s1, c)`: bounds-checked stage-1 byte, leaf word; 0 above U+10FFFF.
* `LocaleView` gains `classes: &'static ClassStage1`. The built-in arm of `isw_int` (all `isw*`, `isw*_l`,
  `iswctype(_l)` for built-in views) is `class_word(self.classes, c) & mask != 0`. `ctype::wclasses` is unchanged (the
  oracle; non-view callers such as `text.rs`, `scanf` still use it).
* Run 2 (`5a6969c1d`): the first after-counts showed en_US.UTF-8 `isw*`/`isw*_l` **+1 instruction** in 144 of 312 rows.
  Cause (disassembly of `iswalpha`, `__iswalpha_l`): with the built-in arm inline, the only call left in `isw_int` was
  `isw_wide(self, c, mask)`, so the view went to the first argument register and every export moved `c` on entry, on
  its inline paths too. `isw_wide` now takes `(c, mask, view)`; the entry `mov` is gone. No behaviour change.

## Tests and proofs
* `locale::view::class_word_tests::builtin_class_tables_equal_wclasses` (s5_2_g_0, s5_2_c_0): every code point
  0..=0x110000 plus edge values, C and C.UTF-8, class word == `wclasses` and `isw` == membership for the 12 classes.
* Plants (`handoff/wctype-trie-builtin-plants.txt`, `cargo xtask plants`): stage-1 shift, leaf offset, class bit, C.UTF-8
  given the C table, POSIX print rule in the generator, and (run 2) a wrong mask passed to `isw_wide`
  (caught by `view_equals_the_per_call_reads`, en_US/de_DE/ja_JP). Job mei:`wctype-trie-builtin-2-plants` rc 0:
  **6 CAUGHT, 0 SURVIVED, 0 WARN**.
* Lint on `5a6969c1d`: mei:`wctype-trie-builtin-2-lint2` rc 0 (workspace clippy -D warnings, xtask clippy, static).
* Gate (`affected --run --base origin/next`; it escalates to FULL because the generator `xtask/src/unicode.rs` changed)
  on `5a6969c1d`: mei:`wctype-trie-builtin-2-gate`: rc 0: 32 suites ok, 11207 passed, 0 failed, 1254 ignored; conformance 126 passed; peak 16G (page cache), oom_kill 0; 3 SKIPPED lines (capability absent).
  (Run 1's keke:`wctype-trie-builtin-gate` rc 0 proved `b7aea11b3`; superseded by the code change.)

## Counts (instructions per call, wcprobe `count`, median over the 12 classes; before / after / glibc)
Before: keke:`wctype-trie-builtin-count-before` (origin/next `70cb44d76`). After: mei:`wctype-trie-builtin-2-count`
(`5a6969c1d`; outputs ~/work/WCTYPE-TRIE-BUILTIN-2/after2/ on mei). Cross-host check: glibc's 936 rows are identical on
both hosts (delta 0 everywhere), so the comparison holds. **No row is slower**: per (locale, entry, class, range) the
after-before deltas are 0 (216 rows: en_US `isw*`, `isw*_l`, the `tow*` rows), -2 (144: en_US `iswctype(_l)`), and
-18 to -46 (the C and C.UTF-8 rows).

| locale | entry | before | after | glibc (ascii; other ranges) |
|---|---|---|---|---|
| C | isw / isw_l | 35-49 / 41-55 | 17 / 23 | 5; 15 / 4; 12 |
| C | iswctype / _l | 50-64 / 56-70 | 22 / 28 | 28; 8 / 28; 8 |
| C.UTF-8 | isw / isw_l | 53 / 59 | 17 / 23 | 5; 28-36 / 4; 25-33 |
| C.UTF-8 | iswctype / _l | 68 / 74 | 22 / 28 | 21-29 / 21-29 |
| en_US.UTF-8 | isw / isw_l | 16-38 / 22-44 | same | 5-36 / 4-33 |
| en_US.UTF-8 | iswctype / _l | 21-52 / 27-58 | 19-50 / 25-56 | 21-29 |

Full table: `python3 handoff/wctype-trie-builtin/cmp.py before.tsv after.tsv glibc.tsv` (fixed in run 2: it crashed on
the reference rows' `-` load columns). cbench_locale (`--compare --counters`, Liella instr/op, keke before vs mei after): `iswalpha_utf8` 64.1 -> 31.0,
`iswctype_en_us` 73.1 -> 71.1, `iswalpha_en_us` 57.1 -> 57.0, `iswalpha_l_en_us` 66.1 -> 66.1; every other row 0 to
-0.8 except `towupper_utf8` 53.0 -> 53.1 (fractional loop share: glibc's own rows move +-0.1 between the two runs, and
wcprobe's exact `towupper` rows are unchanged). Times on the build hosts are loaded-host noise, not evidence.
Above Latin-1, C.UTF-8 `isw*` is now below glibc by instructions; C ASCII `isw` (17 vs 5) is the remaining gap
(glibc's ASCII path is a direct byte-table read; ours pays the TLS view load, the U+10FFFF bound and the leaf bound).

## Open / next
* Timing on Ivy Bridge and the N100 with handoff/wctype-rethink/trun.sh: PENDING (timing hosts down from 11-07 20:00 UTC
  until about midweek). Counts are the evidence here; the change keeps the instruction mix (scalar loads).
* Next stories (rethink section 8): TOWCTRANS-DESC, WCTYPE-TRIE-FILE (D-NEW-2/3: file-backed lazily resolved stage 1).
* Possible follow-ups: `ctype::wclasses` for C/C.UTF-8 could read the same tables (non-view callers); the leaf bound
  check (`cmp $0x93`) could go if the leaf index were provably in range (no `unsafe` in core).
* Do not redo: the generator, leaf dedup, the view field, the `isw_wide` argument order (moving the view first again
  costs every compiled-locale `isw*` call one instruction). Plants file is reusable for WCTYPE-TRIE-FILE.
