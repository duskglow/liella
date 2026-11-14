# DL-DLOPEN-CONCURRENT-5 report (DL-16c, run 5: merge with next)

Branch `task/dl-dlopen-concurrent-5` (on run 4's `task/dl-dlopen-concurrent-4`, head 1e5b52778), role keke:~/w/n2.
Why: CI could not merge run 4 (conflicts in crates/boundary/src/ld.rs, crates/boundary/src/runtime.rs,
crates/rt/src/start.rs, docs/differences-from-glibc.md). The brief's coordinator note named DL-16d as the cause;
the conflicting change on next was DL-16b's `dlsym` entry. DL-16d was already on the branch (run 3's merge).

## Commits (this run)
- 8997c9594 Merge origin/next (118e1680b): the four conflicts were additive (next added `StartInfo::dlsym`,
  its fill and `LOADER_DLSYM`; this branch added `dl_iterate`, `dl_find_object`, `dl_fork` and their stores; next
  added the dlopen mode-bits differences row, this branch the R14-6 row): both sides kept. Semantic conflict fixed in
  the same commit: next's `dlsym.rs` called `lock()`/`unlock()`, the single writer lock this branch replaced by
  `CALL` + `STATE`. `dlsym` now takes `CALL` then `STATE`, drops `STATE` before an IFUNC resolver runs, drops `CALL`
  at the end (same shape as `dlopen`'s `entry`). Behaviour as on next: `dlsym` waits for another thread's `dlopen`;
  new: `fork` (takes `STATE` only) never waits for a resolver. Module docs updated. D-27.44 / COMPAT-28 unchanged.
- 0342c290f The fork check calls `dlsym(RTLD_DEFAULT, "slow_in")` before starting the slow opener thread (a lock
  `dlsym` kept blocks that thread's `dlopen`; the hang guard ends the run) and `dlsym` in the forked child.
- 43fc828dc plants file handoff/DL-DLOPEN-CONCURRENT-5-plants.txt (results appended); the handoff and this report.

## Jobs (keke, role n2)
- `dl-dlopen-concurrent-5-lint0` (inherited head 1e5b52778): xtask clippy + static clean, rc 0 (step 0).
- `dl-dlopen-concurrent-5-lint1` (merge 8997c9594): xtask clippy + static clean, rc 0.
- `dl-dlopen-concurrent-5-dltests` (0342c290f): `cargo xtask so` + dlopen_concurrent (4/4: readers, concurrent,
  fork, C++; each kernel-started and as `ld.so PROGRAM`), dlsym, tls: all passed, none skipped.
- `dl-dlopen-concurrent-5-gate` (43fc828dc, `affected --run --base origin/next`): 9 suites, 7730 passed / 0
  failed / 760 ignored; conformance 54/0 (s27_dlopen_tls_surplus included); peak 11G, oom_kill 0, rc 0.
- `dl-dlopen-concurrent-5-plants` (43fc828dc, `LIELLA_LAYOUT_TRACE=ungrouped`, --timeout 600, as run 4): 7 plants,
  7 CAUGHT, 0 SURVIVED, 0 WARN: dlsym-keeps-state, dlsym-keeps-call (fork check, hang guard), libc-no-dlsym-entry
  (fork check 47), libc-no-iterate-entry, publish-before-pass-2, tls-of-no-program (readers check),
  walk-skips-live-thread (DL-16d's live-thread plant, s27_dlopen_tls_surplus T1). One WARN-DEV-SHM note
  (/dev/shm copy invisible to tests that mount their own /dev/shm; none of these do).
- `dl-dlopen-concurrent-5-lint2` (43fc828dc): `cargo clippy --workspace --all-targets --all-features -D warnings`
  + xtask static clean, rc 0.
Final head differs from 43fc828dc by handoff/ files only.

## Open / informational
- DL-16b follow-up: `dlsym` in one thread waits for another thread's slow constructor (writer lock, as on next);
  probe P-8 settles this for `dlopen` only. Recommend DL-16b probes `dlsym` during a slow constructor.
- Run 4's informational items stand (libc.so.6 without `DT_NEEDED` on ld.so; IFUNC resolver re-entering `dlopen`).

## Refusals
None.
