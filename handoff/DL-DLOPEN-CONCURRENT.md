# DL-DLOPEN-CONCURRENT (DL-16c): lock-free loader readers, safe against dlopen

**State (run 4, DL-DLOPEN-CONCURRENT-4, 2026-11-14).** Branch `task/dl-dlopen-concurrent-4` (run 3's branch plus
this run's commits; stacked on -2, origin/next merged in run 3). Role keke:~/w/n1. Code is complete; this run
changed the readers test only.

**Run 3's red (check 27) was a test defect, not a readers defect.** `tls_cb` took every entry with an empty
`dlpi_name` for the program. Start-time objects are named by their DT_SONAME (DL-16a gap below), and the test's
`setup` rewrites the DT_SONAME tag of its libc copy (`libc-dl.so`) so the program names it by path: that copy is
listed with "" after the program, and its entry (no PT_TLS: module 0, block NULL) overwrote the program's. The
readers report the program's module and block correctly (`readers::tls_of` and the recorded bias agree; both come
from `o.bias`). Fix (ec3413cc6): the program and its dependency are found by the PT_LOAD holding one of their
functions; new KIND 7 `libdeptls` (initial-exec `__thread`, no DT_SONAME, linked into the program by path) gives
check 28: a start-time dependency is listed with its own module ID and the calling thread's block. Program,
initial dependency and `dlopen`'d object are now all checked, kernel-started and as `ld.so PROGRAM`.
`_dl_find_object` has no TLS fields in the 2.39 `struct dl_find_object`, so there is nothing to check there.
Reference probe (black box, keke): the program is listed first, name "", module 1, block = its variable.
Jobs on ec3413cc6 (keke): `dl-dlopen-concurrent-4-lint1` clean; `-gate` 7729/0, conformance 54/0, peak 12G;
`-plants3` 10/10 CAUGHT (handoff/DL-DLOPEN-CONCURRENT-4-plants.txt; run with `LIELLA_LAYOUT_TRACE=ungrouped`, as
the tool's prepare build cannot trace within 600 s on keke: see the report).

## What the branch does
- **Readers** (`crates/boundary/src/ld/readers.rs`): in a program Liella's `ld.so` started, libc's
  `dl_iterate_phdr`/`_dl_find_object` forward through `StartInfo::dl_iterate`/`dl_find_object` to `ld.so`, which
  keeps an append-only static table (bias, name, phdr, phnum) in chain order apart from the writer's `Loader`. A
  `dlopen` stages entries and publishes them with one Release store after pass 2 and the TLS hand-out, before the
  constructors. The walk is live (probe R-2), the vDSO second, no lock held in the callback. `dlpi_tls_modid`/
  `dlpi_tls_data` come from DL-16d's module table (`THREAD_MODS`). `_dl_find_object`: whole PT_LOAD span,
  eh_frame, link map = node address.
- **Counters**: `dlpi_adds` = start objects + vDSO + every mapped object; `dlpi_subs` = objects a failed attempt unmapped.
- **Locks** (`dlopen.rs`): re-entrant `CALL` (writer, whole call incl. constructors) and `STATE` (state changes only);
  `fork` takes `STATE` in prepare, the child frees a vanished thread's `CALL`. Lock order comment at `CALL`.
- **R14-6** differences row (`docs/differences-from-glibc.md`) with a registered, not implemented, compat flag
  `records/pending/DL-DLOPEN-CONCURRENT-3-1.toml` (COMPAT-NEW, rulings #5394/#5418). DL-16c decision:
  `records/pending/DL-DLOPEN-CONCURRENT-1.toml` (D-NEW, series D-27). The coordinator/CI assign both ids.
- **Tests** `crates/ld/tests/dlopen_concurrent.rs`; probe `tools/probe/dl/readers/` (R-1..R-4, spec/behaviour/dynlink.md).

## Open
- Gaps: start-time `dlpi_name` is the DT_SONAME, "" without one (reference: the absolute path; DL-16a gap);
  `dlfo_link_map` is 0 before the first `dlopen` makes the nodes and for the vDSO.
- INFORMATIONAL (not this story): Liella's `libc.so.6` has no `DT_NEEDED` on `ld.so`, so in a program linked
  `--as-needed` a `dlopen`'d object without its own `DT_NEEDED` cannot bind `__tls_get_addr`. Recommend a DL story.
- INFORMATIONAL (pre-existing): an IFUNC resolver that calls `dlopen` re-enters `open_locked` while the outer
  `&mut Loader` is live (STATE is re-entrant, so no deadlock, but the borrow aliases).
- DL-16b: add `dlsym` to the constructor corpus and a `dlsym` reader during a slow constructor (P-8).

## Do not redo
The probe, the table/epoch design, the fork design (STATE, not CALL, in prepare: P-9), the merge resolution, the
check-27 diagnosis above (the readers' TLS lookup is right).

## Next step
Land (coordinator assigns the COMPAT id, CI the D-27 id). Then DL-17 (epochs in `read_section`, sorted address
index). Full report: handoff/DL-DLOPEN-CONCURRENT-4-report.md.

## Plan rows to read
plan/ph14/dl-16-dlopen.md §3, §4, §8; D-27.1, D-27.30, D-27.37; plan/ph14/open-questions.md R14-6; dynlink.md P-7..P-9, readers.
