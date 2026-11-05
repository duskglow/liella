# PTHREAD-ERRNO-UNTOUCHED (spec clause s22_n_2)

State: branch `task/pthread-errno-untouched` (from origin/next), code commit 6859810e; this note and
`handoff/reports/PTHREAD-ERRNO-UNTOUCHED.json` are in the commit after it. Role keke:~/w/n2.

## What was done
- Existing coverage before this change: `s22_n_2` already mapped to `s22_more` + `s22_sync`, but only one
  pthread row checked errno (pthread_create EAGAIN under RLIMIT_AS, with errno 0 as the sentinel), and
  `s22_sync` checks sem_trywait's EAGAIN. A pthread entry other than pthread_create that also stored its
  error in errno would have gone unnoticed, so this was not a tag-only fix.
- New `conformance/c/s22_errno.c`: errno = 4242 (not an error number) before each call, then it checks that
  each call returns its error number and that errno is still 4242: pthread_mutex_trylock on a locked mutex
  (EBUSY), pthread_join of self (EDEADLK), pthread_mutex_unlock on an unowned errorcheck mutex (EPERM),
  pthread_attr_setstacksize(PTHREAD_STACK_MIN - 1) (EINVAL), pthread_kill with signals 65 and -1
  (EINVAL), plus a trylock and unlock that succeed (0). Then sem_trywait on a zero semaphore must
  return -1 with errno EAGAIN. Each failure prints a `s22_errno: ...` line on stderr.
- `crates/core/tests/s22_threads_clauses.rs`: `s22_n_2` now runs `s22_errno` as well.

## Results
- glibc probe (Ubuntu GLIBC 2.39-0ubuntu8.9, keke): the program built against the system libc exits 0, so
  every row matches.
- `cargo xtask clippy` and `cargo xtask static`: clean on the role, run on the code commit.
- Gate `pthread-errno-untouched-gate` (direct tier, `--base origin/next`: 1 test target + 1 conformance
  program): rc 0, peak memory 1.2G, oom_kill=0.
- Plants `pthread-errno-untouched-plants` (RAM copy /dev/shm/claude/pthread-errno-untouched/plants-1,
  baseline rc 0, each plant run under `timeout 600`): rc 0.
  - a: `pthread_mutex_trylock` returns through `ret::errnum_errno` (also stores errno): CAUGHT rc=1.
  - b: `sem_trywait` returns through `ret::errnum` (returns 11, errno untouched): CAUGHT rc=1.
  - Both print WARN-NO-MSG. `cargo xtask affected --only conformance:...` shows only
    `FAIL conformance (1 programs)` and leaves out the program's stderr, so the `s22_errno:` line could
    not be matched. The baseline passed with the same command, so each failure comes from the plant.

## Open / next
- No rulings needed. Next step: CI and landing.
- Improvement: when a conformance program fails, `xtask affected` could print the last few lines of its
  stderr. Plant message checks could then match, and a red result would be easier to diagnose.
