# COLDSTART-FORK-CHILD-RESET: handoff (INCOMPLETE, budget)

Branch `task/coldstart-fork-child-reset` (stacked on next 41b8737b); code commit "WIP: fork child resets skip unused
subsystems and unchanged words", plus the commit adding this note and the stacks.rs import fix. Role keke:~/w/n2.

## Step 0 (done)
N100 re-time read from nozomi:CSP1106-n100 (`~/work/COLDSTART-PLAN-1106/`, DONE, staged commit 730f416b):
`cbench_process/fork_exit_wait` time 1.286x (132807.9 vs 126598.5 ns), so the code change is needed.
Probe deltas per extra fork (counts-n100.txt, (10 forks - 1)/9, N100): Liella +6.8k user / +255k kernel instr / +27 faults;
glibc +0.3k / +218k / +12.

## Cause found (code reading, NOT yet confirmed by a profile)
`gai_async::after_fork_child` cleared all 4096 `QUEUED` slots (32 KiB) and the `used` word of all 256 `GROUP`s on every fork,
and `aio` cleared its 256 `BUCKET`s: about 11 never-touched .bss pages faulted in per child, plus about 4.6k stores (that
roughly matches the +6.8k user instructions). Every `RawMutex::reset`, stream-lock reset (`reset_in_child` wrote every stream's
lock words, even unlocked ones), `swap`/`store` and the nscache `cred` bump also wrote shared copy-on-write pages.

## Change (D-18.8, reserved via `ops reserve`; the ROW IS NOT WRITTEN yet, see next step)
- `fork.rs`: one `USED` word (bits `USED_AIO`, `USED_GAI`) read once in `child_reset`; `mark_used` (load, `#[cold]` fetch_or)
  is set on the first-use path: aio `lock()` (before taking `LOCK`), `getaddrinfo_a` entry. The aio and gai hooks still reset
  their lock (and aio its `OWNER`) unconditionally (cheap compare), and skip the tables while the bit is clear.
- `set_if_differs!` macro (fork.rs); used in the thread, stacks, timer, aio, `__libc_single_threaded` resets; mqueue, spawn,
  timer `swap`s became load-then-store; `RawMutex::reset` writes only a non-zero state; stdio `reset_in_child` writes only the
  non-zero words; nscache bumps `cred` only when a table was attached (`mode != MODE_IDLE`; an idle cache attaches anew anyway).
- Parent side: not changed or checked (the brief asked for it; `heap::HEAP.park_others` and the atfork handlers were not looked at).

## Results
- keke n2: `cargo xtask clippy` rc=0, `cargo xtask static` rc=0 (before the handoff commit; the D-row was not added).
- Gate keke:gate-cfcr (`cargo xtask affected --base origin/next`, direct tier) started detached, pid 940995, log
  `keke:~/w/n2.gate-cfcr.log`. Result NOT collected.
- NOT DONE: new test, plants, counts, N100 timing, profile.

## Exact next step
1. Collect keke:gate-cfcr (`~/bin/test-summary ~/w/n2.gate-cfcr.log`).
2. Add the row `| D-18.8 | §18.2 | <status as the s18 file uses> | fork child reset: aio and getaddrinfo_a tables only after
   their first use (one USED word, bits set on the first-use path); every other reset stores only values that differ; nscache
   bumps cred only with a table attached |` to plan/07-decisions/s18-processes.md, then rerun static.
3. Add the test (spec §18.2): in a process that used a timer (SIGEV_THREAD), aio (one aio_write + aio_suspend) and stdio (a
   stream locked by another thread through `flockfile` at fork), the child: creates a timer, runs an aio request, and `ftrylockfile`
   succeeds on the stream; plus one getaddrinfo_a request in the child after one in the parent.
4. Plants (`cargo xtask plants`): drop each hook call in `child_reset` (timer, aio, gai, stdio); `mark_used` a no-op; `USED_AIO`
   mark removed from aio `lock()`; `set_if_differs!` store removed (always skip).
5. Counts: the per-fork probe (P vs PPPPPPPPPP) before/after/glibc on a build host; then N100 staged timing
   (`cargo xtask bench --stage` / `--staged`) of fork_exit_wait and vfork_exit_wait.

## Not to redo
Step 0 (above). The hook audit (above): thread/stacks/timer/mqueue/syslog/spawn/nscache/stdio hooks are cheap once compare-first;
only gai and aio have tables.

## Plan rows to read
D-18.6 (fork), D-23.3 (timers after fork).
