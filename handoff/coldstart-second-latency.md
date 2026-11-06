# COLDSTART-SECOND-LATENCY(-3): second_setlocale_en_US / second_mktime_new_york time without instructions

State: branch task/coldstart-second-latency-3 (from -2's branch). Code commit 48843455 (proposal (a)), then this handoff.
Result of -3: INCOMPLETE. (a) is implemented, gated and planted green. The N100 / Ivy Bridge timing that judges it
is queued (nozomi:coldstart-second-latency-3-time-n100, nico:coldstart-second-latency-3-time-ivb). Plan rows:
D-23.6 (amended), D-7.14 (amended: one TSC rule for gettext and the zone check). Plan: handoff/coldstart-plan-1106/.

## Step 1: N100 counters (before the change)
nozomi:csl2-pmc-n100 (-2) returned rc 0 but every counter read 0. Cause: the NMI watchdog holds one counter, so a pinned
group of cycles + instructions + 6 raw events cannot be scheduled on Gracemont, and a group that never runs reads
offset 0 through rdpmc. Fix: at most 5 raw events per set, a `perf stat` group check that fails the run, and a check
for all-zero cycles (`~/work/COLDSTART-SECOND-LATENCY-2/run2.sh` on nozomi). Rerun: nozomi:coldstart-second-latency-3-pmc-n100,
310 processes per side. Medians, net of the empty window N2 (Liella 170 / glibc 192 cycles):
| window | cycles L / g | instr L / g | L2 hits L / g | load-stall cyc L / g | icache miss L / g | ld_head L1 miss L / g | dTLB walks L / g |
|---|---:|---:|---:|---:|---:|---:|---:|
| S2 setlocale | 769 / 404 | 165 / 182 | 5 / ~0 | 128 / ~0 | 59 / 34 | 55 / 2 | 1 / 0 |
| M2 mktime | 1285 / 1134 | 1043 / 1295 | 13 / 4 | 161 / 31 | 106 / 73 | 90 / 21 | 0 / 0 |
Reading: on the N100 both second calls lose time to data loads served from L2 (L1D misses), and setlocale also to
+25 icache misses; page walks are about 1 per setlocale and 0 per mktime. The setlocale gap (+365 cycles) is the larger
one on this core and is a data-side problem, not page walks.

## Step 2: (a) done
`recheck::TscGate` (D-7.14's rule: `tsc_holds`/`tsc_allow`, 2^28 ticks) sits in front of the zone's once-a-second
`LastCheck` on `time_compat::select`'s lock-free hot path. `TZ` and the generation are compared first, so a changed `TZ`
is seen at once. No counter, a counter that went backwards, and a forgotten check all read the clock. Tests:
`recheck::tests::tsc_gate_in_front_of_a_check`, plus `reload` `an_implicit_selection_sees_a_replaced_zone_file_after_the_interval` and
`a_changed_tz_is_seen_by_the_next_implicit_selection`.

## Step 3: counts (Broadwell-EP build host, loaded; kanon:coldstart-second-latency-3-counts2)
Probe M2 instructions 1228 -> 1165 (-63; glibc 1522). L1D misses 54.8 -> 54 (no change), L1I about the same. Cycles on
this loaded host moved with the empty window (N2 1696 -> 1583) and settle nothing. cbench_opt compare, loaded host:
second_mktime 584 vs glibc 736 ns, second_setlocale 217 vs 340 ns. So on Broadwell (a) removes instructions but not
the L1D misses: the +19 misses are the zone's spread (proposal (b)) and the stack, not the vvar line.

## Next step (in order)
1. Read nozomi:coldstart-second-latency-3-time-n100 (`~/logs/<label>.log`: the cbench_opt `--compare` table, then the
   after-probe sets A/B; subtract N2) and nico:coldstart-second-latency-3-time-ivb. second_mktime ratio before (-1, N100):
   630 vs 485 ns (1.30x). If the N100 is not slower than before -> COMPLETE (land (a)). If it measured worse, revert 48843455
   (keep the tests) and record a NEGATIVE row.
2. Coordinator step 4: if N100 mktime is still above 1.10x, do (b), one block per zone, as its own commit with its own
   counts and timing (the counters above point there: +9 L2 hits per mktime).
3. (c) setlocale: the N100 now shows the evidence: L1D misses served from L2 (+5 L2 hits, +128 load-stall cycles,
   ld_head 55 vs 2), 1 dTLB walk, +25 icache misses. Not started (coordinator rule); it is the bigger gap on the N100.

## Not to redo
-1/-2 probes, traces and re-times; the N100 pre-change counters above; the event-group limit (5 raw + cycles + instr).

## Report
- Commits: 48843455 (gate + tests + D-23.6/D-7.14 amendments), this handoff + report json.
- Jobs on the final code head (kanon:kn1): coldstart-second-latency-3-lint (clippy, static, recheck unit + reload, rc 0),
  coldstart-second-latency-3-gate (affected --base origin/next: 7 suites, 4681 passed, 0 failed, 3 conformance;
  peak 12G, rc 0), coldstart-second-latency-3-plants (5 CAUGHT, 0 SURVIVED, 0 WARN), coldstart-second-latency-3-counts2 (rc 0).
  N100: coldstart-second-latency-3-pmc-n100 (rc 0, pre-change data). Queued: nozomi:...-time-n100, nico:...-time-ivb.
- Superseded: kanon:coldstart-second-latency-3-counts (rc 1: I named a row, `second_mktime_new_york`, as a bench; the
  rows are in cbench_opt). nozomi:csl2-pmc-n100 (-2; all zero, see step 1).
- Process slip: one call held an empty heredoc (`python3 - <<'X'` with no body, a no-op); a remote `cmd && setsid ... &`
  held the ssh open until the harness stopped it (the remote job finished rc 0).
- RULING NEEDED: none. Refusals: none.
