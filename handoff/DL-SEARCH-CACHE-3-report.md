# DL-SEARCH-CACHE-3 report (phase 14 DL-8, continuation 3)

Branch `task/dl-search-cache-3`, role keke:~/w/n4.

## Why run 2 went red
* `dl-search-cache-2-plants2` rc 1: plant `cache-string-end-unchecked` (string offsets accepted from the header
  end instead of the string table's start) SURVIVED. The corpus probe only checked that a returned path starts
  with `/`, which the reader has guaranteed since run 2, and no corpus case put a `/`-led string inside the entry
  table. Fixed in the test (19a1d91fe): every returned path must lie inside the string table the header declares
  (Appendix D.3.1), and a new case points entry 1's value offset at its osversion word holding `/ev\0`.
* `dl-search-cache-2-gate` rc 124: `fuzz/Cargo.toml` escalated `affected` to FULL, which ran past its limit. See
  INFORMATIONAL.

## Commits (run 3)
19a1d91fe (corpus string-table check; run 3 plants file with the rule-11 plant), then the handoff/report commit.

## Jobs (keke, all on the code of 19a1d91fe; later commits are handoff/ only)
* `dl-search-cache-3-lint0` rc 0: clippy + static on the inherited head (step 0).
* `dl-search-cache-3-gate` rc 0: `~/bin/gate 16G`: clippy, static, so, `affected --run --base origin/next --tier
  direct --files <the 25 changed files but fuzz/Cargo.toml>`: 18 steps ok, 0 failed, conformance 126 passed,
  4 SKIPPED lines, peak 12G, oom_kill 0. `fuzz-check` was selected (by `fuzz/fuzz_targets/ld_cache.rs`) and is in
  static.
* `dl-search-cache-3-plants` rc 0: `cargo xtask plants handoff/DL-SEARCH-CACHE-3-plants.txt --dir
  /mnt/nvme/claude/plants/DL-SEARCH-CACHE-3/plants-1 --timeout 900`: 10 plants, 10 CAUGHT, no WARN-RC-ONLY /
  WARN-NO-MSG. WARN-NOT-RAM is deliberate (#4286): /dev/shm is nosuid on keke, so the set-group-ID child gets no
  AT_SECURE there and the rule-11 test skips; /mnt/nvme has no nosuid. The rule-11 plant
  (`secure-traces-loaded-objects`) was caught by `ldso_run_secure_child_sees_unsecure_cleared`.
* `dl-search-cache-3-count-base2` rc 0 (origin/next 2e33a3cb9, a `git archive` tree in
  /mnt/nvme/claude/w/dl-search-cache-3-base) and `dl-search-cache-3-count` (this branch, role):
  `cargo xtask bench --compare --counters --runs 3 cbench_startup`. Results below.
* Superseded: `dl-search-cache-3-count-base` rc 1 (wrong syntax: `--counters cbench_startup` without `--compare`
  is refused), `dl-search-cache-3-quick` and `dl-search-cache-3-list` (diagnostic: clippy + search_cache test;
  the gate selection).

## cbench_startup, Liella user-mode instructions per process (Zen 1 host)
| Row | origin/next 2e33a3cb9 | this branch | change | reference (dynamic) |
|---|---:|---:|---:|---:|
| `startup_instr_c` | 2146 | 2146 | 0 | 117438 |
| `startup_instr_setloc` | 68079 | 68093 | +14 (+0.02%) | 142651 |
| `startup_instr_printf` | 71875 | 71875 | 0 | 144422 |
| `startup_instr_wctype` | 69104 | 69095 | -9 (-0.01%) | 142856 |

Parity by instructions. The rows link Liella statically (bench/c/cbench_startup.c), so the loader code this story
changes is not on their path; the +14/-9 in opposite directions are the address-randomisation spread the bench
documents ("ASLR moves a few instructions"; minimum of 5 children). All rows stay under bench/instr_limits.tsv.

## INFORMATIONAL
* `cargo xtask affected` sends `fuzz/Cargo.toml` to FULL ("build configuration": the `file == "Cargo.toml"` rule runs
  before the `fuzz/` rule), although the `fuzz-check` selection code expects fuzz `Cargo.toml` changes. Any story
  that adds a fuzz target then gates FULL (run 2's gate hit its limit that way). Recommend an xtask story that classifies
  `fuzz/Cargo.toml` like the other fuzz files (fuzz-check only).
* Gaps (records only, D-27.16): DF_1_NODEFLIB, `--library-path`/`--inhibit-cache`, `$PLATFORM` beyond Haswell,
  `ld_cache` under libFuzzer.

## Refused
None.
