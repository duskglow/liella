# DL-SEARCH-CACHE-2 report (phase 14 DL-8, continuation)

Branch `task/dl-search-cache-2`, role keke:~/w/n1.

## Why run 1 went red
* `dl-search-cache-gate` rc 1: `cargo xtask clippy` (strict lints on the new test file, `hex16` arithmetic in
  boundary/src/ld.rs); `cargo xtask static` also failed (6 tests without clause IDs, one untagged x86_64 `cfg`).
* `dl-search-cache-plants` rc 2: baseline failed, `cache_corrupt_corpus_refused`: an entry value offset at the
  string table's final NUL gave an empty path. Fixed in the reader (a cache path must start with `/`).
* After the lints, `list_matches_reference` failed its precondition: the host `cc` links with `--as-needed`,
  so liba.so.1 had no DT_NEEDED for libb/libgone/libgcc_s (both loaders agreed). Linked with `--no-as-needed`.
* Then the real difference showed: see "Probe-settled".

## Commits (run 2)
merge of origin/next; cd69677d2 (rule 5 per #4237, relative cache paths refused, lints, clause IDs, ARCH tag);
08682cde0 (rule 11 product test, codemap); be837bee6 (`--no-as-needed`); d785bae06 (`--list`/trace on a missing
dependency).

## Probe-settled (reference loader 2.39, black box, on keke)
* `ld.so --list PROG` with a dependency not found: stderr `PROG: error while loading shared libraries: NAME: cannot
  open shared object file: No such file or directory`, stdout empty, status 127.
* `LD_TRACE_LOADED_OBJECTS=1 PROG` (kernel start) and `ldd PROG`: full list with `NAME => not found`, status 0.
* `ld.so --help`: default directories `/lib/x86_64-linux-gnu`, `/usr/lib/x86_64-linux-gnu`, `/lib`, `/usr/lib`
  (re-checked on the workstation): with #4237 these four are the trusted directories for prefix `/`.

## Jobs (keke)
* `dl-search-cache-2-gate`: `~/bin/gate 16G` clippy, static, so, `affected --run --base origin/next`.
* `dl-search-cache-2-plants`: `cargo xtask plants handoff/DL-SEARCH-CACHE-2-plants.txt --dir
  /dev/shm/claude/DL-SEARCH-CACHE-2/plants-1 --timeout 900` (10 plants).
* Diagnostic runs (older heads, superseded): `dl-search-cache-2-lint1`, `-lint2`, `-lint3`, `-listtest`.
* Results: see the hand-back (written before both jobs finished if they ran long).

## INFORMATIONAL
* Not done (gaps): DF_1_NODEFLIB, `$PLATFORM` beyond Haswell, `--library-path`/`--inhibit-cache`, `ld_cache` under
  libFuzzer, cbench_startup counts vs origin/next.

## Refused
None.
