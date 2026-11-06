# LOADTIME-ENTRY-DESIGN handoff (updated by LOADTIME-ENTRY-DESIGN-2)

State: branch `task/loadtime-entry-design-2`, stacked on `task/loadtime-entry-design` (cut from `origin/next`
`06cefa93`). Doc commit `e774f024`, then the handoff/report commit (`git log -1`). The deliverable is
`docs/design/loadtime-entry.md`: section 4 amended, section 5 new. Design only, no product code.

## Result

**M6 replaces M4.** `.text.liella_classed` holds two pre-built 4096-byte pages with one slot layout: page 0 holds
the SSE2 bodies (the exported labels), and page 1 the AVX2 bodies, assembled for page 0's address (`XREF(sym)` =
`sym + 4096` on out-of-page references; no fixups). At static step 4, on the AVX2 class, a pagemap guard (3 calls)
runs, then `mremap(page1 -> page0, MAYMOVE|FIXED)` (1 call). Nothing is written, no exec permission is gained, no
private memory is used, and no fd is needed for the swap. Per-call cost is 0 on both classes, and section 3's
predictions stand.

Probe-settled (unprivileged; Linux 6.8.0 build host plus Linux 7.0.14; static and PIE; 80 runs, none wrong):
* M4's `mprotect` RX on the written copy: **EACCES** under `prctl(PR_SET_MDWE, PR_MDWE_REFUSE_EXEC_GAIN)` and under
  `systemd-run --user -p MemoryDenyWriteExecute=yes -p NoNewPrivileges=yes`, and silent: SSE2 stays and nothing is
  logged. systemd 255 accepted the property in the user manager and applies it as PR_SET_MDWE (`PR_GET_MDWE`=1,
  `Seccomp: 0`).
* M6 (`mremap`; also `+DONTUNMAP`; also `mmap MAP_FIXED` RX from the file) works under every policy.
* gdb: a breakpoint inserted before `run` is lost at the swap. A later stop writes the stale byte into the moved
  page, and the process then gets SIGSEGV. This hits **both** M4 and M6. The pagemap guard (live page present and
  not a file page means skip) fixed it under gdb.
* perf names the slot correctly, but `perf annotate` shows the file's SSE2 bytes.

## Decisions

D-4.53 is **reserved** (`ops reserve`) for this design. Its text is in doc section 4. It is not added to the
register until the rulings. `cargo xtask static` reports it as "cited without a row (report only)".

## RULING NEEDED (doc section 4)

1. Swapping a read+exec text page at start-up (M6, no write): recommend approve. It replaces M4's item 1.
2. 4 system calls and +2 VMAs on the AVX2 class only: recommend accept unless the start-up row measures worse.
3. Guard when `/proc` is missing: recommend keep SSE2 (always correct).
4. Diagnostics (perf annotate, core files, uprobes on the exported symbol after the swap): recommend accept, plus a
   debugging note.
5. RELRO for the remaining words: optional follow-up, as before.

## Not verified / gaps

* SELinux `execmod`, seccomp-form MDWE (kernel < 6.3) and PaX: documented behaviour only, UNTESTED. The exact
  root-only SELinux command for the owner is in doc 5.2. No pre-6.3 kernel was available.
* The keke run of the same probe (same 6.8 kernel as the build host) sat queued behind gates for about 45 minutes and
  was cancelled with `kill-label`. The second kernel came from a 7.0.14 machine instead.
* Not probed: uprobes after the swap, core-file contents, symbolisation when attaching to a running process, and THP
  text. All are marked NOT VERIFIED in 5.6.
* Predicted counts from -1 remain unmeasured.

## Things not to redo

M4's fixup tables and private copy are dropped, so do not design them further. M6b (`DONTUNMAP`, Linux 5.13+) and
M6c (fd plus `mmap`, 3 calls, and for `libc.so.6` a path that can be replaced on disk) were considered and set aside
for M6a.

## Next step

The coordinator rules on items 1-4, then cuts CLASSED-SLOT-INFRA (doc section 4, story 1, with its tests and
plants), and the debugging note (story 7) with it.

Plan rows to read: D-4.25, D-4.14, D-4.45, D-4.52, D-X.15, D-10.50, D-1.13.
