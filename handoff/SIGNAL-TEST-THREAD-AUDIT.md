# SIGNAL-TEST-THREAD-AUDIT (handoff, one page)

State: branch task/signal-test-thread-audit; commits b209bb3ed (helper + 5 cases), adb39a373, d15e5ab39 (lint fixes);
this handoff on top. Status PENDING JOBS (mei role s2): signal-test-thread-audit-gate (clippy+static passed as
signal-test-thread-audit-lint3, then affected --base origin/next --run), -loop (2000 runs per changed case under the
host's own load, ~44 on 48 threads at start, no burners per developer-flow), -plants. Re-attach:
`ssh mei '~/bin/jobwait signal-test-thread-audit-gate signal-test-thread-audit-loop signal-test-thread-audit-plants'`;
loop results: `grep '^loop' ~/logs/signal-test-thread-audit-loop.log`. Superseded: -lint, -lint2 (clippy red, fixed).

Decision D-X.42 (reserved id; ROW NOT YET WRITTEN in plan/07-decisions, next step): a re-executed case that reads what a
handler of a process-directed signal set runs its body through `single_threaded` (crates/core/tests/support/single_thread.rs:
library `fork`, only the calling thread) and checks `sole_thread()` at the read; no sleeps or retries. POSIX kill/sigqueue:
when no other thread has the signal unblocked it is delivered to the sender before the call returns.

## Audit (alone/alone_ok re-executed cases that count a process-directed signal)
| case | verdict | why |
|---|---|---|
| s20_signals_symbols sym_kill, sym_sigqueue | EXPOSED, FIXED | kill/sigqueue to own pid; libtest main thread could take it; waited by a 1 s wall-clock poll (wait_hits) |
| s20_signals_clauses s20_4_c_1, s20_4_c_2, s20_4_c_4 | EXPOSED, FIXED | kill(me)/kill(0)/kill(-pgid)/killpg/sigqueue, same 1 s poll |
| popen_system_eintr system_signal_state_during_and_after | EXPOSED, NOT CHANGED | SIGCHLD count after system(): the D-X.40 case (brief); its fix is on the unmerged POPEN-SYSTEM-SIGCHLD-RACE branch, not read here (rules forbid reading others' branches) |
| s18_processes_clauses s18_4_g_2 (plain, masked) | EXPOSED, NOT FIXED (budget) | kill(0, SIGUSR2) from a helper thread; handler may run on libtest's thread after the case closed the pipe (200 ms+ margin). Next step: `alone_ok(..., move || single_threaded(...))` style + sole_thread at the read |
| s20_signals_clauses s20_2_c_1_5 | WEAK, NOT FIXED | negative check (no SIGCHLD on stop) after a 100 ms sleep: thread choice can only hide a defect, never fail; better: fork child + waitpid(WUNTRACED) |
| eintr_system/pclose_retries_the_wait, s18_6_f_0 | NOT EXPOSED | tgkill to the calling thread (thread-directed) |
| system_two_threads_share_the_ignore | NOT EXPOSED to thread choice | SIGINT ignored at generation, discarded on any thread; INFORMATIONAL: relies on `sleep 1` outlasting the short call |
| s18_3_c_0, s18_4_c_2_3, s18_7_c_1, s18_7_t_0 | NOT EXPOSED | raise (thread-directed) or no handler state read |
| s23_time_symbols alarm/ualarm/setitimer | NOT EXPOSED | read timer values only, no delivery; INFORMATIONAL: ualarm/getitimer windows are wall-clock ranges |
| regex_reentry, regex_dfa_signal_fork, as_safety, sigev_thread, s19 alarm | NOT AUDITED IN DEPTH | ITIMER_REAL stress / hang guards / SIGEV_THREAD; grep showed no post-call count read; next run should confirm |

## Proof
Plants (~/work/signal-test-thread-audit/plants.txt on the host, copy in this handoff's job): per fixed case the fork wrapper is
replaced by std::thread::scope (body in place on the threaded copy) and must fail via sole_thread; one plant gives the
helper's child a second thread. Loop: AFTER only (5 cases x 2000). BEFORE loop NOT RUN (budget); D-X.40's 5/2000 is the
evidence for the pattern. NOT VERIFIED until the jobs end.

Next step: collect the three jobs; write the D-X.42 row; fix s18_4_g_2 with the same helper (+plant, loop); land after
POPEN-SYSTEM-SIGCHLD-RACE and consider moving its fork helper onto single_thread.rs.
