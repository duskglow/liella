# DL-DLOPEN-CONCURRENT (DL-16c): lock-free loader readers, safe against dlopen

**State (run 5, DL-DLOPEN-CONCURRENT-5, 2026-11-14).** Branch `task/dl-dlopen-concurrent-5` (run 4's branch
`task/dl-dlopen-concurrent-4` plus this run's merge of origin/next and one test change). Role keke:~/w/n2. Code is
complete; this run only merged. Full report: handoff/DL-DLOPEN-CONCURRENT-5-report.md (run 4's:
handoff/DL-DLOPEN-CONCURRENT-4-report.md).

**The merge (8997c9594).** next brought DL-16b's `dlsym`/`dlvsym` (`StartInfo::dlsym`, `LOADER_DLSYM`,
`ld/dlopen/dlsym.rs`). The four conflicts (crates/rt/src/start.rs, crates/boundary/src/ld.rs,
crates/boundary/src/runtime.rs, docs/differences-from-glibc.md) were additive: both sides' StartInfo fields, fills,
stores and differences rows are kept. One semantic conflict git did not flag: `dlsym.rs` called next's single
writer lock `lock()`/`unlock()`, which this branch split into `CALL` (whole call) and `STATE` (state changes; the
lock `fork` takes). `dlsym` now takes `CALL` then `STATE`, releases `STATE` before an IFUNC resolver runs and `CALL`
at the end, exactly as `dlopen`'s `entry` treats its constructors: dlsym waits for another thread's `dlopen` as it
did on next, and `fork` never waits for a resolver. Record ids D-27.44 and COMPAT-28 kept as assigned. DL-16d's
pieces (`__liella_thread_walk`, `THREAD_MODS`, the LIST-locked fill) were already on the branch (run 3's merge).

**Test (0342c290f).** The fork check calls `dlsym` before it starts the thread whose `dlopen` it waits for (a lock
`dlsym` kept would block that `dlopen` until the hang guard ends the run) and in the child forked while that thread
held the writer lock.

## What the branch does
- **Readers** (`crates/boundary/src/ld/readers.rs`): in a program Liella's `ld.so` started, libc's
  `dl_iterate_phdr`/`_dl_find_object` forward through `StartInfo::dl_iterate`/`dl_find_object` to `ld.so`'s
  append-only table (bias, name, phdr, phnum), published with one Release store after pass 2 and the TLS hand-out,
  before the constructors. No lock in the callback; vDSO second; TLS modid/block from DL-16d's `THREAD_MODS`.
- **Counters**: `dlpi_adds` = start objects + vDSO + every mapped object; `dlpi_subs` = objects a failed attempt unmapped.
- **Locks** (`dlopen.rs`): re-entrant `CALL` and `STATE`; `dlopen` and `dlsym` take both; `fork` takes `STATE` in
  prepare, the child frees a vanished thread's `CALL`. Lock order comment at `CALL`.
- **R14-6** differences row with compat flag COMPAT-28 (registered, not implemented); decision D-27.44.
- **Tests** `crates/ld/tests/dlopen_concurrent.rs`; probe `tools/probe/dl/readers/` (R-1..R-4, spec/behaviour/dynlink.md).

## Open
- Gaps: start-time `dlpi_name` is the DT_SONAME, "" without one (reference: the absolute path; DL-16a gap);
  `dlfo_link_map` is 0 before the first `dlopen` makes the nodes and for the vDSO.
- DL-16b follow-up: whether `dlsym` in one thread should wait for another thread's slow constructor (it does, as on
  next; probe P-8 settles `dlopen` only). A `dlsym` reader during a slow constructor belongs in DL-16b's tests.
- INFORMATIONAL (not this story): Liella's `libc.so.6` has no `DT_NEEDED` on `ld.so` (see run 4's report).
- INFORMATIONAL (pre-existing): an IFUNC resolver that calls `dlopen` re-enters `open_locked` while the outer
  `&mut Loader` is live (STATE is re-entrant, so no deadlock, but the borrow aliases).

## Do not redo
The probe, the table/epoch design, the fork design (STATE, not CALL, in prepare: P-9), both merge resolutions, the
check-27 diagnosis (run 4), dlsym's lock pair (above).

## Next step
Land. Then DL-17 (epochs in `read_section`, sorted address index).

## Plan rows to read
plan/ph14/dl-16-dlopen.md §2, §3, §4, §8; D-27.1, D-27.30, D-27.37, D-27.44; plan/ph14/open-questions.md R14-6;
dynlink.md P-7..P-9, readers.
