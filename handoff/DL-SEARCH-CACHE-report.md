# DL-SEARCH-CACHE report (phase 14 DL-8)

**Status: PENDING JOBS (context checkpoint).** Branch `task/dl-search-cache`; commits a6e30fbf3, 1fd73d147, then
the handoff commit. Role keke:~/w/n1.

## Jobs (keke)
* `dl-search-cache-gate`: `cargo xtask clippy && cargo xtask static && cargo xtask so && cargo xtask affected
  --run --base origin/next` (still running at hand-back; log ~/logs/dl-search-cache-gate.log).
* `dl-search-cache-plants`: `cargo xtask plants handoff/DL-SEARCH-CACHE-plants.txt --dir
  /dev/shm/claude/DL-SEARCH-CACHE/plants-1 --timeout 900` (still running).
* `dl-search-cache-startup`: after the gate, `cargo xtask bench --compare --counters cbench_startup` (queued
  behind the gate). The brief asks for counts vs origin/next: this compares with glibc only; a run on an
  origin/next tree is still needed. Expected no change: list mode is only taken when `--list` or
  `LD_TRACE_LOADED_OBJECTS` is given; the normal start adds one `bool` test and an unused struct.
* Seen before the gate: `cargo clippy -p liella-ld -p liella-boundary --all-targets -- -D warnings` clean on keke
  (run in the foreground, not a jobrun job).
* Re-attach: `ssh keke '~/bin/jobwait dl-search-cache-gate dl-search-cache-plants dl-search-cache-startup'`.

## Probe-settled
* Reference `ld.so --help` (black box, this workstation's Ubuntu glibc 2.39): system search path
  `/lib/x86_64-linux-gnu`, `/usr/lib/x86_64-linux-gnu`, `/lib`, `/usr/lib`; hwcaps priority `x86-64-v4`, `-v3`,
  `-v2`. Used for `Arch::SYSTEM_DIRS` and `Arch::hwcaps`.
* `ldd` parity: `list_matches_reference` runs the reference `--list` and Liella's `--list` and
  `LD_TRACE_LOADED_OBJECTS` on a program with `$ORIGIN` RUNPATHs, a missing library and `libgcc_s.so.1` from the
  host cache, comparing `name => path` lines (pinned sonames excluded). Result: in the gate (pending).

## RULING NEEDED
* Trusted vs default directories (rule 5 lists `/lib64`, `/usr/lib64`; the reference searches `/lib`, `/usr/lib`
  and not `/lib64`). Recommend amending rule 5 to the probed list.
* Default `LIELLA_PREFIX` `/` and pinned dir `/lib` for it. Recommend keep `/`, take the pinned directory for `/`
  from the arch table (`/lib/x86_64-linux-gnu`), decided with the install story.

## INFORMATIONAL
* Cache key matching follows the reference (numeric comparison: `libv.so.1.9` matches `libv.so.1.09`); no
  differences row needed.
* Gaps: rule 11 product test, DF_1_NODEFLIB, `$PLATFORM` beyond Haswell, `--library-path`/`--inhibit-cache`,
  libFuzzer run of `ld_cache`, the order of "not found" lines vs the reference if the gate shows a mismatch.

## Refused
None.
