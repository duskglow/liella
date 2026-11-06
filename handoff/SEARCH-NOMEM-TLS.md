# SEARCH-NOMEM-TLS: handoff (search_nomem child aborted "Unexpected TLS failure", interposed build)

Branch task/search-nomem-tls, cut from origin/next c101c4c6. Code head db0ec040 (93e33d33 fix + db0ec040 FS-ORDER
note); test only, no product change. Role mei:~/w/s2; keke loops/plants in keke:~/w/search-nomem-tls (own dir, synced
with sync_role.sh). Decisions in force: D-14.1, D-14.3, D-14.4 (unchanged).

## Cause (evidence, not hypothesis)
- Reproduced on keke (Zen 1, 7 CPUs): whole binary, exports interposed, 400 runs, 24 at once: 1 failure (the qsort test
  this time, so all three tests are exposed). mei 6-CPU slot: 0 in 400.
- Core of the failing child (apport, keke), `thread apply all bt`: the ABORTING thread is libtest's MAIN thread:
  `run_tests_console -> mpmc list::Channel::recv -> Context::new -> thread::current::init_current ->
  std::sys::thread_local::key::unix::fail -> abort`. The test thread was inside qsort under the limit. libtest spawns
  the test's thread and only then makes its first blocking recv; that first wait sets up std's current-thread handle
  (a pthread key value), the channel's waiter and its Context. When the test thread took the ballast first, that setup
  ran under the limit: Liella's interposed `pthread_setspecific` needs memory for the main thread's key area
  (crates/boundary/src/pthread.rs `setspecific`: `thread::specific()?`), returned ENOMEM, and std aborts on that.
- Not "test thread touches std after the limit": nothing in the bodies after `limit_now` touches std lazily.

## Fix (crates/boundary/tests/search_nomem.rs)
- `others_settled()`, called first in `limit_now` (so all three tests): waits until every other task in
  /proc/self/task is blocked in futex (`/proc/self/task/<tid>/syscall` starts with "202 "); 100000 polls of 100 us as a
  hang guard; exit code 3 with the view line if they never settle. Sound because the polling thread holds no lock, the
  result is never sent (child ends with exit_group), so a parked sibling stays parked.

## Results (final code head db0ec040)
- gate mei:search-nomem-tls-gate-2 rc 0 (clippy, static, affected --base origin/next; search_nomem selected; peak 1.3G).
- keke:search-nomem-tls-plants-2, plants file ~/work/search-nomem-tls/plants.txt: alloc-after-limit (qsort_body's 2 MiB
  vec moved after limit_now) and settle-wrong-syscall (202 -> 203). On 93e33d33 both CAUGHT (keke:search-nomem-tls-plants,
  WARN-DEV-SHM only); the -2 run is the final-head repeat.
- Loops on 93e33d33 (same code, comment differs): keke export 400/400, plain 100/100. Final-head loops running:
  keke:search-nomem-tls-loops-keke-2 (400 export + 100 plain), keke:search-nomem-tls-loop2000-keke-2 (2000 export:
  at the old 1/400 rate a clean 2000 has ~0.7% chance by luck, a clean 400 ~37%), mei:search-nomem-tls-loops-mei-2.
- SUPERSEDED: search-nomem-tls-gate (rc 1, fs-order static check on read_dir, fixed in db0ec040), search-nomem-tls-plants
  and search-nomem-tls-loops-keke (green, older head), search-nomem-tls-loop2000-keke (stopped, older head),
  search-nomem-tls-diag1 on both hosts (pre-fix evidence).

## Probe (black box, installed glibc 2.39 on keke; ~/work/search-nomem-tls/probe_setspecific.c)
- A new thread, RLIMIT_AS at its size + 64 KiB, malloc taken until it fails: `pthread_setspecific` on the FIRST key
  created returns 0; on the 40th key it returns ENOMEM (3/3 runs). Liella's returns ENOMEM on the main thread's first
  call for a low key (by the core and code reading; not probed against Liella directly). The spec §22.6 lists no
  failure for pthread_setspecific (POSIX allows ENOMEM).

## RULING NEEDED
- Liella's pthread_setspecific needs memory even for the first keys, where glibc's needs none (probe above); Rust
  std aborts the process when it fails, so a Rust program under memory pressure on Liella dies where it lives on glibc.
  Recommend a follow-up story: keep the values of the first keys in the thread block (no allocation), add the §22.6
  failure rule to the spec (spec gap), test it with this probe's shape.

## Next step
Read the three loop jobs and plants-2; all rc 0 -> land. Plan rows to read: D-14.1, D-14.3, D-14.4.
