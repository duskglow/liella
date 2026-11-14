# TIMING-SWEEP-NATSUMI (table written; first Zen 3 sweep; 11-13 libm re-sweep below)

State: branch task/timing-sweep-natsumi-2 (continuation of task/timing-sweep-natsumi), base origin/next 66b8d3ba
(products built from it). No product code change; one analyzer script. Result: results/timing-sweep-zen3-1107-table.md
(summaries, slowest 20 per set, 1106E counts for orientation, full tables).

## TIMING-SWEEP-ZEN3-LIBM (11-13: second Zen 3 libm sweep, next bc146cce)
- Result: results/timing-sweep-zen3-libm-1113-table.md (analyzer handoff/timing-sweep-zen3-libm-1113-analyze.py:
  `python3 -I <it> <out-libm>/sweep.tsv results/timing-sweep-zen3-1107-table.md bc146cce 66b8d3ba`).
- Host check 03:54Z (read-only): perf_event_paranoid 1, governor performance x12, scaling_max 3.6 GHz, swap now OFF
  (`swapon --show` empty, 0 B total), GPU 0 % / 0 MiB, load ~1.0 from kworker/11:1+usb_hub_wq in D state at 0 % CPU
  (the same kworker state as 11-07; not touched). slots idle, no jobs queued.
- Stage: mei:tsz3libm-stage (role s2, synced clean at bc146cce), rc 0, 290 s: `cargo xtask bench --stage
  ~/work/TIMING-SWEEP-ZEN3-LIBM/stage.tar --only cbench_libm`, sha256 3fe7b8ab17f8...b86587, copied mei -> local ->
  natsumi, sha256 checked on arrival.
- Booking 184 (`ops timing-book natsumi 90 --story TIMING-SWEEP-ZEN3-LIBM`), active 04:01:42Z; cancelled by me at
  ~04:04Z right after the run.
- natsumi:tsz3libm-sweep, 04:01:49-04:02:44Z, rc 0 (`~/bin/timing tsz3libm-sweep bash
  ~/work/TIMING-SWEEP-ZEN3-LIBM/run.sh`; run.sh = `./xtask-bin sweep run --out <R>/out-libm --only cbench_libm` in the
  unpacked stage): `sweep: COMPLETE`, 176 ratio_host rows 28 faster / 7 parity / 141 slower (179 counted with the 3
  baselines: 28 / 10 / 141). Spread median 0.007, p90 0.036, max 0.409 (asinhl). Intruder log: no entry.
- Against 11-07 (same host, commit 66b8d3ba): median time ratio 6.19 -> 1.98, geomean 9.76 -> 3.25; 166 selected rows
  5/5/156 -> 28/7/141; 19 slower->faster, 4 slower->parity. Time ratio up > 5 % with instr unchanged: cexpl, cpowl,
  j0, tgammal, y1 (+6-14 %, one run each: recheck before reopening); y1f also moved in instructions (0.731 -> 0.822).
