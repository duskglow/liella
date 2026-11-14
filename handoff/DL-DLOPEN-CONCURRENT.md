# DL-DLOPEN-CONCURRENT (DL-16c): lock-free loader readers, safe against dlopen

**STOPPED AT THE CONTEXT CHECKPOINT (run 3, 2026-11-14): INCOMPLETE, head test-red.** Code head 17d465549 adds
check 27 (the program's own `__thread` variable): it FAILS: `the program listed with module 0 block (nil) ... (libtls
module 2)` (job `dl-dlopen-concurrent-3-lint3`, against release products of the same code). So `readers::tls_of`
misses the PROGRAM's start-time TLS module (the program holds module 1: libtls got 2), while it finds a `dlopen`'d
one. A real DL-16c defect (`dlpi_tls_modid`/`dlpi_tls_data` of the program are 0), found because Liella's libc.so.6
and ld.so have no PT_TLS, so the plant `tls-of-start-only` survived until the program got TLS. **Next step:** find
why `e.bias + p_vaddr` (readers.rs `tls_of`; the program's bias is `START_OBJS[0][2]` via `fill_start`) differs from
the program's `THREAD_MODS` image (`biases[i] + m.vaddr` in ld.rs `record_thread_tls`): print both in a scratch
run, fix `tls_of` or the recorded bias, rerun the dlopen_concurrent test, lint, gate, then plants
(`handoff/DL-DLOPEN-CONCURRENT-3-plants.txt`: tls-of-start-only expects check 26 and should now be caught).

**State before check 27 (DL-DLOPEN-CONCURRENT-3, 2026-11-13).** Branch `task/dl-dlopen-concurrent-3` (stacked on -2's branch, then
origin/next merged at 9fdf87d40 after LAYOUT-TRACE-DEFAULT-4 landed). Role mei:~/w/s2. Code head 811bc207f was green
(below). **The layout red is gone:** with trace-at-build the default,
`hot_text_products` passes fast and interposed in the direct-tier gate (both gates of this run) and in the release profile
(job `dl-dlopen-concurrent-3-layout-release`, see the report). No bound touched.
Run 3 commits: merge (one conflict at the dlopen publication point: DL-16d's `publish_tls` stays before the chain's
`l_next` store and `readers::publish`, so a listed object's TLS module is already recorded); readers test check 25/26:
a `dlopen`'d `PT_TLS` object is listed with `dlpi_tls_modid` != 0 and `dlpi_tls_data` equal to the address its own code
reaches (DL-16d appends `dlopen`'d modules to `THREAD_MODS`, which `readers::tls_of` reads, so the gap below closed
with the merge; the helper uses initial-exec, see INFORMATIONAL); the R14-6 differences row
(`docs/differences-from-glibc.md`, user-facing incompatibilities) with a registered, not implemented, compat flag
`records/pending/DL-DLOPEN-CONCURRENT-3-1.toml` (COMPAT-NEW: the coordinator assigns the id; decisions D-27.1;
rulings #5394/#5418). The DL-16c decision is still pending: `records/pending/DL-DLOPEN-CONCURRENT-1.toml` (D-NEW,
series D-27; CI auto-assigns).
Jobs (mei): `-lint0` (inherited head) and `-lint2` (final head): clippy + static clean. `-gate` (fc56524a2): 7728/1,
my new check 25 (the TLS helper could not bind `__tls_get_addr`); fixed in 811bc207f. `-gate2` (811bc207f): 7729
passed, 0 failed, conformance 54/0, peak 10G. `-plants` (fc56524a2): baseline failed on the same check, no plant run.
`-plants2` (811bc207f): 7 CAUGHT by message, 0 WARN, 1 SURVIVED (`tls-of-start-only`, see above). `-layout-release`
(811bc207f): hot_text_products 25/25 release profile (the role's target/liella holds release products since).

## What the branch does
- **Readers** (`crates/boundary/src/ld/readers.rs`): in a program Liella's `ld.so` started, libc's
  `dl_iterate_phdr`/`_dl_find_object` forward through `StartInfo::dl_iterate`/`dl_find_object` to `ld.so`, which
  keeps an append-only static table (bias, name, phdr, phnum) in chain order apart from the writer's `Loader`. A
  `dlopen` stages entries and publishes them with one Release store after pass 2 and the TLS hand-out, before the
  constructors. The walk is live (probe R-2), the vDSO second, no lock held in the callback. `read_section` wraps
  every read (empty; DL-17 adds epochs). `_dl_find_object`: whole PT_LOAD span, eh_frame, link map = node address.
- **Counters**: `dlpi_adds` = start objects + vDSO + every mapped object; `dlpi_subs` = objects a failed attempt unmapped.
- **Locks** (`dlopen.rs`): re-entrant `CALL` (writer, whole call incl. constructors) and `STATE` (state changes only);
  `fork` takes `STATE` in prepare, the child frees a vanished thread's `CALL`. Lock order comment at `CALL`.
- **Tests** `crates/ld/tests/dlopen_concurrent.rs`; probe `tools/probe/dl/readers/` (R-1..R-4, spec/behaviour/dynlink.md).

## Open
- Gaps: start-time `dlpi_name` is the DT_SONAME (reference: absolute path, DL-16a gap); `dlfo_link_map` is 0 before
  the first `dlopen` makes the nodes and for the vDSO.
- INFORMATIONAL (not this story): Liella's `libc.so.6` has no `DT_NEEDED` (the reference's names `ld-linux-x86-64.so.2`),
  so in a program linked `--as-needed` (the toolchain default) `ld.so` is not in the global scope and a `dlopen`'d
  object without its own `DT_NEEDED` cannot bind `__tls_get_addr` ("undefined symbol"). Not probed on the reference.
  Recommend a DL story: probe the reference's scope for that case, then decide (add the `DT_NEEDED`, or keep the
  interpreter in the global scope).
- INFORMATIONAL (pre-existing): an IFUNC resolver that calls `dlopen` re-enters `open_locked` while the outer
  `&mut Loader` is live (STATE is re-entrant, so no deadlock, but the borrow aliases).
- DL-16b: add `dlsym` to the constructor corpus and a `dlsym` reader during a slow constructor (P-8).

## Do not redo
The probe, the table/epoch design, the fork design (STATE, not CALL, in prepare: P-9), the merge resolution.

## Next step
Fix the program's TLS entry in the readers (top of this page), re-gate, plants on the final head; then land
(coordinator assigns the COMPAT id, CI the D-27 id). Then DL-17 (epochs in `read_section`, sorted address index).

## Plan rows to read
plan/ph14/dl-16-dlopen.md §3, §4, §8; D-27.1, D-27.30, D-27.37; plan/ph14/open-questions.md R14-6; dynlink.md P-7..P-9, readers.
