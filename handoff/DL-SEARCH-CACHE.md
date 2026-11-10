# DL-SEARCH-CACHE (phase 14 DL-8): search order, DSTs, cache reader, hwcaps, `--list`

**State:** branch `task/dl-search-cache` (on DL-MAP 908f61a17; origin/next is NOT merged in). Commits a6e30fbf3
(the change), 1fd73d147 (clippy fixes, plants), then this handoff. Role keke:~/w/n1. Stopped at the context
checkpoint (~150k): jobs started, results below are what was seen at hand-back.
Decisions in force: D-27.16 (new, `records/decisions/D-27/D-27.16.toml`), D-27.15, D-27.14, R14-3 (a), R14-15,
R14-18, #940, #941.

## Done
* `liella_ld::cache` (new format only, #941): checked parse (magic, counts, string table, extension directory,
  hwcaps names), binary search under the Appendix D.3.3 numeric comparison, run of equal keys: wrong
  `Arch::CACHE_FLAGS` skipped, best supported hwcaps entry before the first plain entry, else the plain one (#940).
* `liella_ld::search` (pure, no allocation): pinned sonames -> `<prefix>/lib` (LD_LIBRARY_PATH first outside secure
  mode only); RPATH chain / LD_LIBRARY_PATH / RUNPATH / cache / defaults; hwcaps subdirectories per directory;
  `$ORIGIN`/`$LIB`/`$PLATFORM`; secure rules 1-3 and 5. Compile-time `LIELLA_PREFIX` (default `/`).
* Arch seam: `LIB`, `SYSTEM_DIRS`, `SYSTEM_TRUSTED`, `CACHE_FLAGS`, `hwcaps(&Features)`, `platform(&Features)`.
* `boundary/src/ld.rs`: `ld.so --list PROG`, `LD_TRACE_LOADED_OBJECTS` (kernel start and `ld.so PROG`; ignored in
  secure mode): BFS over DT_NEEDED, each found object mapped by `map::load`, ldd-style lines, 127 on not found.
  Running a program with dependencies stays refused (DL-10).
* Tests `crates/ld/tests/search_cache.rs` (7); fuzz target `ld_cache` (docs/fuzzing.md row; not run under libFuzzer).
* Plants `handoff/DL-SEARCH-CACHE-plants.txt` (6): the brief's four plus best-hwcaps and string-table bound.

## Arch seam checklist
1 no new page/machine literal outside arch.rs; 2-4 n/a; 5 hwcaps only from `Arch::hwcaps(&Features)` of
`Arch::features`; 6 paths, `$LIB`, `$PLATFORM`, cache flag from the table (the `/lib` prefix dir is the R14-3
prefix, not arch); 7 pure tests use `Native` and say so, the product test is cfg x86_64; 8 rows added (above); 9 n/a.

## Open / next (see handoff/DL-SEARCH-CACHE-report.md)
* RULING NEEDED: the reference's default directories (its `--help`: `/lib/x86_64-linux-gnu`,
  `/usr/lib/x86_64-linux-gnu`, `/lib`, `/usr/lib`) differ from threat model rule 5's trusted list (`/lib64`,
  `/usr/lib64` + multiarch pair). Implemented: search the probed list, trust rule 5's list as approved. Recommend:
  amend rule 5 to the probed list (trust = search defaults).
* RULING NEEDED: the default `LIELLA_PREFIX` (`/` here) and the pinned directory `<prefix>/lib` = `/lib` for it
  (Ubuntu's libc is not there; tests set LD_LIBRARY_PATH). Recommend `/` default, pinned dir from the table for `/`.
* Not done: product test for rule 11 (set-group-ID program with LD_TRACE_LOADED_OBJECTS; pure-logic check only),
  DF_1_NODEFLIB, `$PLATFORM` on non-Haswell CPUs (unprobed), libFuzzer run of `ld_cache`, `--library-path`,
  `--inhibit-cache`, startup counters vs origin/next (job started, compare not reviewed).
* Next: read the three jobs (report), fix what is red, then DL-10 relocates the mapped dependencies.
