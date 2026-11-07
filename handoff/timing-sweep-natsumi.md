# TIMING-SWEEP-NATSUMI (table written; first Zen 3 sweep)

State: branch task/timing-sweep-natsumi, base origin/next 66b8d3ba (products built from it). No product code
change; one analyzer script. Result: results/timing-sweep-zen3-1107-table.md (summaries, slowest 20 per set,
1106E counts for orientation, full tables).

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
  results tables. Analyzer: `python3 -I handoff/timing-sweep-natsumi/analyze.py <out>/sweep.tsv
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
Coordinator: if a Zen 3 class comes back, add its host_limits.tsv entry and set this table beside TIMING-SWEEP-LIBM-1107
(Ivy Bridge / N100 libm, running now). Zen 3 time-only rows (instr < 1, time > 1.05: strcmp_late_256, memrchr_64,
strlen_64*, strnlen_4k, rawmemchr_64_mix) are candidates for a Zen 3 reopen list.

## Not to redo
The stage, both runs, the analysis. Plan rows read: none (brief only).
