# SIGNAL-TEST-THREAD-AUDIT (handoff, one page; -2 continuation)

State: branch task/signal-test-thread-audit-2 (stacks on task/signal-test-thread-audit). Code commits: b209bb3ed (helper +
5 s20 cases), adb39a373, d15e5ab39 (lint), 389cfddab (s18_4_g_2 + D-X.42 row), 90d55b865 (docs/plants.md: crates/core is
package `liella`), bf6dc5bb8 (regex_reentry, regex_dfa_signal_fork); handoff on top. Jobs on mei role s1 (labels below).

Decision in force: **D-X.42** (plan/07-decisions/x-implementation-decisions.md, written this run): a re-executed case that
reads what a process-directed signal's handler set runs its body through `single_threaded`
(crates/core/tests/support/single_thread.rs: library `fork`, parent waits, exit status = body's result) and checks
`sole_thread()` at the read, or before it starts its own thread when it joins one (a joined thread may still be listed in
/proc/self/task for a moment). No sleeps, polls or retries. D-X.40 (POPEN-SYSTEM-SIGCHLD-RACE branch) keeps
`system_signal_state_during_and_after`.

## Audit (alone/alone_ok re-executed cases that count a process-directed signal)
| case | verdict | why |
|---|---|---|
| s20_signals_symbols sym_kill, sym_sigqueue | EXPOSED, FIXED | kill/sigqueue to own pid; libtest main thread could take it; was a 1 s wall-clock poll (wait_hits) |
| s20_signals_clauses s20_4_c_1, s20_4_c_2, s20_4_c_4 | EXPOSED, FIXED | kill(me)/kill(0)/kill(-pgid)/killpg/sigqueue, same 1 s poll |
| s18_processes_clauses s18_4_g_2 (plain, masked) | EXPOSED, FIXED (this run) | kill(0, SIGUSR2) from a helper thread; the leader (libtest's thread) could run the handler after the case closed the pipe; masked only by 200 ms+ sleeps |
| regex_reentry, regex_dfa_signal_fork (signal case) | EXPOSED (vacuity), FIXED (this run) | ITIMER_REAL SIGALRM to the process; a handler on libtest's thread beside the search counted as "mid-search", so the case could pass with no re-entry at all |
| popen_system_eintr system_signal_state_during_and_after | EXPOSED, D-X.40 | fixed on the POPEN-SYSTEM-SIGCHLD-RACE branch (read with git show per #3998); not touched here |
| s20_signals_clauses s20_2_c_1_5 | WEAK, NOT FIXED | negative check (no SIGCHLD on stop) after a 100 ms sleep: thread choice can only hide a defect, never fail; better: fork child + waitpid(WUNTRACED) |
| eintr_system/pclose_retries_the_wait, s18_6_f_0 | NOT EXPOSED | tgkill to the calling thread |
| system_two_threads_share_the_ignore | NOT EXPOSED | SIGINT ignored at generation; INFORMATIONAL: relies on `sleep 1` outlasting the call |
| s18_3_c_0, s18_4_c_2_3, s18_7_c_1, s18_7_t_0 | NOT EXPOSED | raise (thread-directed) or no handler state read |
| s23_time_symbols alarm/ualarm/setitimer | NOT EXPOSED | read timer values only; INFORMATIONAL: wall-clock ranges |
| as_safety | NOT EXPOSED | its handler is driven by raise(10) (thread-directed) in its own fork child; alarm() is a hang guard |
| s19_terminals_* alarm(60)/alarm(10) | NOT EXPOSED | hang guards (default SIGALRM action), no handler state read |
| sigev_thread | NOT EXPOSED | SIGEV_THREAD/none timers, no signal handler state; runs in its own alone copy |

## Proof (mei, role s1)
- Loop `signal-test-thread-audit-2-loop2` (loop.sh: builds AFTER = branch and BEFORE = base test files in the role, copies
  binaries to /mnt/zram, runs each case 2000x at 32 parallel, interleaved per case; host load 24-72 on 48 threads, no burners):
  BEFORE 0/2000 and AFTER 0/2000 for all six s18/s20 cases (each run checked to execute 1 test). The BEFORE versions
  hid the race behind a 1 s poll / 200 ms sleeps, so the loop cannot show it; what the fix removes is the wall-clock
  wait and the wrong-thread pass, which the plants prove deterministically. Regex cases: no loop (budget); plants only.
- Plants `signal-test-thread-audit-2-plants` (7: each s18/s20 case moved back in place into the threaded copy, helper
  child given a second thread) and `-plants2` (2: regex cases in place). Gate: `signal-test-thread-audit-2-gate`.
- RESULTS (code head bf6dc5bb8): gate rc 0 "affected: ok", peak 3.6G; plants 7/7 CAUGHT and 2/2 CAUGHT, 0 WARN; lint2 rc 0.
  Superseded: -lint (inherited head, green, before the s18 change), -loop (script bug, rc 1, role tree untouched).

## Not to redo
Audit above is complete for files using SIGALRM/ITIMER/kill/sigqueue with alone copies. Do not return to bounded waits.

## Next step
Land after POPEN-SYSTEM-SIGCHLD-RACE; then (optional) move D-X.40's fork helper onto single_thread.rs and fix
s20_2_c_1_5 with fork + waitpid(WUNTRACED).