- Left on natsumi at hand-back: dirs ~/work/TIMING-SWEEP-NATSUMI (221M) and ~/work/TIMING-SWEEP-ZEN3-LIBM (89M:
  stage.tar, tree/, out-libm/ raw data), ~/bin tools, ~/logs. No processes, no crontab, no user timers of ours
  (the one user timer, launchpadlib-cache-clean, is the distribution's), booking cancelled, slots idle.

## TIMING-SWEEP-NATSUMI-2 (fixed the CI precheck red)
- CI precheck on 7d52b306 failed `xtask static: lab-names`: results/timing-sweep-zen3-1107-table.md lines 15 and 30
  cited handoff paths that contain the host name (the story is named after the host). Reproduced on mei s4 (static
  rc 1, the same 2 hits; clippy rc 0).
- Fix (no ALLOW entry, no LAB-ONLY marker: the results doc must not name hosts): analyzer moved to
  handoff/timing-sweep-zen3-1107-analyze.py (the timing-sweep-1105-analyze.py convention, named like the results
  table); the table now says "details in the sweep's handoff note, named in the analyzer's header", and that header
  (handoff/, may name hosts) names this file. No data changed.
- Lesson for briefs: a story named after a lab host puts that name in every handoff path a results doc might cite;
  name results-facing files by CPU class from the start.

## Runs (products of next 66b8d3ba, staged once on mei s3)
- Stage: mei:timing-sweep-natsumi-stage, rc 0 (196 s), ~/work/TIMING-SWEEP-NATSUMI/stage.tar on mei, sha256
  08381188503cc987fb79afba111daaffc869016429df501579968302df314241, all 18 cbench_* groups, 52 products matched the
  manifest. Copied mei -> local -> natsumi, sha256 checked on arrival.
- Booking 103 (natsumi, 330 min, `ops timing-book natsumi 330 --story TIMING-SWEEP-NATSUMI`), cancelled by me after
  both runs ended. Both jobs were queued with `~/bin/timing` (they hold every slot); the non-libm job started 44 s
  before the booking left "settling" (host idle, nothing else queued).
- natsumi:timing-sweep-natsumi-nonlibm, 02:47:58-03:42:58Z, rc 0: `sweep: COMPLETE`, 309 ratio_host rows,
  376 judged + 8 wordexp: 274 faster / 64 parity / 38 slower, spread median 0.008 (p90 0.059).
- natsumi:timing-sweep-natsumi-libm, 03:43:00-03:43:56Z, rc 0: `sweep: COMPLETE`, 166 ratio_host rows (169 judged),
  5 / 8 / 156, spread median 0.006. The libm bench is short: 3 adaptive rounds, every row settled at 3, 25 s.
- Command: ~/work/TIMING-SWEEP-NATSUMI/run.sh nonlibm|libm on natsumi = `./xtask-bin sweep run --out <R>/out` and
  `./xtask-bin sweep run --out <R>/out-libm --only cbench_libm` in the unpacked stage. Raw data stays on natsumi
  (~/work/TIMING-SWEEP-NATSUMI/out{,-libm}/, until the host leaves); the sweep.tsv files are reproduced in the
  results tables. Analyzer: `python3 -I handoff/timing-sweep-zen3-1107-analyze.py <out>/sweep.tsv
  <out-libm>/sweep.tsv results/timing-sweep-1106e-table.md`.

## Host setup (what a future Zen 3 timing host needs)
- ~/timing_host_setup.sh is not installed on natsumi. Run read-only from liella-ops (`bash -s -- --status` over
  ssh), it says NOT READY: 8 periodic timers not masked (man-db, apt-daily*, fstrim, motd-news, e2scrub_all,
  mdcheck*), swap active (/swap.img 8G, 0 B used, 29 GB free), turbo "unknown", min 400 MHz != max 3.6 GHz, no pin
  frequency known for the host, liella-timing.service not installed. Every fix needs root (owner); none made.
- What is set: perf_event_paranoid 1, governor performance x12, amd-pstate-epp active with EPP performance,
  scaling_max_freq 3.6 GHz = base (cpuinfo_max 4.267 GHz): the cap is the boost control, because amd-pstate-epp
  exposes no cpufreq/boost and the script then reports "turbo unknown". Idle clocks read ~3.567 GHz.
- No row of bench/host_limits.tsv names `AMD Ryzen 5 5500`: rows are reported, not gated (0 flagged). A Zen 3
  entry needs a full bench of main (docs/benchmarks.md section 6).
- Load average ~1.0 with no job: one kernel thread (kworker usb_hub_wq) stuck in D state, 0% CPU (vmstat 100% idle,
  CPU pressure 0). Not ours, not touched. Spread stayed at the other hosts' level (non-libm 0.008 vs Ivy Bridge 0.009,
  N100 0.012 in 1106E), so the run was kept.
- timing-intruders.log: one line, fwupd at 76% CPU for 2 s at 02:50:16 (fwupd-refresh.timer), 2 min into non-libm.
  man-db (05:01) and apt-daily-upgrade (06:42) were due after both runs ended.

## Findings (absolute Liella/glibc ratios, Zen 3; no earlier Zen 3 sweep exists)
- Non-libm: median time ratio 0.752, geomean 0.627; slowest: getline_80 1.75 (instr 1.02), strcmp_late_256 1.65
  (instr 0.885), strcmp_64_mix 1.51, gettext_language_steady 1.50 (instr 1.40), memset_144/256 1.45, iswctype_en_us
  1.43, qsort_c_cmp_1024* 1.25-1.40, then memrchr/strlen/strnlen/rawmemchr 64-4k at 1.27-1.32 with instr < 1 (time
  only on this class). regexec_pathological_4k prints 0.000 (< 0.0005).
- Libm: 5 faster / 8 parity / 156 slower, median 6.2x, worst ynf 725x, cexp 714x, cpow 578x, yn 527x; the long
  double and complex rows dominate the slowest 20.
- Orientation only (other classes, commit 34c98f61, 407 rows): 1106E Ivy Bridge 252/99/56, N100 313/74/20.

## Next step
Ruling #3776 (applied): the sweep is accepted at spread 0.008 despite NOT TIMING-READY; natsumi is out of dispatch
after the sweep (owner needs it back). For a future Zen 3 host the owner runs timing_host_setup.sh --apply and the
script learns the amd-pstate-epp boost cap (scaling_max_freq = base). Nothing left to run on this branch: land it.
Coordinator: if a Zen 3 class comes back, add its host_limits.tsv entry and set this table beside TIMING-SWEEP-LIBM-1107
(Ivy Bridge / N100 libm, running now). Zen 3 time-only rows (instr < 1, time > 1.05: strcmp_late_256, memrchr_64,
strlen_64*, strnlen_4k, rawmemchr_64_mix) are candidates for a Zen 3 reopen list.

## Not to redo
The stage, both runs, the analysis. Plan rows read: none (brief only).
