# PROFILING-GMON-CLAUSES handoff

State: branch task/profiling-gmon-clauses (cut from origin/next 773e1693); test-only story, no product code changed.

Done:
- crates/core/tests/s28_profiling_clauses.rs: s28_1_c_1, s28_1_c_2, s28_3_c_0, s28_1_c_4 specified (s28_1_c_3 untouched).
  monstartup runs in re-executed children (one-shot, and its ITIMER_PROF sampling would disturb s28_1_c_3).
- crates/boundary/tests/s28_profiling_symbols.rs: sym_monstartup, sym__mcleanup, sym___cyg_profile_func_enter/_exit.
- crates/core/tests/oracle/cyg_hooks.c: -finstrument-functions program, built static against host and Liella.

Probe-settled (glibc 2.39, static, on mei): -pg program exiting from main writes only `gmon.out`; with
GMON_OUT_PREFIX=p only `p.<pid>`. cyg_hooks.c: library hooks -> `r=85 errno=4242 enter=0 exit=0`, no stderr;
own hooks link without duplicate definition -> `enter=16 exit=15`. Liella matches both (dev4 run green).

Jobs on mei (test head 303126da, all rc 0, plants 4/4 CAUGHT): profiling-gmon-clauses-static (full-workspace clippy, xtask clippy, xtask static),
profiling-gmon-clauses-gate (affected --base origin/next --run), profiling-gmon-clauses-plants
(~/work/profiling-gmon-clauses/plants.sh 1, RAM copy /mnt/zram/claude/profiling-gmon-clauses/plants-1;
plants a record-drops, b prefix-ignored, c fentry-skips, d cyg-errno). Results: see the hand-back / jobs.tsv.
Superseded: dev, dev2, dev3, dev4 (development runs on older heads).

Open questions: none. Next step: coordinator reads the three jobs; if a plant shows WARN or SURVIVED, fix that test.
