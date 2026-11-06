# PERF-LOCALTIME-FIRST-SECOND-MKTIME: where option A's zone-load cost goes; cold second mktime

State: branch task/perf-localtime-first-second-mktime, stacked on PERF-FIRSTCALL-MKTIME-SNPRINTF-2 (head 2162cc8e
inherited). This run adds only this handoff and handoff/reports/PERF-LOCALTIME-FIRST-SECOND-MKTIME.json: **no code change**.
Role shiki:~/w/sh2 (Haswell-EP). Decisions in force: D-23.6 (counter query at zone load, ruling #3698 option A), D-7.14.

## Method (black-box counters only; perf record was used on the Liella binary alone)
Probe (scratch, not in the tree; shiki ~/work/perf-localtime-first-second-mktime/ltprobe.c, probe.sh, tprun.sh):
a static Liella build (linked like the bench) and a dynamic glibc build of one C program run as 31 fresh processes
(`env -i TZ=America/New_York`). Around each call it reads its own aggregate counters (user and kernel instructions,
page faults, cycles, L1i and LLC misses) plus rdtsc. The `empty` row is the read overhead: subtract it. Modes:
1 = a cold `prctl(PR_GET_TSC)` first; 2 = cbench_opt's cold order before localtime_r (snprintf, strcmp, setlocale,
strcoll, iconv), then localtime_r, strftime, mktime, a second pass (snprintf, setlocale, regcomp/exec, fnmatch,
localtime_r, strftime) and the second mktime; 3 = mode 2 with 64 MB walked before the second pass (cold caches).
Builds: `head` (2162cc8e) and `noA` (head with the load-time `sys.tsc()` replaced by 0: a scratch edit, never
committed; the gate then starts closed and the first mktime queries the counter, as before option A).
Timing hosts kotori (Ivy Bridge E3-1265L v2) and nozomi (N100), booked (#99, #100), runs interleaved per round.

## 1. Option A's cost on first_localtime_r (net of `empty`, mode 2 = bench order, medians of 31)
| host | build | faults | kernel ins | user ins | TSC ticks first localtime_r | first mktime ticks |
|---|---|---|---|---|---|---|
| Ivy Bridge | head (A) | 18 | 128.1k | 23.5k | 289.3k (115.7 us) | 3.67k |
| Ivy Bridge | noA | 18 | 127.6k | 23.5k | 284.0k (113.6 us) | 8.88k |
| Ivy Bridge | glibc | 4 | 36.1k | 12.0k | 136.6k (54.6 us) | 4.96k |
| N100 | head / noA / glibc | 18 / 18 / 4 | 127.1k / 126.5k / 35.0k | 23.3k / 23.2k / 11.6k | cycles 136.0k / 155.1k / 63.2k | cycles 2.5k / 5.5k / 2.8k |
| Haswell-EP | head / noA / glibc | 18 / 18 / 4 | 130.9k / 130.4k / 36.2k | 23.3k / 23.3k / 11.6k | cycles 204.2k / 203.8k / 106.2k | cycles 2.7k / 6.7k / 2.8k |
Finding: option A adds **no page fault**, ~500 kernel instructions and 44 user instructions (one `prctl` round
trip) to the zone load. On Ivy Bridge that is +5.3k ticks = **~2.1 us** in mode 2 (+1.2 us mode 1, +3.4 us mode 3),
and the same cost leaves first_mktime (noA's first mktime is +5.2k ticks). It is a move, not an addition. A cold
`prctl` alone costs 2.0 us on that host under glibc too (glibc mode 1 prctl1 - empty = 5.0k ticks, no fault). On
N100 and Haswell-EP, A measures as noise to a gain on localtime_r. The `tsc_probe`, `liella_sys_prctl` and
`liella_machine_rdtsc` text shares a 64 KB fault-around window with the syscall wrappers, which are already
faulted by then (perf record of page faults on the Liella probe), so no new fault is possible there.
**The remaining ~4.7 us of the booked 6.8 us is not reproduced** by an in-process A/B with one layout. The
predecessor's A/B compared two trees whose bench binaries also differ in layout, and its 6.8 us is a difference of
two medians (88.9 vs 95.8 us), so layout or noise remains possible. Not attributed further; see Open.
The only cure for the ~2 us is to not query at load (undo A), which moves the same cost back to first_mktime.
-> RULING NEEDED: keep A (recommended; it is a wash in total, and first_mktime is the row the timing sweep flagged).

### Where first_localtime_r's real gap is (all hosts: Liella 18 faults vs glibc 4, ~92k more kernel instructions)
Faults in the load (perf record on the Liella probe, mode 1): the zone code's text pages, the vDSO data and text
pages (first `clock_gettime`), the TZif parse's stack page, and **about 10 heap pages: each new allocation size first
touches its own size-class region** (TryVec growth in `tzif::parse`, `TypeRecord` push, `Zone::from_tzif`,
`fill_rule_years`, realloc/memmove copies at 7e78960{4,8,c}0000, ..., 280000). A load that sized its vectors from the
TZif header counts and made one allocation per table (or one arena for the zone) would remove most of these.
Proposed follow-up story; not done here (budget).

## 2. second_mktime_new_york (cold second call)
Probe (net of `empty`), Liella vs glibc: user instructions 886 vs 1236 (all hosts). Ivy Bridge ticks: mode 2
2.88k vs 3.69k (0.78x), mode 3 4.49k vs 6.11k (0.73x). N100: mode 2 379 vs 340 ticks (1.11x), mode 3 1161 vs 1077
(1.08x); L1i misses 127 vs 82 (mode 3), LLC 115 vs 112. Haswell-EP mode 3: 6.2k vs 5.3k cycles, LLC 93 vs 83.
Finding: the second call repeats **no work it should not**. It runs fewer instructions than glibc and takes the
thread's period cache (MKT-2) path. The bench's 1.21x (IVB) / 1.42x (N100) is not reproduced on Ivy Bridge by this
sequence. On N100 the residue is code footprint: more L1i misses on fewer instructions, so the mktime path is spread
over more cache lines. The bench's pass between the two calls also runs wordexp, gettext, regcomp and more, which
the probe does not, so the bench's own sequence under counters is the next step. **No fix made, no test added.**

## Open / next step
- Run cbench_opt's exact cold sequence under the probe's counters (or teach CBENCH_COUNTERS the cold rows) to
  attribute second_mktime's 1.2-1.4x. Then, if it is L1i, cluster the mktime path (`zone::mktime`, `select`,
  `resolve_small`, `pack::tm`) in hot-text with a pin, not a full regen (NEGATIVE row in docs/saturation/07).
- Zone-load allocation faults (above): a separate story, "size TZif tables from the header, one allocation".
- The unattributed ~4.7 us of the booked +6.8 us: retime pre-A vs head bench binaries interleaved >= 3 rounds.
## Not to redo
A/B of option A in-process (this file); a full hot-text regen; bisecting older commits.

## Jobs
shiki: perf-localtime-first-second-mktime-probe1 (head, build + first probe, multiplexing-free), -probe2 (SUPERSEDED:
7 events multiplexed, kernel column invalid), -probe3 (head + glibc), -probe-noa (scratch noA build + probe; role re-synced
to head afterwards), -mkbins (static probe binaries from the saved archives), -lint (clippy + static on final head).
kotori: perf-localtime-first-second-mktime-tp-kotori; nozomi: perf-localtime-first-second-mktime-tp-nozomi (both rc 0,
run under ~/bin/timing in bookings #99/#100, cancelled after). No `bench --stage` comparison build was made (0 calls):
the probe answered step 3's question per call, so the time-group bench rows were not re-timed (rows unchanged: no code).
