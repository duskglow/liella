# DL-LIBC-EARLY-2B: regex_reentry red on DL-LIBC-EARLY-2 (CI train 597): attributed and fixed

State: branch `task/dl-libc-early-2b`, stacked on `task/dl-libc-early-2` (its head 47fa75c1d, not merged with next:
no merge was needed). Commits: 4a4c26bbf (the test fix), 423524359 (plants file), then this handoff and the report.
Decisions in force: D-14.12 (the regex scratch claim, unchanged), D-X.42 (sole_thread, unchanged). No new decision row:
this changes only how the test counts and schedules its own work (a local test detail).

## Verdict: DL-LIBC-EARLY-2 is innocent; the test depended on timing

CI's failing line: `runs 3 in 4.5 s, handler calls 400, mid-search 0, outer wrong 0, inner wrong 0`.

Mechanism. The old handler did the inner search on the first WORK = 200 signals wherever they landed, and counted
mid-search only when IN_SEARCH was set. Outside `regexec` the loop keeps SIGALRM blocked, so the only place outside a
search where a handler can run is the unblock just before a search (a signal left pending while blocked). When the
inner search takes longer than the 2 ms timer period (a loaded host: in CI 200 calls took about 4 s, about 20 ms each),
each handler return delivers the next pending signal at once, still at that unblock. One pending signal there
therefore starts a run that spends the whole 200-call budget outside any search: mid-search 0, hits 400 = 200 working +
200 empty. The numbers fit only this: 200 working calls, none seeing IN_SEARCH, can only be one run at one unblock.

## Evidence (all on mei, Zen 1 host, load 29-39 during the runs)

| run | build | result |
|---|---|---|
| dl-libc-early-2b-attr-next | origin/next 28a903a01, old test, 20 runs | 20/20 rc 0, mid-search 200 every run, ~0.75 s |
| dl-libc-early-2b-attr-e2 | -2 head 47fa75c1d, old test, 20 runs | 20/20 rc 0, mid-search 200 every run, ~0.75 s |
| dl-libc-early-2b-diag-next (part 1) | origin/next, whole `cargo test -p liella-boundary` | regex_reentry ok (0.86 s) inside the suite |
| dl-libc-early-2b-diag-next (part 2) | old test + a signal left pending at the first unblock, 5 runs | 5/5 pass, mid 199: no run here, since the inner search (~1.5 ms) is shorter than the period |
| dl-libc-early-2b-diag-cascade, casc-old | old test + pending signal + period 300 us (handler outlasts the period), 5 runs | **5/5 FAIL, mid-search 0, outer/inner wrong 0: CI's exact pattern** |
| dl-libc-early-2b-diag-cascade, casc-new | fixed test, same 300 us period, 5 runs | 5/5 pass, mid-search 200 |
| dl-libc-early-2b-attr-fixed | fixed test on this branch, 20 runs | 20/20 rc 0, mid-search 200 |

The `cargo test -p liella-boundary` suite run in the RAM copy of next had other failures (failinject_absent_from_products
and one binary's 5 tests): that copy had no `cargo xtask so` products. They come from how I set up the run, not from this story.
The brief's `cargo xtask affected --run --only liella-boundary` is refused by xtask (`--only` takes test targets,
`lib:<crate>`, `conformance:<p>`, not a crate; job dl-libc-early-2b-attr-suite rc 1). I ran the plain cargo suite instead.

## The fix (crates/boundary/tests/regex_reentry.rs)

- The handler runs the inner search only when it interrupted the outer one (IN_SEARCH set). WORK now counts those
  calls (MID), so no run of calls outside a search can spend the budget.
- The loop ends on `runs >= 3 && MID >= WORK`. The 30 s limit stays a hang guard only.
- The test now waits until SIGALRM is pending before its first unblock, so every run starts from the case that failed in CI.
- What it proves is unchanged: both answers stay right when `regexec` is re-entered. Plant `claim-always` (the scratch
  claim always succeeds) is CAUGHT: `outer wrong 1` (handoff/DL-LIBC-EARLY-2B-plants.txt).
- The period stays 2 ms on purpose. A period shorter than the handler would land all 200 inner searches at one point
  of the first outer search (one long run), and maybe before the outer search claims its scratch. That would weaken
  what the plant needs. The spread of landing points is part of what the test proves.

## Not to redo
The 20+20 attribution runs. The cascade reproduction (the variant files are in my scratch dir, ~/work/DL-LIBC-EARLY-2B/ on mei).

## Next step
Land with DL-LIBC-EARLY-2 (this branch carries it). Nothing else open.

## Report
Jobs on the final content: dl-libc-early-2b-clippy (clippy + static, rc 0), dl-libc-early-2b-plants (1 CAUGHT),
dl-libc-early-2b-attr-next, dl-libc-early-2b-attr-e2, dl-libc-early-2b-attr-fixed, dl-libc-early-2b-gate (direct tier, --base
origin/next). Diagnostic jobs (evidence, not proofs of the head): dl-libc-early-2b-diag-next, dl-libc-early-2b-diag-cascade.
Superseded: dl-libc-early-2b-attr-suite (rc 1: `--only liella-boundary` is not a valid --only name).
INFORMATIONAL: to read next's copy of the test for the diagnostic variant I used `git show origin/next:<test path>`. That
file is identical to the one on this branch before my change (-2 does not touch it).
