# COLDSTART-STRCOLL-FIRST(-2): first strcoll in a rules locale

**State:** branch `task/coldstart-strcoll-first-2` (on `task/coldstart-strcoll-first`, base next e8189dd8). Code commit
7eaa03af (or later) REVERTS the D-4.54 change: `crates/core/src/locale/collate.rs` and `crates/core/tests/collate_lazy.rs`
are the base's again. What the branch adds is records only: D-4.54 marked REJECTED with its numbers, a negative row in
docs/saturation/05-locale-ctype-time-gettext.md, and this handoff. Result: NEGATIVE. The target (first_strcoll near glibc)
is not reached.

## What was measured (run 2, 2026-11-06)
Jobs from run 1 (keke, on the D-4.54 code): csf-gate2 rc 0 (direct tier, 104 targets), csf-plants rc 0 (3 CAUGHT),
csf-counts rc 0. These are evidence for the reverted code only. csf-gate ran nothing (no `--run`).

Probe (keke job csf2-prep2; source and scripts in ~/work/COLDSTART-STRCOLL-FIRST-2/ on keke: csfprobe.c, prep.sh;
static Liella builds of the branch ("after") and of the branch with the collate.rs diff reversed ("before"); perf stat -r 200;
steps S = setlocale(LC_ALL, "en_US.UTF-8"), C = strcoll("zeta.h", "alpha.h")):

| delta | after iu | before iu | glibc dyn iu | glibc static iu | faults after / before / glibc |
|---|---|---|---|---|---|
| first strcoll (SC - S) | 1,601 | 11,749 | 284 | 66 | +4 / +3 / 0 |
| second strcoll (SCC - SC) | 194 | 86 | 283 | 70 | 0 |
| second after a 2nd setlocale (SCSC - SCS) | 196 | 85 | 282 | 65 | 0 |

Kernel instructions SC - S: after about +43k, before about +30k, glibc +0.5k (noise between identical S runs is about 5k).

N100 timing (nozomi job csf2-n100, ~/work/COLDSTART-STRCOLL-FIRST-2/run.sh: staged cbench_opt after / before / glibc dynamic,
CBENCH_ONLY=strcoll_en_us,cold, 11 rounds with the order rotating each round; ratio = median of the per-round ratios):

| row | after ns | before ns | glibc ns | after/before | after/glibc |
|---|---|---|---|---|---|
| first_strcoll | 13190 | 11571 | 503 | 1.130 (1.097-1.186) | 25.5 |
| second_strcoll | 297 | 268 | 329 | 1.100 (1.029-1.433) | 0.91 |
| strcoll_en_us_names | 18.1 | 18.1 | 41.7 | 1.000 | 0.43 |
| strcoll_en_us_equal_16 | 5.4 | 5.4 | 404.6 | 1.000 | 0.013 |
| strcoll_en_us_utf8_16 | 3490.7 | 3478.3 | 5568.7 | 1.002 | 0.63 |

Verdict (ruling #3337 and the "measured worse is reverted" rule): second_strcoll is slower, and so is first_strcoll, the
target row itself. #3337's fallback (finish the fill on the second strcoll of any kind) would fix only second_strcoll and
not the first call's loss, so the change was reverted rather than reworked.

## What this says about the row
About 10k fewer user instructions still made the first call ~1.6 us slower, so most of its time is not the fill. It is
kernel work: the base pays 3 page faults and about 30k kernel instructions on the first strcoll where glibc pays 0. The
on-demand variant added one fault (not profiled: perhaps fill_demand on another text page, or a different data page).

## Next step (a new story, profile first)
1. Find which pages the first strcoll faults on, in the base: `perf record -e page-faults -d` or
   `perf trace -F all --no-syscalls` on the probe (keke ~/work/COLDSTART-STRCOLL-FIRST-2/probe.before), with step strings
   S and SC. Sort the faults into text, locale data (the mapped LC_COLLATE file) and .bss (ASCII_POOL slot).
2. Design against the fault class found (for example: keep the first compare's code and data on pages that setlocale has
   already touched, or collate without touching the table pages for an all-ASCII pair). Count faults and kernel
   instructions as well as user instructions; judge by N100 timing.

Do not redo: the D-4.54 design (rejected). Plan rows: D-4.30, D-4.37, D-4.54 (plan/07-decisions/s04-byte-arrays-and-strings.md).
handoff/COLDSTART-STRCOLL-FIRST-plants.txt is for the reverted code (kept for the record; do not run it).
