# DL-DLOPEN-CONCURRENT (DL-16c): lock-free loader readers, safe against dlopen

**State.** Branch `task/dl-dlopen-concurrent` (cut from origin/next 68e72ab32). Code commit 2d72f7cc2, test
hang-guard fix 599c1d728 (final code head); this page and the report json follow it. Role mei:~/w/s1.
**INCOMPLETE: LAYOUT-JITTER (ruling #4347).** Gate `dl-dlopen-concurrent-gate` (2d72f7cc2): 7714 passed, 2 failed,
both `hot_text_products init_data_clusters_hold_at_every_text_phase` (fast and interposed): tool faults 53 at nine
pads, 54 at `cold 0xf000`. The same test on origin/next 68e72ab32 (job `dl-dlopen-concurrent-next-layout`, a RAM
copy on mei): 53 at all ten pads, passes. The change grows cold text only (fork's lock step, the readers'
forwarding, early_init's three stores for dynamic programs); the static start path the test measures runs none of
it. Not chased (no regen, no moved sections, no bound change). Final jobs on 599c1d728: `dl-dlopen-concurrent-gate2`
(clippy and static clean; 7714 passed, 2 failed: the same layout test with the same numbers, peak 11G) and
`dl-dlopen-concurrent-plants2` (7/7 CAUGHT by message, 0 WARN, rc 0).
Plants run 1 (`dl-dlopen-concurrent-plants`, 2d72f7cc2): 7/7 caught; `find-no-link-map` WARN-NO-MSG (check 16
caught it first; regex now names 16|18); `child-keeps-writer-lock` caught only by the 600 s tool timeout because a
fork child does not inherit `alarm` (fixed in 599c1d728: the child arms its own). Decision: `records/pending/DL-DLOPEN-CONCURRENT-1.toml`
(D-NEW, series D-27; waits for the coordinator's id). dlsym (DL-16b) was not on next when this run started, so
nothing here builds on it.

## What the branch does
- **Readers** (`crates/boundary/src/ld/readers.rs`, new): in a program Liella's `ld.so` started, libc's
  `dl_iterate_phdr`/`_dl_find_object` forward through `StartInfo::dl_iterate`/`dl_find_object` (set by
  `run_init`, stored by `runtime::early_init`) to `ld.so`. `ld.so` keeps an append-only static table (bias, name,
  phdr, phnum) in chain order, apart from the writer's `Loader` (readers never form a reference to it). The
  start-time entries are filled at the first reader or `dlopen` (nothing at start). A `dlopen` stages entries in
  `candidate` and publishes with one Release store (`readers::publish`, next to the chain's `l_next` store, after
  pass 2 and before the constructors). The walk is live (probe R-2), the vDSO second, the callback holds no lock.
  `read_section(|view| ..)` wraps every read (empty; DL-17 adds epochs). `_dl_find_object`: whole PT_LOAD span,
  eh_frame, link map = node address (probe R-1). Static programs keep the old path (one load+branch more).
- **Counters**: `dlpi_adds` = start objects + vDSO + every object a `dlopen` mapped; `dlpi_subs` = objects a
  failed attempt unmapped (`count_unmapped` in `unwind` and the two post-map refusals in `candidate`).
- **Locks** (`dlopen.rs`): `Reentrant` futex mutex; `CALL` (writer, whole call incl. constructors) and `STATE`
  (inside it, only while state changes). `fork_step` (`StartInfo::dl_fork`): prepare takes `STATE` first in
  `fork::lock_all` (only when other threads exist, P5), parent/child release; the child frees `CALL` if a vanished
  thread held it. Lock order comment at `CALL`.
- **Probe** `tools/probe/dl/readers/` (R-1..R-4) recorded in `spec/behaviour/dynlink.md` + `.glibc.txt`.
- **Tests** `crates/ld/tests/dlopen_concurrent.rs` (Liella only; the IFUNC-window check is a Liella design
  property): readers/publication/counters/callback-dlopen/constructor corpus; 20000-iteration concurrent walk;
  fork during another thread's constructor (P-9); C++ exception through a `dlopen`'d C frame.

## Open / for later stories
- RULING NEEDED: the R14-6 differences row (callbacks run without the loader lock; s27_1_c_8 says the reference
  holds one). The plan puts it in DL-17; the behaviour is observable from this branch on. Recommend: add it now,
  no compat option (holding the lock across a callback only adds a deadlock path).
- Gaps: start-time `dlpi_name` is the DT_SONAME (reference: absolute path; same as `l_name`, DL-16a gap);
  `dlfo_link_map` is 0 before the first `dlopen` creates the nodes and for the vDSO (reference non-null).
- DL-16b: add `dlsym` to the constructor corpus and a `dlsym` reader during a slow constructor (P-8) through
  `read_section`. DL-16d: `tls_of` matches only start-time modules (DL-15's `THREAD_MODS`); place a `dlopen`'d
  module's TLS before `readers::publish` and report its module id there. My change at the publication point is the
  one `readers::publish(k, k - base)` line.
- INFORMATIONAL (pre-existing): an IFUNC resolver that calls `dlopen` re-enters `open_locked` while the outer
  `&mut Loader` is live (STATE is re-entrant, so no deadlock, but the borrow aliases).

## Do not redo
The probe, the table/epoch design, the fork design (STATE, not CALL, in prepare: P-9 needs fork to return while a
constructor runs).

## Next step
Land. Then DL-17 (epochs in `read_section`, sorted address index) and DL-16b's corpus additions.

## Plan rows to read
plan/ph14/dl-16-dlopen.md §3, §4, §8; D-27.1, D-27.30, D-27.37; dynlink.md P-7, P-8, P-9, `== phdr`, readers.
