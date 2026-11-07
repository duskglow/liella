# WL-PIPE-READS-ADVISORY: pipe read/write counts of multi-process workloads are advisory

State: branch `task/wl-pipe-reads-advisory`, code commit 344f6361e (from origin/next 82fe6cfd1); this note is the next commit.
Decision in force: D-X.46 (plan/07-decisions/x-implementation-decisions.md). Owner ruling #4065.

## What changed
- `validation/wl_syscls.sh`: `WL_CLASSIFY` reads an `strace -C -f -y` output (trace lines, then the `% time` summary;
  a summary-only `strace -c` file still works, with piperw 0). With `mp=1` each trace line starting a read/write-family
  call whose first fd is `N<pipe:[ino]>` counts as pipe rw: it leaves `rw` and the gating total, and becomes the 12th
  field `piperw` (advisory). New `wl_has_child CLONEFILE`: a clone/clone3 without CLONE_THREAD, fork or vfork.
- `wl_run.sh`, `wl_count_run.sh`: the clone trace adds `fork,vfork`, the strace run is `strace -C -f -y`, MP is passed in;
  wl_run adds the column `sys_piperw`; wl_count rows have 11 fields (old 10-field cached rows are re-measured) and print
  "pipe rw (advisory)". `wl_stats.sh`: `wl_strace_sys FILE MT MP` and `wl_strace_median MT MP FILE...` give 4 numbers.
- Single-process runs keep pipe calls in the gate; every other class is unchanged.
- Docs: script headers, validation/README.md (wl_count paragraph), wl_report.py legend.

## Tests and proof (probe: strace 6.8 on the build host writes the trace and the -C summary into the one -o file)
- `bash validation/wl_stats_test.sh` 38 passed; `bash validation/wl_run_test.sh` 24 passed (local). Canned traces:
  only pipe reads differ, so the gating number stays the same; a file read differs, so it changes; single-process keeps pipe calls;
  the median includes piperw; wl_has_child cases.
- Plants: 14 in the story's plants file (copied to `~/work/WL-PIPE-READS-ADVISORY/plants.txt` on the host),
  `cargo xtask plants`, job `WL-PIPE-READS-ADVISORY-plants`. Gate job `wlpipe-gate-1` (clippy, static, affected
  --base origin/next): rc 0, clippy ok, static ok, affected selected 0 of 246 targets (validation/ scripts map to no
  test target; the shell self-tests above are the proof). Plants: PENDING at hand-back (first 2 of 14 CAUGHT with msg).
- `validation/wl_stats_plants.sh`: two legacy plant patterns updated to the new text (count_cache_old, count_mt_ignored).

## Limits / open
- A pipe call is recognised by its FIRST fd argument (splice/sendfile with a pipe only as the output side stay deciding).
- `-y` adds a /proc lookup per traced call: the strace runs are slower (they are counts only, not timed).
- Not re-run: a real wl_count of pipe-mix / tar-gz to confirm that the gating number is now stable (next step).
- Process slip: one `sed -i` was used on validation/wl_run_test.sh (a path fix in a test line); the rules ask for Edit.

## Next step
Collect `wlpipe-gate-1` and `WL-PIPE-READS-ADVISORY-plants` (`~/bin/jobwait <label>` on the build host); then a
wl_count of pipe-mix and tar-gz twice on one build to see the gating syscalls repeat.
