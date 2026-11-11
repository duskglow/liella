# DL-SEARCH-CACHE-4 report (phase 14 DL-8, continuation 4)

Branch `task/dl-search-cache-4` (on `task/dl-search-cache-3` 61652fa88), role kanon:~/w/kn1.

## Why run 3 went red
CI merge conflict only: DL-RELOC-3 (landed on next) and this branch both added items to the `Arch` trait in
`crates/ld/src/arch.rs` at the same place. After the textual merge a second, semantic conflict showed in the first
clippy (`dl-search-cache-4-lint`, rc 1, E0428/E0631): DL-RELOC's run path in `crates/boundary/src/ld.rs` brought its
own `OBJECTS_MAX` (64) and `open_file(path: usize)`, and this branch's `--list` walk had items of the same names
(256 per D-27.16, `open_file(path: &[u8])`).

## Commits (run 4)
* ee0bce0b5 merge origin/next: arch.rs keeps both stories' trait items (LIB, SYSTEM_DIRS, CACHE_FLAGS, hwcaps,
  platform; then apply, PROPERTIES), one closing brace. The test doubles in elf_reader.rs / elf_tables.rs already
  implement this branch's items; DL-RELOC's items have defaults.
* 0cc76bf59 the list mode's pair becomes `LIST_OBJECTS_MAX` and `open_whole`; no behaviour change on either path.
* 03500123e `cargo xtask codemap` (docs/codemap/README.md: line numbers moved by the merge; static's codemap check
  failed in `dl-search-cache-4-lint2` before it); `handoff/DL-SEARCH-CACHE-4-plants.txt`.
* then this handoff/report commit (handoff/ only).

## Jobs (kanon, all on the code of 03500123e)
* `dl-search-cache-4-gate`: `~/bin/gate 16G sh ~/work/DL-SEARCH-CACHE-4/gate.sh` = clippy, static, so,
  `affected --run --base origin/next --tier direct --files <17 changed files but fuzz/Cargo.toml and handoff/>`.
  Selection: 248 of 255 test targets, 127 of 127 conformance programs (`crates/ld/src/lib.rs` is a crate root, wide).
  Result rc 0: suites 15 ok, 0 failed; tests 12101 passed, 0 failed, 1349 ignored; conformance 122 passed, 0
  failed; 148 SKIPPED lines (absent tools/capabilities, as on other gates); peak 16G (page cache included), oom_kill 0.
* `dl-search-cache-4-plants` rc 0: `cargo xtask plants handoff/DL-SEARCH-CACHE-4-plants.txt --dir
  /mnt/zram/claude/DL-SEARCH-CACHE-4/plants-1 --timeout 900`: 10 plants, 10 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY,
  0 WARN-NO-MSG. /mnt/zram has no nosuid, so the rule-11 plant (`secure-traces-loaded-objects`) ran its
  set-group-ID child and was caught by `ldso_run_secure_child_sees_unsecure_cleared`; `list-missing-not-fatal` caught
  by `list_matches_reference`.
* No counts this run: the code change is a merge plus a rename; run 3's cbench_startup parity (statically linked
  rows, loader not on their path) is unaffected.
* Superseded: `dl-search-cache-4-lint` rc 1 (merge head before the rename: E0428), `dl-search-cache-4-lint2` rc 1
  (clippy clean; static codemap out of date), `dl-search-cache-4-codemap` rc 0 (generator run, not a proof job).

## INFORMATIONAL
* The two openers in boundary ld.rs should become one once the run path uses the search (next loader step); keep
  `--list`'s "not a dynamic executable" for a file shorter than an ELF header when merging them.
* `cargo xtask affected` still sends `fuzz/Cargo.toml` to FULL (run 3's note); this run gated with `--files` as run 3.
* Gaps (records only, D-27.16): DF_1_NODEFLIB, `--library-path`/`--inhibit-cache`, `$PLATFORM` beyond Haswell,
  `ld_cache` under libFuzzer.

## Refused
None.
