# DL-SEARCH-CACHE (phase 14 DL-8): search order, DSTs, cache reader, hwcaps, `--list`

**State:** branch `task/dl-search-cache-3` (on `task/dl-search-cache-2`, which merged origin/next per ruling #4253);
run 3 commits 19a1d91fe (corpus string-table check, run 3 plants file), then this handoff. Role keke:~/w/n4.
Full report of run 3: handoff/DL-SEARCH-CACHE-3-report.md (run 2: handoff/DL-SEARCH-CACHE-2-report.md, run 1:
handoff/DL-SEARCH-CACHE-report.md).
Decisions in force: D-27.16, D-27.15, D-27.14, R14-3 (a), R14-15, R14-18, #940, #941,
#4237 (threat model rule 5: trusted for prefix `/` = `Arch::SYSTEM_DIRS`; `/lib64`, `/usr/lib64` dropped; in this
branch), #4238 (prefix `/`; its pinned dir comes from the arch table in the install story), #4286 (rule-11 plant runs
where the set-group-ID child can run).

## Done (runs 1-3)
* `liella_ld::cache` (new format only): checked parse, Appendix D.3.3 binary search, arch flag, best hwcaps (#940);
  an entry whose path does not start with `/` is inconsistent; a string offset below the string table is refused.
* `liella_ld::search`: pinned sonames, RPATH/LD_LIBRARY_PATH/RUNPATH/cache/defaults, hwcaps subdirectories,
  `$ORIGIN`/`$LIB`/`$PLATFORM`, secure rules 1-3, 5 (#4237).
* `boundary/src/ld.rs` `--list` / `LD_TRACE_LOADED_OBJECTS` (probed: `--list` with a missing dependency = loader
  error, 127; tracing = `not found` line, 0); rule 11 (no listing in secure mode).
* Tests: `crates/ld/tests/search_cache.rs` (7; run 3: the corpus probe checks every returned path lies inside the
  declared string table, plus an entry-table offset case); rule 11 product test in `ldso_run.rs`.
* Plants: `handoff/DL-SEARCH-CACHE-3-plants.txt` (10, incl. the rule-11 plant). Run it with `--dir` on a file
  system WITHOUT nosuid (keke: under /mnt/nvme/claude/; /dev/shm is nosuid, so the secure child would SKIP).

## Things not to redo
* Gate: `fuzz/Cargo.toml` escalates `affected` to FULL (its `Cargo.toml` rule precedes the `fuzz/` rule). Run 3 gated
  with `--tier direct --files <every changed file but fuzz/Cargo.toml>`; the fuzz target is built by `fuzz-check`
  (selected by `fuzz/fuzz_targets/ld_cache.rs`, and part of `cargo xtask static`).

## Open / next
* Gaps (records only, D-27.16): DF_1_NODEFLIB, `--library-path`/`--inhibit-cache`, `$PLATFORM` beyond Haswell
  (unprobed), `ld_cache` under libFuzzer.
* Next: DL-10 relocates the mapped dependencies.
