# UTIL-CACHE-WRITER (UTIL-2): the ld.so cache writer in liella-ld

**State.** Branch `task/util-cache-writer-2` (continues `task/util-cache-writer`, from origin/next
`fa251fbb0`). Code head `1a1c96692`; the commits after it hold only handoff/ files. COMPLETE pending the
coordinator's id for the decision record.

**UTIL-CACHE-WRITER-2 (finish only, no code change).** Run 1's verify went red because its fuzz job ran
before the last commits to `crates/ld/tests/cache_write.rs` and `fuzz/seeds/ld_cache/writer-*`. Run 2
reran every job on the final tree on mei:~/w/s2 under new labels (table "Results, run 2" below).

**What landed on the branch.**
* `crates/ld/src/cache/writer.rs`: `cache::write(entries, generator, out)`, `cache::encoded_len`,
  `cache::order` (the writer's total order), `cache::Entry`, `cache::WriteError`, the bounds
  `NAME_MAX` 255, `PATH_MAX` 4095, `HWCAPS_MAX` 64, `GENERATOR_MAX` 4096, `ENTRIES_MAX` 2^20.
  It allocates nothing: the caller's slice is sorted in place and the file is written into the caller's buffer.
* `crates/ld/src/cache.rs`: shares `MAGIC`, `HEADER`, `ENTRY`, `EXT_MAGIC`, `TAG_HWCAPS`,
  `HWCAP_INDEX` and `compare` with the writer. New: `HEADER_FLAGS`, `TAG_GENERATOR`,
  `Cache::get(i)` returning `Stored` (an entry with its hwcaps name), and `Cache::generator()`.
  `parse` and `lookup` are unchanged.
* `crates/ld/tests/cache_write.rs`: 5 tests, plus 1 ignored test that regenerates `fuzz/seeds/ld_cache`
  (it is a NOT_SWEEPS row in `xtask/src/longsweeps.rs`).
* `fuzz/fuzz_targets/ld_cache_write.rs`: a new target that runs the writer, then the reader.
  `fuzz/seeds/ld_cache/writer-0..4` are 5 seeds the writer made for the reader target.
* `records/pending/UTIL-CACHE-WRITER-1.toml`: a D-NEW record in series D-27 (the loader cache series;
  D-27.16 is the reader). It records the order, layout, bounds and the probe result below.
  The coordinator assigns the id.

**Decisions in force.** D-27.16 (reader), the pending record above, owner #941 (the loader reads the new format only).

**Probe-settled.**
* The reference `ldconfig -r ROOT` runs unprivileged.
* The reference orders plain entries of one directory whose keys compare equal (`libv.so.1.9`
  and `libv.so.1.09`) by directory listing. Two runs over the same files gave opposite orders on
  ext4 and on zram (spec D.3.3 says "not settled").
* Liella orders them by path bytes, which is one of the outcomes the reference can produce. The
  black-box tests compare that one group as a set and pin Liella's order.

**Results, run 2 (these prove the final head).** mei:~/w/s2, tree = code head 1a1c96692 plus handoff/.
| Job | Label | Result |
|---|---|---|
| Clippy (workspace, all features) + `xtask clippy` + `xtask static` | `util-cache-writer-2-lint` | green |
| Gate (direct tier) | `util-cache-writer-2-gate` | 5 suites, 460 passed, 0 failed, 1 ignored, peak 1.9G |
| Plants | `util-cache-writer-2-plants` | 13 of 13 CAUGHT, 0 WARN |
| Fuzz | `util-cache-writer-2-fuzz` | `ld_cache_write` 6 min: 1.26M runs, 622 edges; then `ld_cache` 5 min with the seeds: 43M runs, 122 edges; no finding |

Both fuzz corpora started empty (`/mnt/zram/claude/UTIL-CACHE-WRITER-2/fuzz`), so `ld_cache` reaching 122
edges comes from the committed writer seeds.

**Results, run 1 (superseded: the fuzz job predates the final test and seed commits).** All jobs ran on
shiki:~/w/sh1.
| Job | Label | Result |
|---|---|---|
| Gate | `util-cache-writer-gate` | 5 suites, 460 passed, 0 failed, 0 SKIPPED, peak 1.9G |
| Plants | `util-cache-writer-plants` | 13 of 13 CAUGHT, 0 WARN |
| Static | `util-cache-writer-static3` | green |
| Clippy | `util-cache-writer-clippy2` | green |
| Fuzz (writer) | `util-cache-writer-fuzz` | `ld_cache_write` 6 min: 0.9M runs, 622 edges, no finding |
| Fuzz (reader) | `util-cache-writer-fuzz2` | `ld_cache` 5 min with the new seeds: 36M runs, 110 edges (11 without seeds), no finding |

Superseded runs: `-dev1` to `-dev5` (development); `-static`, `-static2` (record pipe and trace
marker, fixed); `-clippy` (an older head).

**Percall: not applicable.** The change adds no instructions to any benchmarked path. The loader's
`parse` and `lookup` are byte-for-byte unchanged; the new code runs only from `cache::write` and
the two new accessors.

**Open questions.** None for the owner. The record's id is for the coordinator to assign.

**Not to redo.**
* Byte equality with reference-written caches is not a goal. The record documents the string layout.
* Without seeds, the reader fuzz target never gets past the 20-byte magic.

**Next step: UTIL-3 (ldconfig).**
* Build `Entry` rows from the configuration and the directory scan: `order` = the directory's
  position, then the trusted directories.
* Call `encoded_len`, then `write` into a buffer, then replace the cache atomically.
* `ldconfig -p` can use `Cache::get` and `Cache::generator`.

Read: plan/utilities.md §4 to §6 (origin/task/liella-utilities-design), spec Appendix D.3, D-27.16.
