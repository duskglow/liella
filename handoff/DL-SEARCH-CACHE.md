# DL-SEARCH-CACHE (phase 14 DL-8): search order, DSTs, cache reader, hwcaps, `--list`

**State:** branch `task/dl-search-cache-2` (origin/next merged in, ruling #4253); run 2 commits cd69677d2,
08682cde0, be837bee6, d785bae06, then this handoff. Role keke:~/w/n1. Full report of run 2:
handoff/DL-SEARCH-CACHE-2-report.md (run 1: handoff/DL-SEARCH-CACHE-report.md).
Decisions in force: D-27.16 (corrected in run 2), D-27.15, D-27.14, R14-3 (a), R14-15, R14-18, #940, #941,
#4237 (threat model rule 5 amended), #4238 (prefix `/`; its pinned dir comes from the arch table in the install story).

## Done (runs 1 and 2)
* `liella_ld::cache` (new format only): checked parse, Appendix D.3.3 binary search, arch flag, best hwcaps (#940);
  run 2: an entry whose path does not start with `/` is inconsistent (the corpus found an empty path).
* `liella_ld::search`: pinned sonames, RPATH/LD_LIBRARY_PATH/RUNPATH/cache/defaults, hwcaps subdirectories,
  `$ORIGIN`/`$LIB`/`$PLATFORM`, secure rules 1-3, 5. Run 2 (#4237): trusted for prefix `/` = exactly
  `Arch::SYSTEM_DIRS` (`SYSTEM_TRUSTED` removed; `/lib64`, `/usr/lib64` not trusted); threat-model.md rule 5 amended.
* `boundary/src/ld.rs` `--list` / `LD_TRACE_LOADED_OBJECTS`. Run 2 (probed): `--list` with a missing dependency =
  the loader error on stderr, empty stdout, 127; tracing = `not found` line, status 0. Lines written after the walk.
* Tests: `crates/ld/tests/search_cache.rs` (7; lints, clause IDs, ARCH tag fixed; the `--list` test linked with
  `--no-as-needed`, which the host toolchain's default dropped); rule 11 product test in `ldso_run.rs`
  (set-group-ID child with `LD_TRACE_LOADED_OBJECTS=1` runs, no listing).
* Plants: `handoff/DL-SEARCH-CACHE-2-plants.txt` (10). Codemap regenerated.

## Arch seam checklist
Unchanged from run 1 (all ticked); run 2 removed one seam constant (`SYSTEM_TRUSTED`), trust comes from `SYSTEM_DIRS`.

## Open / next
* Gaps: DF_1_NODEFLIB, `$PLATFORM` beyond Haswell (unprobed), `--library-path`/`--inhibit-cache`, `ld_cache`
  under libFuzzer, cbench_startup counts vs origin/next (run 1 has Liella-vs-glibc counts only; the normal start
  path gained one `bool` test).
* Next: read the jobs in the report; then DL-10 relocates the mapped dependencies.
