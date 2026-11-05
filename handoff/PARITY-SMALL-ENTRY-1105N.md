# PARITY-SMALL-ENTRY-1105N: small-call entry cost (ctype_b_loc, isalpha, nl_langinfo, strnlen)

State: branch `task/parity-small-entry-1105n` (from origin/next ff125cce), role mei:~/w/s1. Code head be808ed
(handoff commits follow). Budget-limited run (70 calls): ONE of the four rows was changed (nl_langinfo /
nl_langinfo_l); strnlen, isalpha and ctype_b_loc are profiled below but left unchanged.

## The change
* `validation/percall/small_entry.c` (new): per-call cases ctype_b_loc, isalpha_c (the real function through a
  pointer, varying char), nl_langinfo_mon, nl_langinfo_l_mon and nl_langinfo_l_codeset (newlocale object),
  strnlen_16, strnlen_16_mix. The brief's small-probe.c was not found in the brief directory, the scratchpad or
  liella-ops, so this probe was written from the brief's case list.
* `crates/core/src/locale/view.rs`: every view built by `LocaleView::new` keeps `answers`, the finished
  `nl_langinfo` word of every item of every category (word value, item address, or `EMPTY` for an empty item), in
  one leaked allocation (one `usize` per item, built once per interned view). `LocaleView::answer(number, index)`
  reads it. The const `"C"` view has no table (empty slices) and keeps the old `Tab`/`Item` decode.
* `crates/boundary/src/locale.rs`: `langinfo` tries `v.answer` first, then the old path. The empty-string static
  moved to core (`liella::locale::view::EMPTY`) so both paths answer an empty item with the same address.
* Test: `view.rs` `answer_tests::answer_table_matches_entries` (no spec clause; table equals the decode for every
  category number 0-14 and index 0-1099 of a built "C" view, past-the-end included). en_US items are compared with
  glibc by the existing `locale_products` test (the plants' cmd).

## Profile (instructions:u per call incl. loop, en_US.UTF-8, percall-before, Skylake-SP host)
| case | glibc | Liella before | where the surplus is |
|---|---|---|---|
| nl_langinfo_l_mon | 35 | 49 | view_of null/alignment test (5), Tab tag + Item decode (~8 branches/loads) |
| nl_langinfo_l_codeset | 28 | 42 | same |
| nl_langinfo_mon | 38 | 42 | Tab tag + Item decode |
| strnlen_16 / _mix | 28 / 33 | 33 / 38 | Rust entry `test rsi; je; test rdi; je; jmp liella_machine_strnlen` before the scanner |
| isalpha_c (function) | 19 | 26 | range test (3) + a null test of the computed entry address (`add; je`, from conv::mapped_bytes) |
| ctype_b_loc | 14 | 16 | the "words stored" test (`test; je`) |

## Results
Counts before (cbench, instr ratio): ctype_b_loc 1.20 (time 0.84), isalpha_c 1.44 (time 1.00), nl_langinfo_en_us
1.13, nl_langinfo_l_en_us 1.57. After: see job `parity-small-entry-1105n-counts-after` and
`parity-small-entry-1105n-percall-after` (started detached at hand-back; NOT VERIFIED in this run). Expected:
nl_langinfo_l about -8 to -10 per call; any row where it measures worse must be reverted (developer-flow).
Lint (cargo xtask clippy + static) rc 0 on be808ed. Gate and plants were started detached; see the hand-back.

## Next step (things not to redo: the profile above)
1. Read the after jobs on mei: `ssh mei '~/bin/jobwait parity-small-entry-1105n-gate parity-small-entry-1105n-plants
   parity-small-entry-1105n-percall-after parity-small-entry-1105n-counts-after'`; check no cbench_locale row
   (setlocale/newlocale rows especially: a new view now builds its answer table) is slower.
2. strnlen: a direct asm entry `strnlen` in csys/src/machine/scan.S (as rawmemchr's SCAN-TAIL entry) that tests
   only NULL (tail to a Rust rest entry for the abort) and falls into liella_machine_strnlen, whose small path
   already handles max = 0: -3 per call (33 -> 30 vs 28, 1.07). The selftest settle check must stay on the
   Rust path.
3. isalpha family: read the class word without the null test of the entry address (a crate-private
   non-null read in locale.rs): -2 per call. ctype_b_loc (2 instructions, time 0.84): record in the
   saturation register, not chase (brief).
