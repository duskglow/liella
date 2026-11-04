# FUZZ-COVERAGE-STATUS (branch task/fuzz-coverage-status)

State: `cargo xtask fuzz` keeps the last libFuzzer status line per target (across chunks, one fold `keep_last` shared by the run
and its unit tests), prints `<target> cov=<n> ft=<n> corpus=<n> execs=<n>` at the end (also after a failing target), and
`--summary <file>` writes the rows as TSV. No status line -> `none`, never 0. `docs/fuzzing.md` describes it.
Proof: 4 unit tests in xtask/src/fuzz.rs (last line, crash run, fork-mode line, none); plants first-status-line and none-as-zero CAUGHT
(jobs fuzz-coverage-status-plants, plants2); clippy + static + fuzz tests green (gate3).
NOT DONE: the rerun of all 20 targets (30 min each, about 10 h of fuzzing) that fills the coverage column of docs/fuzzing.md. It is a
long detached job, outside this budget. Next step: on mei in RAM, `cargo xtask fuzz --all` is capped at 140 min per invocation, so run
`cargo xtask fuzz <t> --time 30 --summary <f>` per target (two queued jobs of 10 as in the PRE14 campaign), then add the cov/ft columns.
Gate notes: gate (cargo xtask affected --run, no --base) escalated to FULL and was killed (rc 143); gate2 with --base origin/next ran
all tests green (10762 passed, conformance 110) but its clippy step failed on the first draft (fixed; gate3 green). The docs commit and
the final code commit came after gate2's tree; gate3 covers clippy/static/fuzz tests on the final head.

## FUZZ-COVERAGE-STATUS-2 (branch task/fuzz-coverage-status-2)

Done: the rerun. Job `fuzz-coverage-status-2-run2` on shiki (rc 0, 3740 s): all 26 targets of `xtask fuzz --list`, 30 min each, 13 at a time,
in RAM (`/dev/shm/claude/FUZZ-COVERAGE-STATUS-2/run2`, corpus empty + repo seeds), `--summary` per target; the cov/ft/corpus/execs table
is now in docs/fuzzing.md ("Coverage rerun"). The script is `~/work/fuzz-coverage-status-2/run.sh`; summary `summary.tsv` and the two
artifacts (`artifacts/`) are in that directory on shiki. The first launch (`fuzz-coverage-status-2-run`) was killed at 2 min by me (it was
attached to my ssh session); it is SUPERSEDED.
Findings (new fix stories, not fixed here; neither looks like a security weakness):
1. group: harness assertion `each GID once` (fuzz/fuzz_targets/group.rs:36) failed: `users::group_list(text, user, gid, ..)` returned 5 entries,
   4 distinct. Artifact `crash-cecd3719...` (257 bytes, an /etc/group text with alice in root/users/wheel). Either group_list duplicates a
   GID or the harness expectation is wrong (spec: getgrouplist returns each group once is not stated in the clauses I checked; unchecked).
2. xdr_mem: libFuzzer out-of-memory (2055 MB > 2048) after 150.9 M executions. Artifact `oom-da39a3ee...` is the empty input and runs clean
   alone (`--repro` rc 0), so memory grows across executions: a leak in the target or harness. Needs a fix story that finds the leak.
Not done: nothing from the original brief; the campaign's four earlier time-outs were not replayed here.
