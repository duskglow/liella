# PERF-FIRSTCALL-MKTIME-SNPRINTF: first_mktime_new_york and first_snprintf regressions

State: branch task/perf-firstcall-mktime-snprintf (from origin/next 6faf9548). Code commits 26b3413e (float frame),
822ff3fa (zone gate started at load), 96f3a6db (test bound, plants file); then this handoff. Role shiki:~/w/sh1.
Plan rows: D-7.14, D-23.6 (TSC gate), D-8.11 (exact decimal). No new D row: the brief reserved no id, and the
mktime change needs a ruling first (below).

## Cause (step 1)
Not measured on the parent and child of a86c9677 / bf079683: building other people's old commits is an "old
checkout of others' work", which thread_common forbids. The causes were found on the current head instead
(strace and `perf record -e page-faults -d` of cbench_opt's cold child, shiki = Haswell-EP host):
- **first_mktime_new_york**: `localtime_r` uses `time_compat::current` (no gate), so the cold child's first
  `mktime` is the zone TSC gate's first use (`select` -> `TscGate::fresh(.., sys.tsc(), ..)`, added by
  COLDSTART-SECOND-LATENCY-3/4 = a86c9677). `Sys::tsc` with `TSC_STATE` unknown runs `tsc_probe`:
  `prctl(PR_GET_TSC)` (the only syscall in that window) plus one text fault on `tsc_probe` (`#[cold]`, its own page).
  About 4 us here (4736 ns vs 772 ns for glibc).
- **first_snprintf**: `format_float` had a 24,984-byte frame (6 x 0x1000 + 0x198: `decimal_general`'s
  `exact()` -> `Decimal<MAX_DIGITS>` (12 KiB) plus the rounded copy in `decimal_styles::<MAX_DIGITS>`, both
  inlined). Its prologue probes six stack pages, i.e. six page faults on the first float conversion, even for `%.2f`,
  which takes the scaled path. Which landing made those pages newly untouched (deeper startup stack before, or a
  changed inlining) was NOT attributed to a commit (same rule). The frame cost exists on head either way.

## Change
1. `format.rs`: the `exact()` branch is `decimal_exact`, `#[inline(never)]`. The `format_float` frame is now 0x1a8 bytes.
   Output is unchanged.
2. `time_compat::add`: a file zone reads `sys.tsc()` before its clock read and starts the gate
   (`TscGate::start`, new in recheck.rs, full interval). So the probe runs on the zone load (which already does
   openat/fstat/read/close), and the first `mktime` takes the gate with no probe and no clock read. A rule zone
   reads no counter.

## Measurements (shiki, Haswell-EP, `bench --compare --counters --runs 5 --rows first_*,second_*`, ns, host not booked)
| row | glibc | head | fix | ratio head -> fix |
|---|---:|---:|---:|---|
| first_snprintf | 15825 / 16065 | 23495 | 12348 | 1.48 -> 0.77 |
| first_mktime_new_york | 772 / 755 | 4736 | 702 | 6.13 -> 0.93 |
| second_mktime_new_york | 594 / 589 | 626 | 635 | 1.05 -> 1.08 (spread) |
| first_localtime_r_new_york | 26834 / 26502 | 47639 | 50507 | 1.78 -> 1.91 |
| first_strftime_c | 1770 / 1742 | 1698 | 1533 | 0.96 -> 0.88 |
1106E (brief): first_mktime 4.89 (Ivy Bridge) / 4.80 (N100), 1106 0.91 / 0.99; first_snprintf 1.45 / 1.26, 1106 1.16 / 0.99.
Cold rows have no instruction counters (they run in a fresh process). The fault counts come from the new tests:
first `%.2f` 4 faults (head: 6 more stack faults by the trace above), first mktime after localtime_r 0 (head: 1).
Logs: shiki:~/work/PERF-FIRSTCALL-MKTIME-SNPRINTF/{head,fix}/bench.log, pf-head.txt.

## RULING NEEDED
- **Where the zone's one-time TSC query goes** (D-23.6 / D-7.14). (A, on the branch) at the zone file load:
  first_mktime 4736 -> 702 ns, first_localtime_r +2.9 us (47.6 -> 50.5; the probe's syscall + its cold text page).
  A program that calls only the `_r` forms now pays one `prctl` it did not pay before. (B, head) at the first gate use:
  first_mktime stays at ~6x. A third option, not measured: start the gate only from the third implicit tzset, so
  calls 1-2 read the coarse clock (second_mktime would lose the gate's ~25 ns N100 gain) and short-lived programs
  never probe. Recommend (A): the query belongs on the path that already makes the load's syscalls, the
  first+second mktime pair gets cheaper overall, and putting `tsc_probe`/`prctl_get_int` on the hot-text list
  should shrink the +2.9 us (next step). If (A) stands, amend D-23.6 with one sentence (the gate starts at load).

## Not done
- Step 4 timing (booked Ivy Bridge + N100 run): not started. A booking waits 30-60 min, which did not fit this run's budget.
- Hot-text placement of `tsc_probe` (would remove the text fault from the load path): not tried.

## Next step
Ruling on (A). Then: stage head with `cargo xtask bench --stage --only cbench_opt` and time the cold rows on an Ivy
Bridge timing host and the N100 host (`~/bin/timing`), with the 1106E numbers above beside them. Optionally move
`tsc_probe` into the hot group and recount first_localtime_r.

## Jobs (shiki)
- pfms-count-head rc 0 (head counts above); pfms-check rc 1 (SUPERSEDED: FLOAT_BOUND 3 too tight, measured 4, set to 6;
  clippy, static and the unit tests passed in it; its count run gave the "fix" column).
- pfms-gate rc 0 on 96f3a6db (`cargo xtask clippy`, `cargo xtask static`, `affected --run --base origin/next`):
  6 suites ok, 4110 passed, 0 failed, 601 ignored; conformance 4/4; peak 12G. Later commits change only handoff/.
- pfms-plants rc 1 (SUPERSEDED): exact-inlined CAUGHT, start-no-allow CAUGHT, gate-not-started SURVIVED. The
  counter is still queried at load in that mutant, so the first mktime takes no fault. It is now `equivalent:`
  (reason in the plants file), and plant no-probe-at-load (drops the load-time query) was added.
- pfms-plants-2 rc 0: the 4-line plants file handoff/perf-firstcall-mktime-snprintf.plants: 3 CAUGHT
  (exact-inlined, no-probe-at-load, start-no-allow), 0 SURVIVED, 1 EQUIVALENT (gate-not-started), no WARN lines.
- Static log shows one warning that predates this branch (unused `AcqRel` import, crates/boundary/src/timer.rs:93).
