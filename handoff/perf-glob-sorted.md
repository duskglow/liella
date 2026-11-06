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

## PERF-GLOB-SORTED-2 (continuation, mei role s1)
- Plant lcp-last-pair now CAUGHT: glob_sort_key_ties has a second tree (`w/`) whose last entry ends the shared prefix and sorts after
  the others (fails at s14_search_clauses.rs "last entry shortens the shared prefix"). Plant key-off-by-lcp is EQUIVALENT by design:
  taking the key from byte 0 gives the same order, only more tie-breaks (a speed difference, no result difference).
- Jobs (mei): perf-glob-sorted-2-plants (4 plants CAUGHT, rc 0), perf-glob-sorted-2-gate (affected --base origin/next, rc 0).
  Clippy (cargo xtask clippy) and `cargo xtask static` clean on the head.
- Bench cbench_patterns (perf-glob-sorted-2-counts2, mei): glob_1000_sorted instr/op Liella 2897363 vs glibc 3386914 (0.86, i.e. fewer
  than glibc); glob_1000_nosort 0.96. The sorted 1000-entry row is below glibc, so the sort is no longer the cost there.
- The 1.12x on parity1105 glob_sorted_38 (38 entries, 76510 vs 68437 instr) was NOT worked on further: the parity1105 source is not in the
  repo or on mei (~/work/parity-rows-1105 absent), so its locale and the per-call profile could not be reproduced this run. At 38 entries
  the sort is a small share; the rest is readdir, fnmatch and path building (PERF-NFTW-ENTRY overlap). Saturation-register row NOT written
  (no profile to cite). Next: restore the parity1105 spec, confirm the locale, profile with the per-call tool.
- Superseded: perf-glob-sorted-2-counts (wrong bench name, rc 1).
