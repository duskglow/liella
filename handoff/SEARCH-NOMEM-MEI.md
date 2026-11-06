# SEARCH-NOMEM-MEI: handoff (search_nomem RLIMIT_AS limit now sized at the call)

Branch task/search-nomem-mei, cut from origin/next 41b8737b. Role mei:~/w/s2; keke loop/plants ran in keke:~/w/search-nomem-mei
(an unregistered directory, synced with sync_role.sh). Code commit 3ae55040 (test only, no product change).

## The failure (from mei logs, not reproduced in my slot)
- `hcreate_and_hsearch_fail_with_the_table_intact`: child exit 14 ("the limit never bit"), only in the exports-interposed
  pass (`--features liella-boundary/export,selftest`, where the test's own Vec ballast goes through Liella's malloc), e.g.
  mei ~/logs/ci-gate-18077d4d1-1793911619.affected.log line 10882.
- Mechanism: hsearch grows by chained segments (5, 11, 23 ... 49151 entries, D-14.3); 40000 keys need the last one,
  ~786 KiB, a large block (mmap). The old test set RLIMIT_AS once to (size before ballast + 1 MiB) and relied on the
  ballast leaving under 786 KiB. Measured view on mei in a 6-CPU slot (old test, 192 runs, 48 parallel): base 78950400,
  limit +1 MiB, ballast 0 x 1 MiB + 2536 x 4 KiB, room left 240 KiB, entered 10736 every time: no failure.
- Why the room can grow (measured with the new view line): (1) an allocator can return memory to the kernel after the
  limit is set: the plain pass measured 640 KiB LESS at the report than at the call; (2) the ballast took only 1 MiB and
  4 KiB blocks with a 4096-block cap, so kept large blocks under 1 MiB (the heap keeps up to 4 per thread, D-3.13) and
  free slab room beyond 16 MiB stayed usable; (3) giving back 64 blocks of mixed sizes returned several slabs (seen when
  a ladder ballast was tried: tsearch then never bit). Which of these fired in mei's CI gate is NOT PROVEN (the failure did
  not reproduce in a 6-CPU slot on one NUMA node's CPUs; CI ran 24 test threads). Nothing points at Liella's hsearch or
  heap: every failing run ended in exit 14 with the calls reporting success correctly.

## The fix (crates/boundary/tests/search_nomem.rs)
- `limit_now`: first limit size + 1 MiB; ballast at every power of two from 1 MiB down to 4 KiB, each until failure (cap
  65536 blocks, reserved before the limit); give back at most 64 page-sized blocks; then `tighten()`.
- `tighten()`: limit = size measured now + 16 KiB (SLACK). Called right before every run of calls under test (both hsearch
  ENTER loops, tsearch loop, qsort). Never from host RAM or CPU count.
- A failing child prints `search_nomem view: code N; size before, first limit, ballast blocks, size after ballast, limit
  at the call, size now, mappings` (read from pre-opened /proc files into stack buffers: no allocation under the limit).
- No helper reused from ENOMEM-REALPATH-INTERMITTENT: its fix was a mallinfo2 window guard, not an RLIMIT_AS helper.

## Results
- mei try2 (8 runs x 2 configurations): 16/16 pass. clippy + static clean (job search-nomem-mei-lint, before the last edit;
  rerun at the head of the gate job).
- PENDING (started detached, collect with `~/bin/jobwait <label>` on the host): mei: search-nomem-mei-gate (clippy,
  static, `cargo xtask affected --base origin/next`), search-nomem-mei-loop50 (50 runs x 2 configs, 8 parallel),
  search-nomem-mei-plants; keke: search-nomem-mei-loop50, search-nomem-mei-plants.
- Plants (~/work/search-nomem-mei/plants.txt on both hosts): grow-ignores-nomem (core/src/search.rs growth failure arm
  ignored), hcreate-ignores-nomem (search_c.rs hcreate ignores a failed segment). Expected CAUGHT; not yet read.

## Next step
Read the five jobs. If any loop fails, its view line says which room was left (size now vs limit at the call). Then land.
Plan rows: D-14.1, D-14.3, D-14.4, D-3.13. Not to redo: the old test's mei view (above).
