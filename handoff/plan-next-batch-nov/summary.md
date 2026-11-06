# PLAN-NEXT-BATCH-NOV: summary

Five briefs, not ten: within this run's 40-call budget only five candidates had evidence I could check. The other candidates need a ruling, are already in flight, or are blocked (listed below). Padding the list would break the accuracy floor.

| id | one line | size | host | why now |
|---|---|---|---|---|
| ARCH-STRING-PORTABLE | portable SWAR versions of the scan/fold kernels (ledger A-8) | M | - | A-2 landed today; unlocks aarch64 work, no x86_64 count change |
| FUZZ-COVERAGE-STATUS | xtask fuzz keeps cov/ft per target; fills docs/fuzzing.md coverage | S | - | the only open column of the phase-16 fuzz evidence |
| BENCH-UNMEASURED-WIDEN | qsort at 5 element sizes, fts at 2 KiB, re-count after the qsort landings | S | keke | section 17's not-measured list; decides whether qsort/fts need stories |
| VALIDATION-ROWS-STALE | refresh T23/T34 (and other landed-fix rows) from the tree | S | - | phase-16 gate report reads these rows; T34 refresh was asked by PRE14-LIBC-FMOD |
| PERF-READDIR-ENTRY | readdir 54.3 vs 46.1 user instructions per entry | S | keke | measured 1.18 gap on a hot walk path; STACKS ON PERF-FSWALK-PHYS (same file) |

## RULING NEEDED (not briefed)

* **DL-THREAT-TEXT** (DL-0d): its prerequisite DL-0a has landed (DL-PROBE-SECURE-2), but it folds the owner-approved
  `plan/ph14/threat-model-draft.md` into `spec/security/threat-model.md`, and R14-15 (approval) is still open. Recommend
  that the owner approve the draft's B/C rules; then brief DL-THREAT-TEXT (S, Host -). It must land before DL-8.
* **T22 stdio needs the heap to print** (`validation/README.md`): "RULING NEEDED: assignment". This is a correctness gap
  (once the heap is exhausted nothing prints). Recommend assigning it as a stdio story after STDIO-SHORT-IO-5 lands
  (same files), using the row's own proposal: give the three standard streams static slots.
* **T24 `__nss_hostname_digits_dots` signature**: a spec gap ("pending"). Recommend a spec-gap ruling before any story.

## Checked and left out

* Already briefed or in flight: every PERF row of register sections 14, 15 and 17 except readdir; PRE14-BENCH-RERUN
  (ready); the PRE14 and DL-0a/0b/0c stories (landed); X-1..X-8 except X-1 (which overlaps PRE14-BENCH-RERUN).
* Blocked: math M6-7 and later (M6-4 and M6-6 are red); ARCH-TLS-LAYOUT (needs the phase-14 TLS story);
  ARCH-CSYS-CROSS (after ARCH-SYSCALLS, red); ARCH-REF-PROBE (needs rulings RL-2/RL-3); LIBMVEC-SCAN (waits for the
  owner's Devuan mirror; its state was not checked).
* fts per entry at 1.88x: PERF-FSWALK-PHYS is running.
* Not searched within the budget: the follow-up columns of all of `plan/07-decisions/` (only a keyword grep was run)
  and spec clauses that have no test. A next planning run should start with a script that lists clauses with no
  traced test.
