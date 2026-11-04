# PERF-FNMATCH-PATHNAME-3 handoff
Branch task/perf-fnmatch-pathname-3 (contains -2). Role keke:~/w/n3. Tests only; matcher unchanged.
State: the `bracket` survivor is caught by direct_brackets_after_a_first_close_and_runs_stop_at_metacharacters (`a[x]` vs `a[x]` must not match), added at 387feb1e after the plants2 run. The `question` plant is EQUIVALENT: a run only consumes `?` when the string holds `?` at the same place, and `?` matches a literal `?` (never `/`, never a leading period), so the result is unchanged; the plant is now named EQ_question and a non-equivalent `runstart` plant (run starts one string character early) replaces it, CAUGHT.
Plants (new RAM copy /dev/shm/claude/perf-fnmatch-pathname-3/plants-1, script ~/work/perf-fnmatch-pathname-3/plants.sh on keke): skipcmp, star, runstart, bracket, backslash, runend, firstclose CAUGHT; EQ_casefold, EQ_question, EQ_memo EQUIVALENT. Exit 0.
Gate: affected --run ok (0 suites: no code change vs base). clippy --workspace and cargo xtask static green.
Weak spot: the plants script prints the first line matching error|FAILED, which is a build warning (_IO_ferror); CAUGHT means a test failed, the exact test name is in p.log (not kept).
Counters/register: unchanged from -2 (nomatch 313, pathname 485, star 336, bracket 945); no code change, not re-measured.
Next: none.
