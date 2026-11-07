# WCTYPE-TRIE-BUILTIN: two-stage class-word tables for the built-in C and C.UTF-8 views (D-5.9)

**State:** branch `task/wctype-trie-builtin` from origin/next `70cb44d76`; code commit `b7aea11b3`, then this report.
Decision in force: **D-5.9** (plan/07-decisions/s05-character-classification.md; the rethink's D-NEW-1, scoped to the
built-in locales). Design and evidence: handoff/wctype-rethink.md sections 3-6 (rulings #3876 design A, #3877 table size).

## What changed
* `cargo xtask unicode-gen` (xtask/src/unicode.rs `generate_class_words`) also writes
  `crates/core/src/unicode/class_words.rs` (@generated): `C_UTF8_STAGE1` and `C_STAGE1` (`[u8; 4352]`, block = `c >> 8`)
  and `LEAVES: [[u16; 256]; 148]` shared by both (leaf 0 = no class). Rodata: 75.8 KB leaves + 2 x 4.25 KB stage 1.
  C.UTF-8 words = the existing D-5.4 `Prop.class`; C words from the POSIX locale rules (`posix_class`, XBD 7.3.1).
  The generator fails if the leaves exceed 256 (then widen stage 1 to u16).
* `unicode::class_word(s1, c)`: bounds-checked stage-1 byte, leaf word; 0 above U+10FFFF.
* `LocaleView` gains `classes: &'static ClassStage1` (C or C.UTF-8 stage 1; a compiled LC_CTYPE keeps the C one, unread).
  The built-in arm of `isw_int` (all `isw*`, `isw*_l`, `iswctype(_l)` for built-in views) is now
  `class_word(self.classes, c) & mask != 0`. File-backed views are untouched. `ctype::wclasses` is unchanged (the oracle;
  non-view callers such as `text.rs`, `scanf` still use it).

## Tests and proofs
* New `locale::view::class_word_tests::builtin_class_tables_equal_wclasses` (s5_2_g_0, s5_2_c_0): every code point
  0..=0x110000 plus 0x7fffffff/0xfffffffe/0xffffffff, C and C.UTF-8: the class word == `wclasses`, and `isw` == membership
  for each of the 12 standard classes. Passes; `view_equals_the_per_call_reads` still passes (job `wctype-trie-builtin-test`).
* Plants (`handoff/wctype-trie-builtin-plants.txt`, run through `cargo xtask plants`): stage-1 shift, leaf offset, class
  bit, C.UTF-8 view given the C table, POSIX print rule in the generator (regenerates, then tests). Job keke:`wctype-trie-builtin-plants-2` rc 0: 5 CAUGHT, 0 SURVIVED, 0 WARN-NO-MSG/RC-ONLY (one WARN-DEV-SHM notice: the RAM dir is under /dev/shm; no test here mounts /dev). The first run `wctype-trie-builtin-plants` rc 3 was REFUSED by the tool: the leaf-offset pattern matched two sites (same line in `props`); re-anchored on the leaf read.
* Gate (direct tier, `affected --run --base origin/next`): keke:`wctype-trie-builtin-gate` still RUNNING at hand-back (NOT VERIFIED). clippy and static: rc 0 (job `wctype-trie-builtin-gen`).

## Counts (instructions per call, wcprobe `count`, Zen 1 build host, median over the 12 classes; before / after / glibc)
Before: keke:`wctype-trie-builtin-count-before` rc 0 (origin/next `70cb44d76`; outputs ~/work/WCTYPE-TRIE-BUILTIN/before/). After: keke:`wctype-trie-builtin-count-after` starts when the gate ends (chain ~/work/WCTYPE-TRIE-BUILTIN/chain.sh) -> after/. **NOT VERIFIED: not yet compared.** Compare with
`python3 ~/work/WCTYPE-TRIE-BUILTIN/cmp.py ~/work/WCTYPE-TRIE-BUILTIN/{before,after}/count-liella.tsv ~/work/WCTYPE-TRIE-BUILTIN/before/count-glibc.tsv`
(prints before/after/glibc instructions per locale, entry and range, and the worst after-before delta), and the cbench_locale tables at the end of {before,after}/bench.log. Expected (rethink section 4, design A): C and C.UTF-8 isw*/iswctype 13-22 instructions, 4 loads, every range; en_US rows unchanged (file-backed path untouched; `LocaleView` gained one field).

## Open / next
* Timing on Ivy Bridge and the N100 with handoff/wctype-rethink/trun.sh: PENDING (timing hosts down from 11-07 20:00 UTC
  until about midweek). Counts are the evidence here.
* Next stories (rethink section 8): TOWCTRANS-DESC, WCTYPE-TRIE-FILE (D-NEW-2/3: file-backed lazily resolved stage 1).
* Possible follow-up: `ctype::wclasses` for C/C.UTF-8 could read the same tables (non-view callers); kept as the oracle here.
* Do not redo: the generator, leaf dedup, the view field. Plants file is reusable for the file-backed story.
