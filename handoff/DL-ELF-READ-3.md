# DL-ELF-READ-3 (phase 14 DL-3, IP-24 part 2, continuation of DL-ELF-READ-2)

**State:** branch `task/dl-elf-read-3` (DL-ELF-READ-2's commits from origin/next `9424819f`, then this run's).
Role keke:~/w/n1. Part 2 of the ELF reader is complete: code, tests, readelf parity, plants, fuzz target.
Supersedes `handoff/DL-ELF-READ-2.md` (kept for its design notes: arch seam checklist, probe-settled symbol count).

## This run
1. DL-ELF-READ-2's jobs: gate rc 0; plants rc 1, 28 CAUGHT / 3 SURVIVED. Causes:
   * `gnu-symoff`: the test's bad bucket was also the highest one, so the count walk failed on it anyway.
     `gnu_chain_out_of_range` now adds a two-bucket table with the low bucket second.
   * `from-end`: `Elf::bytes_from` was only tested with one PT_LOAD. New test
     `addresses_resolve_through_the_segment_holding_them` (second PT_LOAD above the first; bytes_from and bytes_at).
   * `sysv-index`: equivalent by construction (SysvHash bytes end at nchain, so the word lookup gives the same
     `HashIndex`); recorded as `equivalent:` with the reason in `handoff/DL-ELF-READ-3-plants.txt`.
2. Fuzz target `elf` (`fuzz/fuzz_targets/elf.rs`): parse::<X86_64>, then walk program headers, segment bytes,
   interp, notes/properties, dynamic, needed, both hashes (4 chains each), versym/verdef/verneed, RELA/JMPREL/RELR,
   each walk capped at 4096 items. `fuzz/Cargo.toml` (`[[bin]] elf`, `liella-ld` dep), `xtask/src/fuzz.rs`
   (`elf`: max_len 64 KiB, timeout 10 s, 2048 MB, chunk 30), docs/fuzzing.md row + seed description.
   Seeds `fuzz/seeds/elf/` (6 stripped `-nostdlib` objects; sources and build script were scratch only:
   versioned lib + needer with both/SysV/GNU hash, RELR, TLS, PLT, a no-notes lib, a PIE with PT_INTERP).
   `cargo xtask fuzz-check`: 27 targets type-check (job dl-elf-read-3-t1).
3. Fuzz run: `LIELLA_FUZZ_CORPUS=/dev/shm/claude/DL-ELF-READ-3/fuzz-1 cargo xtask fuzz elf --time 5` (job
   keke:dl-elf-read-3-fuzz, rc 0): 18,517,651 executions, cov 478, ft 1456, 581 corpus inputs, **zero crashes**
   (artifacts dir empty), 61k exec/s on the Zen 1 host at load ~28/60.
4. Coordinator ruling applied: plan/ph14/design.md now says `TextRel(width, pc_relative, signed)`. D-27.8 names
   the fuzz target.

## Jobs (keke)
dl-elf-read-3-t1 (elf_tables 17 passed + fuzz-check), dl-elf-read-3-fuzz (5 min), dl-elf-read-3-gate
(clippy, static, affected on the final head), dl-elf-read-3-plants (31 plants, `handoff/DL-ELF-READ-3-plants.txt`).
Results: `handoff/reports/DL-ELF-READ-3.json` and the hand-back.

## Decisions in force
D-27.7 (part 1), D-27.8 (part 2 error rules; owned by this story). No rulings open.

## Informational
* `fuzz/Cargo.lock` (tracked) is stale on origin/next: it lacks `liella-boundary` though `fuzz/Cargo.toml` names it;
  the fuzz build regenerates it locally. Not touched here (not this story's).
* The nightly fuzz build warns `unused import AcqRel` in `crates/boundary/src/timer.rs:93` (not this story's).

## Next step
None for part 2. Phase 14's next ELF consumer (symbol lookup over these tables) reads D-27.8 and the
`liella_ld::elf` docs; the fuzz target should gain lookups when that API exists.
