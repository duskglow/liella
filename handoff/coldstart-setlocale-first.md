# COLDSTART-SETLOCALE-FIRST(-2): first `setlocale(LC_ALL, "en_US.UTF-8")`

State: branch `task/coldstart-setlocale-first-2` (stacks on `task/coldstart-setlocale-first`), cut from origin/next e8189dd8.
**No product code change on the branch**: the one change tried was reverted (it measured worse). The branch adds tests, a
plants file, a NEGATIVE saturation row and this handoff. The row is still about 2.1x on the N100 (probe deltas at 730f416b).

## Decisions in force
- Ruling #3348: the `CtypeFile` table checks stay at load (a corrupt `LC_CTYPE` is rejected at setlocale). Tested by
  `locale_data::ctype_parse_rejects_a_corrupt_file_at_load`.
- Ruling #3347 (conditional): static first-view storage only if it adds no second ownership/lifetime path. **Not done in -2**
  (budget). Analysis below.

## -2 run: validate-only `codeset_matches` (NEGATIVE, reverted)
`codeset_matches` ran the whole `CtypeFile::parse`, including `ascii_case_runs`. Three variants made selection validate-only
(case flag derived once). Probe on mei (Skylake-SP class; role `~/w/s2`; commit 998e2029's message says "Zen 1 host" by mistake), static probe, `perf stat -r 21`,
interleaved, `S` variant (faults / user instr / kernel instr):

| build | pf | iu | ik |
|---|---:|---:|---:|
| before (e8189dd8) | 57 | 65120 | 587-592k |
| split, separate `check` fn | 59 | 64370 | 613k |
| one body + flag, `#[inline(never)]` | 58 | 64308 | 601-609k |
| one body + flag, `#[inline]` | 58 | 64270 | 600-607k |
| glibc (same probe) S / _ | 65-66 / 53 | 146498 / 124131 | 750k / 625k |

Every variant saves about 811 user instructions (-1.2%) but adds one text page fault. `perf record -d` shows a new instruction-
fetch fault around 0x4bd000 (code layout shift in the 3.4 MB text). That costs +12k to +14k kernel instructions, so the row
measured worse and the change was reverted (owner rule). Faults from layout are fragile: a later change could flip it back. Not
worth retrying alone, but worth folding into the part 1 change and measuring as a whole.

## Part 1 analysis (ruling #3347), not implemented
`global_locale.rs` interns every view, name and table and leaks it (`TryBox::leak`, `TryVec::leak`). Nothing is ever freed, so a
static slot for the first view keeps the same `'static` lifetime: freelocale, uselocale and duplocale(LC_GLOBAL_LOCALE) work on
handles (indices), fork copies `.bss` like heap, and publication stays under `LOCK` with Release stores. What it does add is a
**second storage source**: `LocaleView::new` allocates `TryVec<Item>` (all categories' items) and `TryBox<Tables>`, and
`intern_name_locked` allocates names. A `.bss` bump arena handing out `&'static mut` from a static needs new `unsafe` in
`liella-boundary` (a new category, so a ruling). Recommendation: a one-shot arena type in the boundary crate (claim flag,
heap fallback when taken or too small), used by `intern_locked`, `LocaleView::new` and `intern_name_locked` for the first view.

## Tests added
- `conformance/c/s07_setlocale_first_case.c`: first setlocale then first use of toupper/tolower/isalpha/towupper/towlower/
  iswalpha, MB_CUR_MAX, CODESET, then C, en_US again, LC_CTYPE=C composite (names and answers). It passes on glibc 2.39 on mei.
  SKIPPED when `locale -a` lacks en_US.UTF-8.
- `locale_data::ctype_parse_rejects_a_corrupt_file_at_load` (magic, width Table3 shift, class name).

## Probe (committed recipe; sources in `~/work/COLDSTART-SETLOCALE-FIRST-2/` on mei)
`cs.c`: argv[1] contains `S` = `setlocale(LC_ALL, "en_US.UTF-8")`, `T` = case functions (`ST` = both). `meas.sh <role> <out> <tag>`
builds `cargo xtask so` (cargo by full path), links `gcc -O2 -static -nostdlib -I target/liella/include
target/liella/liella-crt1.o cs.c target/liella/libc.a -lgcc`, and counts `_ S ST` for Liella and glibc (median of 11).

## Jobs (mei)
counts-before, counts-after, counts-after2, counts-after3 (evidence; superseded by the revert), gate
`coldstart-setlocale-first-2-gate` (direct, --base origin/next), plants `coldstart-setlocale-first-2-plants`. No timing job: no code
change is left to time.

## Next step (exact)
1. Part 1 under ruling #3347 with the boundary arena (needs the unsafe-category ruling first), counted with `meas.sh`
   (`S`, `ST`). Include the validate-only `codeset_matches` and judge both together on faults and kernel instructions.
2. Then N100 timing of `first_setlocale_en_US` and `second_setlocale` with `~/bin/timing` called directly on nozomi.

## Things not to redo
- Step 0 (row about 2.1x on the N100 at 730f416b). The validate-only `codeset_matches` on its own (above).
- perf sampling throttles on this probe; use `perf stat -r 21` and `perf record -d -e page-faults -c 1`.
