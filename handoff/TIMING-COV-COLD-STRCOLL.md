# TIMING-COV-COLD-STRCOLL: first strcoll in en_US.UTF-8 (NEGATIVE, reverted)

**State:** branch `task/timing-cov-cold-strcoll` (base next e0c6b146). The code change (commit 84f01705) is REVERTED on the branch. The branch adds records only: the rejected D-row (`D-NEW`
placeholder, plan/07-decisions/s04-byte-arrays-and-strings.md, after D-4.63), the saturation entry
(docs/saturation/05-locale-ctype-time-gettext.md, `first_strcoll` row), this note, and
handoff/TIMING-COV-COLD-STRCOLL-plants.txt (for the reverted code; do not run).

## Profile (base, mei, static probe `~/work/TIMING-COV-COLD-STRCOLL/probe.c` + faults.sh, ASLR off, perf -d)
Steps S = setlocale(LC_ALL, "en_US.UTF-8"), C = strcoll("zeta.h", "alpha.h"); SC - S, perf stat -r 100:
| side | user instr | kernel instr | faults |
|---|---|---|---|
| base | +11,766 | +30.8k | +3 |
| change | +11,635 | +21.1k | +2 |
| glibc static | +64 | about 0 | 0 |
The base's three faults (addresses): one locale-archive page read in `Collate::fill`; two copy-on-write faults on
`ASCII_POOL` (file-backed .data), because the compiler put `rank`/`ready`/`ascii_fast` about 10 KiB past `unit`
(after the 8 KiB lead indexes). The change (`AsciiSlot` `repr(C, align(4096))`, written fields first) removed one.

## Timing (11 interleaved rounds, staged cbench_opt head/base/glibc-dynamic; median of per-round ratios)
| row | N100 head/base | IVB head/base | N100 ns head / base / glibc |
|---|---|---|---|
| first_strcoll | 0.996 (0.971-1.010) | 1.017 (11/11 slower) | 12,810 / 12,889 / 514 |
| second_strcoll | 0.974 | 1.012 | 301 / 310 / 351 |
| first_setlocale_en_US | 1.017 (10/11 slower) | 1.001 | 124,957 / 123,162 / 53,894 |
| cold_exec_once | 1.008 (11/11 slower) | 1.002 | |
| strcoll_en_us_* | 0.999-1.001 | 1.000-1.012 | |
Verdict: no gain on the target, small losses elsewhere: reverted (rule "measured worse is reverted").

## What this says
D-4.54 cut 10k user instructions and added a fault: slower. This run cut a fault (about 10k kernel instructions in
the probe): no change. So neither the fill's instructions nor a fault explains the ~12 us vs 0.5 us. Not profiled:
where the wall time goes inside the cold cbench child (it is not the probe; layout differs).

## Next step (new story, profile first)
Measure the first strcoll's wall time by phase INSIDE the cold process (rdtsc/cycles around strdiff, decide, stream,
general path, fill; cycles:u vs cycles:k with perf stat on the cold child), and check cache/TLB misses on the archive
data and the cold text (perf stat -e dTLB/iTLB/LLC misses SC - S). Only then design.
Do not redo: D-4.54 (fill only the pair's bytes) and D-NEW (one-page slot layout). Plan rows: D-4.30, D-4.37, D-4.54, D-NEW.
