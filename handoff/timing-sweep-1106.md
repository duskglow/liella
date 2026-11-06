# TIMING-SWEEP-1106 (PENDING JOBS: timing runs started on both timing hosts, results not yet tabled)

State: branch task/timing-sweep-1106, base origin/next 4d454f06. No code change. Products staged from this tree.

## Done
- Stage: shiki role sh1 (synced head 4d454f06), job `timing-sweep-1106-stage` rc 0 (183 s):
  `cargo xtask bench --stage ~/work/TIMING-SWEEP-1106/stage.tar --only <17 non-libm groups>` (the same 17 groups as
  1105: aio, fstree, hot, locale, malloc, memref, netdb, nscache, opt, patterns, process, services, startup, stdio,
  thread, time, unmeasured). stage.tar sha256 2f8b50a6...5083, copied to both timing hosts (checked by sha256).
- XTASK-BENCH-OUTPUTS has not landed at 4d454f06 (no `bench-out` in xtask/src): the tables go to
  target/liella/bench/; run.sh copies either location to out/.
- Bookings: ops #75 nico (Ivy Bridge E3-1220L v2) and #76 nozomi (Intel N100, Alder Lake-N, 4 E-cores, AVX2; its
  verdicts are for that core type), 180 min each, booked 07:15Z.
- Timing runs started 07:19Z, `~/bin/timing` called directly (queues until the host drains):
  nico:timing-sweep-1106-nico and nozomi:timing-sweep-1106-nozomi, each running
  `bash ~/work/TIMING-SWEEP-1106/run.sh` (copy: handoff/timing-sweep-1106/run.sh):
  `./xtask-bin bench --no-build --staged --compare --counters --only <17 groups>` in ~/work/TIMING-SWEEP-1106/tree.
  Output ~/work/TIMING-SWEEP-1106/out/{bench.log (last line rc=N), counters.tsv, rounds.tsv, ...}. 1105 took over
  60 min per host for the same groups.
- Analyzer: handoff/timing-sweep-1106/analyze.py (the 1105 analyzer plus 1105 time columns, now/1105 change, the
  1106B re-time marks and lists a/b/c; wordexp rows in their own section). Checked on the 1105 run's own tsv: it
  reproduces the 1105 table and verdict counts (now/1105 = 1.000 +- 0.002 rounding), all 38 re-time rows found.
- Landings between the two builds: handoff/timing-sweep-1106/landings-f2157504..4d454f06.txt (55 first-parent merges).

## Exact next step (continuation, about 12 calls)
1. Re-attach: `ssh nico '~/bin/jobwait timing-sweep-1106-nico'`, `ssh nozomi '~/bin/jobwait timing-sweep-1106-nozomi'`
   (tools/vm_ssh.sh). As soon as each ends: `ops timing-cancel 75` (nico) / `ops timing-cancel 76` (nozomi).
   rc=1 on nico may again be the nscache first-call budget (as in 1105); the tsv files are still complete then.
2. Copy out/counters.tsv and out/rounds.tsv from nico into <dir>/ivb and from nozomi into <dir>/n100; run
   `python3 -I handoff/timing-sweep-1106/analyze.py <dir> results/timing-sweep-1105-table.md <1106b table.md>`
   (1106B table: handoff/timing-coverage-1106b/table.md on origin/next once landed, else on
   origin/task/timing-coverage-1106b). It writes <dir>/table.md and <dir>/lists.md.
3. For each REGRESSION row: `git log --oneline f2157504..4d454f06 -- <files on the row's path>` (codemap
   docs/codemap/symbols.tsv) and name the most likely landing. IMPROVED rows: name the open stories from the 1106B
   table's story column / `ops status`.
4. Write results/timing-sweep-1106-table.md (CPU classes only, no host names), update this file, `cargo xtask static`
   on the role, push.

## Not to redo
The stage build and the copies to the timing hosts. Do not start a second bench while the runs are alive.
Plan rows: V-4 handoff/benchmarks-v4.md, docs/benchmarks.md section 6; method as handoff/timing-sweep-1105.md.
