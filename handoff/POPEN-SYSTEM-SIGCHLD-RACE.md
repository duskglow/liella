# POPEN-SYSTEM-SIGCHLD-RACE: system_signal_state_during_and_after hit-count race

State: branch `task/popen-system-sigchld-race`, code head 8a140c37c (tested). A later commit adds only this handoff and the report json.
Decision in force: D-X.40 (plan/07-decisions/x-implementation-decisions.md).

## Finding
The defect was in the test, not in `system`. POSIX blocks SIGCHLD only in the thread that calls `system`. In the
re-executed test binary, libtest's main thread kept SIGCHLD unblocked. The shell's SIGCHLD could go to that thread
while `system` was still waiting, and the handler could still be running when the case read `HITS`, so it read
`hits=0`. With `system`'s mask and dispositions correct, the failing line was
`after 0 int=handler quit=handler chld=handler blocked=0 hits=0`. Mask, dispositions and return value were all correct.

## Fix
The case runs inside a child of `fork`, which has only one thread. The pending SIGCHLD can then be delivered only to
the caller, when `system` restores the mask, so `hits=1` is settled by the time it is read. The child prints
`R:alone <gettid()==getpid()>` and the parent asserts it is true. The checks on the mask during the call and on the
mask, dispositions and delivery after it are unchanged. Rejected: polling HITS with a bound. A handler that ran on
another thread while `system` was waiting would pass, so "delivered after" would go untested in those runs.

## Proof (keke, role n3; loop = loop.sh in the task scratch: build the test once, copy it to RAM, run the case N times next to 2*nproc `yes` burners)
- before (origin/next test): `psr-loop-before` fails=5 of 2000, load 27 (nproc 7 in the job cpuset, 14 burners). Failure line as above.
- after (8a140c37c): `psr-loop-after` fails=0 of 2000, load 31-39.
- plants (`handoff/POPEN-SYSTEM-SIGCHLD-RACE-plants.txt`, `psr-plants`): mask-not-restored CAUGHT,
  dispositions-not-restored CAUGHT; 0 SURVIVED, 0 WARN.
- gate `psr-gate`: cargo xtask clippy, static, affected --run --base origin/next: rc 0, "affected: ok", peak 1.3G.

## Open questions / rulings
None.

## Not to redo
The diagnosis is confirmed. Do not move this back to a bounded wait on HITS (see Fix).

## Next step
Land. Possible follow-up: other `alone_ok` cases that install a SIGCHLD or other process-directed signal handler and
read its count afterwards may have the same libtest-main-thread exposure. `eintr_*` targets its signal at one thread
with tgkill, so it is not exposed. Not audited beyond this file.
