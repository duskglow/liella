# DL-SEARCH-CACHE (phase 14 DL-8): search order, DSTs, cache reader, hwcaps, `--list`

**State:** branch `task/dl-search-cache-4` (on `task/dl-search-cache-3`). Run 4 merged origin/next (DL-RELOC-3) and
resolved the conflict: ee0bce0b5 (merge; arch.rs), 0cc76bf59 (list-mode rename in boundary ld.rs), 03500123e
(codemap regenerated, run 4 plants file), then the handoff/report commit. Role kanon:~/w/kn1.
Full report of run 4: handoff/DL-SEARCH-CACHE-4-report.md (run 3: handoff/DL-SEARCH-CACHE-3-report.md, run 2:
handoff/DL-SEARCH-CACHE-2-report.md, run 1: handoff/DL-SEARCH-CACHE-report.md).
Decisions in force: D-27.16, D-27.15, D-27.14, D-27.17 (DL-RELOC, on next), R14-3 (a), R14-15, R14-18, #940, #941,
#4237 (threat model rule 5: trusted for prefix `/` = `Arch::SYSTEM_DIRS`; `/lib64`, `/usr/lib64` dropped; in this
branch), #4238 (prefix `/`; its pinned dir comes from the arch table in the install story), #4286 (rule-11 plant runs
where the set-group-ID child can run).

## Done (runs 1-4)
* `liella_ld::cache` (new format only): checked parse, Appendix D.3.3 binary search, arch flag, best hwcaps (#940);
  an entry whose path does not start with `/` is inconsistent; a string offset below the string table is refused.
* `liella_ld::search`: pinned sonames, RPATH/LD_LIBRARY_PATH/RUNPATH/cache/defaults, hwcaps subdirectories,
  `$ORIGIN`/`$LIB`/`$PLATFORM`, secure rules 1-3, 5 (#4237).
* `boundary/src/ld.rs` `--list` / `LD_TRACE_LOADED_OBJECTS` (probed: `--list` with a missing dependency = loader
  error, 127; tracing = `not found` line, 0); rule 11 (no listing in secure mode).
* Run 4: the `Arch` seam holds both stories' items (this story's LIB, SYSTEM_DIRS, CACHE_FLAGS, hwcaps, platform;
  DL-RELOC's apply, PROPERTIES). In boundary ld.rs the list mode's limit and opener are `LIST_OBJECTS_MAX` (256,
  D-27.16) and `open_whole`; DL-RELOC's run path keeps `OBJECTS_MAX` (64) and `open_file` (with file identity).
* Tests: `crates/ld/tests/search_cache.rs` (7); rule 11 product test in `ldso_run.rs`.
* Plants: `handoff/DL-SEARCH-CACHE-4-plants.txt` (the same 10 as run 3). Run it with `--dir` on a file system
  WITHOUT nosuid (kanon: /mnt/zram; keke: /mnt/nvme; /dev/shm is nosuid, so the secure child would SKIP).

## Things not to redo
* Gate: `fuzz/Cargo.toml` escalates `affected` to FULL (its `Cargo.toml` rule precedes the `fuzz/` rule). Gate with
  `--tier direct --files <every changed file but fuzz/Cargo.toml and handoff/>`; the fuzz target is built by
  `fuzz-check` (part of `cargo xtask static`). Script used in run 4: kanon ~/work/DL-SEARCH-CACHE-4/gate.sh.

## Open / next
* The run path (`link`, DL-RELOC) still opens `DT_NEEDED` names only by path ("the library search is not available
  in this build"); wiring `liella_ld::search` and the cache into it is the next loader step. Then the two openers in
  boundary ld.rs can become one (keep `--list` printing "not a dynamic executable" for a file shorter than an ELF
  header: `open_file` refuses such a file, `open_whole` does not).
* Gaps (records only, D-27.16): DF_1_NODEFLIB, `--library-path`/`--inhibit-cache`, `$PLATFORM` beyond Haswell
  (unprobed), `ld_cache` under libFuzzer.
