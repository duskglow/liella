# DL-DLOPEN-CONCURRENT-4 report (DL-16c, run 4)

Branch `task/dl-dlopen-concurrent-4`, role keke:~/w/n1. Inherited head 3c69c7ada (run 3's WIP hand-off).

## What was found
Run 3's last job (`dl-dlopen-concurrent-3-lint3`, rc 101) failed readers check 27: "the program listed with module 0
block (nil) ... (libtls module 2)". Cause: the check, not the readers. `tls_cb` matched the program by an empty
`dlpi_name`, but start-time objects are named by their DT_SONAME and the test's libc copy has its DT_SONAME tag
rewritten (so the program can name it by path), so that copy is listed with "" too, after the program, and
overwrote the program's values with its own (no PT_TLS: 0, NULL). `readers::tls_of` (bias + `p_vaddr` against
`THREAD_MODS`) and `record_thread_tls` both use the object's `o.bias`; they agree.

Reference behaviour (black-box probe `dl_iterate_phdr` in a PIE with `__thread`, keke): the program comes first,
name "", `dlpi_tls_modid` 1, `dlpi_tls_data` = the variable's address; libc.so.6 module 2; ld.so and the vDSO 0/NULL.

## Commits (this run)
- ec3413cc6 Find the readers test's start-time TLS objects by address: the program and its dependency are found by
  the PT_LOAD holding one of their functions; new helper KIND 7 `libdeptls` (initial-exec `__thread`, the program's
  `DT_NEEDED` by path) and check 28 (a start-time dependency's module ID and block); plants file
  handoff/DL-DLOPEN-CONCURRENT-4-plants.txt (run 3's eight plus `tls-of-no-program`, `tls-of-no-dependency`).
- The handoff and this report.

## Jobs (keke)
- `dl-dlopen-concurrent-4-lint0` (inherited head 3c69c7ada): xtask clippy + static clean, rc 0.
- `dl-dlopen-concurrent-4-probe1`: `cargo xtask so` + the probe; the Liella half of the probe could not run (the
  probe's libc NEEDED was a bare name; Liella's ld.so has no search in this build), so the reference half only.
  Diagnostic, not in JOBS.
- `dl-dlopen-concurrent-4-t1` (test change before commit): dlopen_concurrent 4 passed.
- `dl-dlopen-concurrent-4-lint1` (ec3413cc6): xtask clippy, `cargo clippy --workspace --all-targets --all-features
  -D warnings`, xtask static: clean, rc 0.
- `dl-dlopen-concurrent-4-gate` (ec3413cc6, direct tier, `--base origin/next`): 9 suites ok, 7729 passed, 0 failed,
  546 ignored; conformance 54/0; SKIPPED 0; peak 12G, oom_kill 0, rc 0 (2327 s; products step 865 s).
- `dl-dlopen-concurrent-4-plants3` (ec3413cc6, `LIELLA_LAYOUT_TRACE=ungrouped`, --timeout 600): 10 plants, 10
  CAUGHT (each by its `msg:`), 0 SURVIVED, 0 EQUIVALENT, 0 WARN-RC-ONLY, 0 WARN-NO-MSG; rc 0. Run 3's survivor
  `tls-of-start-only` is now caught (check 26); `tls-of-no-program` by check 27, `tls-of-no-dependency` by check 28.
- SUPERSEDED: `dl-dlopen-concurrent-4-plants` and `-plants2` (rc 2, "BASELINE FAILED: so rc 124"): the tool's
  unmutated `cargo xtask so` in the copy runs under the per-plant 600 s limit, and its trace-at-build step (gdb over
  the `page_faults` modes) takes about 10 minutes on this Zen 1 host (the build itself about 3 minutes; the gate's
  whole products step 865 s). The first guess (contention with the gate's build) was wrong: -plants2 ran alone and
  failed the same way. Plants judge behaviour, not layout, so -plants3 used the documented opt-out
  `LIELLA_LAYOUT_TRACE=ungrouped` (no tracer) and kept the 600 s limit.

## Tooling findings (INFORMATIONAL)
- The plants tool's prepare build shares the per-plant timeout; with trace-at-build the default it cannot finish on
  keke (next's PLANTS-PREPARE-TIMEOUT merge may address the prepare step; this branch does not have it). Each
  plant's own products rebuild would also trace, so a timeout there would read as "caught by timeout": plants that
  rebuild the products should run ungrouped, or the tool should set `LIELLA_LAYOUT_TRACE=ungrouped` itself.
- When `timeout` ended the prepare build, the trace's `timeout 900 gdb ...` children were orphaned (reparented to
  init) and kept running after the job ended (6 processes from -plants2, killed by PID; -plants's likely overlapped
  -plants2). The tool should run each command in its own process group and kill the group.

## Rulings, informational
- No new ruling. R14-6 row and the pending compat flag stay as run 3 wrote them (ids assigned by the coordinator/CI).
- INFORMATIONAL: start-time `dlpi_name` is "" for an object without DT_SONAME (the reference gives the path):
  the known DL-16a gap; it is what made check 27 misfire. Worth fixing in DL-16a's continuation.
- INFORMATIONAL (carried): libc.so.6 has no `DT_NEEDED` on ld.so (see the handoff page).

## Refusals
None.
