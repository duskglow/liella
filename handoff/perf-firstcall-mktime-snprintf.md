# PERF-FIRSTCALL-MKTIME-SNPRINTF(-2): first_mktime_new_york and first_snprintf regressions

State: branch task/perf-firstcall-mktime-snprintf-2 (from origin/next 6faf9548). Code: 26b3413e (float frame),
822ff3fa (zone TSC gate started at load), 96f3a6db (fault-bound test), 4fbd59e4 (AcqRel import only for selftest),
43719b11 (D-23.6 amended, saturation NEGATIVE row); then this handoff. Role shiki:~/w/sh1 (Haswell-EP).
Decisions in force: D-23.6 (amended: the counter query at zone load, coordinator ruling #3698 option A, provisional
until timing, now timed below), D-7.14 (TSC gate), D-8.11 (exact decimal).

## Causes (run 1, kept for the record)
- first_mktime: the cold child's first `mktime` was the zone TSC gate's first use -> `tsc_probe` (`prctl(PR_GET_TSC)`).
  Fix: a file zone queries the counter and starts its gate when it is loaded (`time_compat::add`, `TscGate::start`).
- first_snprintf: `format_float` had a 24,984-byte frame (inlined `Decimal<MAX_DIGITS>` exact path): six stack-page
  faults on the first float conversion. Fix: `decimal_exact` is `#[inline(never)]`; frame 0x1a8 bytes.

## Timing (run 2, booked; Ivy Bridge = kotori, N100 = nozomi; `bench --no-build --compare --runs 7 --only cbench_opt`,
staged with `cargo xtask bench --stage`; A/B = pre-A tree 26b3413e (float fix only) vs head 43719b11, alternating, 2 rounds)
| row | 1106E IVB / N100 | pre-A IVB | head IVB | pre-A N100 | head N100 |
|---|---|---|---|---|---|
| first_mktime_new_york | 4.89 / 4.80 | 5.03-5.06 (9.0 us) | 0.86-0.87 (1.54 us) | 4.87-4.95 (4.4 us) | 0.98-0.99 (0.90 us) |
| first_localtime_r_new_york | n/a here (1106: 1.727 / 1.772) | 1.82-1.83 (88.9 us) | 1.96-1.97 (95.8 us) | 1.94-1.95 (44.4 us) | 2.01-2.03 (46.2 us) |
| first_snprintf | 1.45 / 1.26 | 0.70 | 0.70 | 0.64-0.65 | 0.60-0.61 |
| second_mktime_new_york | (1106: 1.256 / 1.150) | 1.23-1.24 | 1.20-1.21 | 1.38-1.43 | 1.42 |
| second_localtime_r_new_york | - | 0.92-0.93 | 0.94 | 0.87-0.88 | 0.82-0.83 |
Verdict on ruling #3698 (A): first_localtime_r +6.8 us IVB / +1.8 us N100; first_mktime -7.5 us / -3.5 us. In ratio
terms +0.14 / +0.07 against -4.2 / -3.9. The loss is smaller than the gain on both hosts, so A stands. A program that calls only the
`_r` forms pays the +6.8 us on Ivy Bridge. That is large for one prctl. Not attributed (see Open). second_mktime 1.2/1.4 is the
same before and after A (not this change). Logs: kotori/nozomi ~/logs/pfms2-time-<host>.log, pfms2-ab-<host>.log.

## Hot-text step (ruling item 1): NEGATIVE, not kept
`tsc_probe`, `liella_sys_prctl` and `liella_machine_rdtsc` were already in hot-text.list (position 489, gettext part).
A fast-profile `tools/hottext/regen.sh` on head moved them beside the zone load (321-323). The same run also moved the startup
memcpy/malloc_debug group behind `stdio_compat::load`. Shiki, untimed: first_localtime_r 1.79 -> 1.78, first_snprintf
0.75 -> 0.91, first_mktime 0.89 -> 1.02. Not committed. Row in docs/saturation/07. The list says "do not edit by hand", so a
targeted move needs regen.sh to support it (idea: a pin file of keys kept in place).

## Open
- Where the +6.8 us on Ivy Bridge first_localtime_r comes from (prctl cost under that host's mitigations, or a fault).
  Profile with `perf stat -e page-faults,syscalls` on a pre-A vs head cold child before designing anything.
- second_mktime_new_york 1.21 IVB / 1.42 N100 (slower before this story too): a separate story.

## Not to redo
Bisecting the parent commits (forbidden: old checkouts of others' work); a full regen of hot-text.list for this row.

## Next step
Land. Optional follow-ups: the two Open items.

## Jobs (run 2)
shiki: pfms2-lint rc 0 (inherited head: clippy + static clean, 1 warning AcqRel -> fixed), pfms2-regen-count rc 0
(hot-text experiment), pfms2-stage rc 1 (SUPERSEDED: `--only` not valid with --stage), pfms2-stage2 rc 0 (head tar),
pfms2-stage-prea rc 1 (SUPERSEDED: --base needs .git, the role has none), pfms2-stage-prea2 rc 0 (pre-A tar, role
re-synced to head afterwards), **pfms2-gate rc 0** on 43719b11 (clippy, static, affected --run --base origin/next:
8 suites ok, 7311 passed, 0 failed, 1132 ignored; conformance 6/6; peak 13G), **pfms2-plants rc 0** (4 plants: 3 CAUGHT,
1 EQUIVALENT, 0 SURVIVED, 0 WARN). Foreground `cargo clippy --workspace --all-targets --all-features -D warnings` rc 0.
kotori: **pfms2-time-kotori**, **pfms2-ab-kotori** rc 0. nozomi: **pfms2-time-nozomi**, **pfms2-ab-nozomi** rc 0.
Full report: handoff/reports/PERF-FIRSTCALL-MKTIME-SNPRINTF-2.json.
