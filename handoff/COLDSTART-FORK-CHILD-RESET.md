# COLDSTART-FORK-CHILD-RESET: handoff (run 2, COLDSTART-FORK-CHILD-RESET-2)

Branch `task/coldstart-fork-child-reset-2` (stacked on next 41b8737b; next has moved since, no merge done). Role keke:~/w/n3.
Commits: run 1's "WIP: fork child resets skip unused subsystems and unchanged words" + import fix; run 2's "Test the fork
child reset and record D-18.8" (conformance/c/s18_fork_reset.c, wired into `sym_fork`; row D-18.8 in
plan/07-decisions/s18-processes.md) and this handoff.

## Change in force (D-18.8)
`fork.rs`: one `USED` word (`USED_AIO`, `USED_GAI`) read once by `child_reset`; `mark_used` on the first-use path (aio
`lock()`, `getaddrinfo_a` entry). The aio and gai hooks reset their locks always and their tables only when the bit is set.
Every other reset stores only differing values (`set_if_differs!`, compare-first `RawMutex::reset`, stream-lock reset);
nscache bumps `cred` only with a table attached.

## Results (run 2, head = the commit holding this file; code unchanged since 9dec619f)
- keke:cfcr2-check rc 0: `cargo xtask clippy`, `cargo xtask static`, s18_fork_reset passes.
- keke:cfcr2-gate rc 0 (direct tier, `--base origin/next`): 9790 passed, 0 failed, 1220 ignored; conformance 50/50; peak 12G.
- keke:cfcr2-plants rc 1 (SUPERSEDED): 11 CAUGHT, 1 SURVIVED (`stream-owner-kept`). Cause: the stream owner is only
  compared with the caller's thread id; a stale owner names a live parent thread, so dropping that store is visible only
  after thread-id reuse, which no deterministic test can force. Marked `equivalent:` (store kept as the defensive reset).
- keke:cfcr2-plants-2 rc 0: 11 CAUGHT, 0 SURVIVED, 1 EQUIVALENT, no WARN; same 12 plants, the last marked equivalent (plants file keke:~/work/COLDSTART-FORK-CHILD-RESET-2/plants.txt).
- keke:cfcr2-counts rc 0, per extra fork+_exit+waitpid ((10 forks - 1 fork)/9, perf stat -r 30, keke (Zen 1 host), minimal
  static probe forkn.c):
  | | user instr | kernel instr | page faults |
  |---|---|---|---|
  | before (41b8737b) | 6728 | 354122 | 29.1 |
  | after | 421 | 299183 | 15.1 |
  | glibc 2.39 dynamic | 275 | 283239 | 12.3 |
- Timing (staged cbench_process, before/after interleaved x3, Liella vs glibc in each run): nozomi:cfcr2-n100 (N100),
  nico:cfcr2-ivb (Ivy Bridge, E3-1220L v2). Logs `~/logs/cfcr2-n100.log`, `~/logs/cfcr2-ivb.log` on those hosts. Verdict:
  see the hand-back (if absent: not collected yet; read the fork_exit_wait / vfork_exit_wait rows per round).

## Open
- Parent side (writes after fork to still-shared pages: `heap::HEAP.park_others`, atfork handlers) not examined.
- Remaining gap to glibc: ~146 user instr, ~16k kernel instr, ~3 faults per fork.
- `RawMutex::reset` compare-first has no plant of its own (a lock held by another thread at fork for aio/gai is not tested).

## Not to redo
Step 0 (run 1), hook audit (run 1), the test, plants, counts above.

## Plan rows to read
D-18.6, D-18.8, D-23.3.
