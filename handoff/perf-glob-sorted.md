# PERF-GLOB-SORTED (partial)

Branch task/perf-glob-sorted. Change: crates/core/src/pattern/glob.rs `sort_found` sorts (8 key bytes past the shared prefix, index)
pairs when `Locale::collates_by_bytes()` (C, C.UTF-8); other locales keep the collation compare. Test: `glob_sort_key_ties`, called from s14_8_c_3.

Results (keke, instructions:u per call, parity1105 glob_sorted_38): glibc 68437.0, base 85734.8, new 76509.9 (1.12x; target 1.05x NOT met).
Note: that run's locale was LC_ALL=en_US.UTF-8; if the case sorts in a rules locale the key path is not what moved the number - not checked.
Bench glob_1000_sorted (cargo xtask bench --compare --counters) NOT run (budget).
Gate (perf-glob-sorted-gate2): ok. Clippy workspace and static clean on the code commits.
Plants (perf-glob-sorted-plants2, exit 1): CAUGHT reversed-key, tie-ignored, little-endian; SURVIVED lcp-last-pair (test names share no
prefix beyond the directory: add names whose first and last share more) and key-off-by-lcp (EQUIVALENT in result: only a speed difference).
Superseded: perf-glob-sorted-gate, -plants (test lacked a directory), -percall, -percall2, (spec path / base not primed).
Next: confirm which locale glob_sorted_38 uses; profile the remaining ~210 instr/entry (readdir, fnmatch, path building: overlaps PERF-NFTW-ENTRY);
add a test for lcp-last-pair; run the bench rows. The percall spec is not in the repo (copied from ~/work/parity-rows-1105/parity1105.c).
