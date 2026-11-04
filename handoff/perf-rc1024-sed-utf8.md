# PERF-RC1024-SED-UTF8: sed-utf8 instructions vs fa678f74

State: branch `task/perf-rc1024-sed-utf8-3` (continues -2 and the first run), code identical to fa513bd3
(on origin/next 125c0ccc). READY: the remaining sed-utf8 +0.40% vs fa678f74 is accepted (ruling #2333) and
recorded in docs/saturation/06 (register row "sed-utf8 workload, regex prefilter one-byte search").

## PERF-RC1024-SED-UTF8-3 (2026-11-04): the short-window pre-scan loses, reverted

Ruling #2333: one bounded try of a scalar pre-scan in `conv::find_core` (m == 1) before `memchr`. Tried:
the first 16 bytes byte by byte, the rest (if any) by `strz::find_in` (commit 4c568bd7). Job
perf-rc1024-sed-utf8-3-count (keke, wl_count, instructions:u, output SAME everywhere): sed-utf8 2485913088
vs 2456411104 at fa513bd3 (+1.20%) and vs 2446684937 at fa678f74 (+1.60%); sed-s 9664611389 (+0.22% vs
fa513bd3); gawk-regex, gawk-utf8, grep-E, grep-i-utf8 1.0000. Reverted (the scanner is already cheaper on
these windows). Do not retry a scalar pre-scan here. Gate and plants rerun on the reverted head (report json).
Next: nothing on sed-utf8; gawk-regex/gawk-utf8 are PERF-RC1024-GAWK-REST.

## PERF-RC1024-SED-UTF8-2 (2026-11-04): the presence cache cannot be hit on sed-utf8

Ruling #2253 allowed a per-thread cache of the prefilter's 'present' answer (never 'absent'). Before
building it, a black-box probe logged sed's calls: an LD_PRELOAD shim over /usr/bin/sed (re_search, regexec,
re_search_2; sed imports only re_search) on the first 20 lines of ~/wl/work/words.txt, LC_ALL=C.UTF-8,
`s/[[:alpha:]]*a[[:alpha:]]*/<&>/g`. Result: lines with no 'a' make no regex call at all. A matching line makes
exactly three re_search calls, at start 0, then at match 1's end, then at match 2's end (the last answers -1).
sed alternates two line buffers (two pointers). A start never repeats, and each start lies past the required
byte the previous call found (the match holds it). So a cache keyed on "same buffer, first <= cached
occurrence" never hits, and the third call's memchr (absent) could only be skipped by caching 'absent', which
the ruling forbids. Not implemented: it would add a claim, a table fetch and a compare per call and save
nothing. RULING NEEDED: accept sed-utf8 +0.40% vs fa678f74 (recommended), or let a run try a short-window path
in `conv::find_core` m == 1 (a scalar scan of the first few bytes before `liella_machine_memchr`; the windows
here are about 5 to 30 bytes), measured per call with `cargo xtask bench --counters` before any count.
The -2 run also verified plants and the gate on this head (labels in the -2 report json).

## Change

`Program::first_start` (crates/core/src/pattern/regex/program.rs) tests the window's first byte before the
four-byte chunk scan. A repeated search (sed `s///g`) mostly starts on a byte that can begin a match; the
chunk scan used five tests to answer offset 0 (four in the chunk plus the rescan). Windows that do not
start on such a byte take the unchanged chunk scan. Local detail, no decision row needed (no interface,
no behaviour change).

## Counts (keke, validation/wl_count.sh, instructions:u; output SAME everywhere)

| workload | fa678f74 | 125c0ccc (head) | fa513bd3 (fix) | fix/head | fix/fa678f74 |
|---|---|---|---|---|---|
| sed-utf8 | 2446684937 | 2460015447 | 2456411104 | 0.9985 | 1.0040 |
| sed-s | 9629460590 | 9671460710 | 9643126719 | 0.9971 | 1.0014 |
| gawk-regex | 3324860493 | 3361292452 | 3361292587 | 1.0000 | 1.0110 |
| gawk-utf8 | 3449602115 | 3474609154 | 3474609067 | 1.0000 | 1.0072 |
| grep-E | - | 466299818 | 466299870 | 1.0000 | - |
| grep-i-utf8 | - | 358419413 | 358388732 | 0.9999 | - |

Jobs: perf-rc1024-sed-utf8-count0 (head vs fa678f74), -count1 (fix vs head, fix vs fa678f74).
No row is slower. sed-utf8 is still +0.40% (+9.7M) over fa678f74.

## Not done (exact next step)

- Per-candidate attribution (brief step 1) not done: fa678f74, 12e93df9 and 76041c21 are not ancestors of
  the post-cutover origin/next (76041c21 is not in the clone at all), so counting each landing needs their
  source primed on keke (validation/wl_prime_base.sh from a checkout that has them) and one wl_count each.
- (Superseded by the -2 probe above: the cache cannot hit.) The remaining sed-utf8 gap: the RC-1024 profile shows a new `liella_machine_memchr` (0 -> 52M), which is
  the regex prefilter's single-byte required-byte scan (`conv::find_core`, m == 1 -> `strz::find_in`) for
  `[[:alpha:]]*a[[:alpha:]]*`. Called once per `re_search`; sed's `s///g` makes about three per line.
  Candidate (not tried): when the bytes neither lead every match nor are the whole pattern, the prefilter
  can only answer NoMatch or Engine; a presence answer cached for the same subject could skip the rescan
  (Engine is always correct). That cache is shared mutable state in the boundary, so it needs a design
  ruling first.
- gawk-regex (+1.10%) and gawk-utf8 (+0.72%) vs fa678f74 are unchanged by this fix; they need their own
  profiles at head.
- A profile at head: the ~/wl/kit/<sha>/sed binaries did not run the workload by hand (wrong environment).
  Use the wl_count log's profile mode or the RC tool, not a hand-made perf command.

## Verification

Gate perf-rc1024-sed-utf8-gate (direct tier), plants perf-rc1024-sed-utf8-plants (p1 skip first byte, p2
empty window answers Some(0): expected CAUGHT; p3 drop the hit test: EQUIVALENT, an earlier lower bound
only costs time because the reverse pass still finds the start). Results: see the report json / hand-back.

## PERF-RC1024-SED-UTF8-4 (2026-11-04)
CI lab-names fix only: register row in docs/saturation/06 now says "Zen 1 host, 60 vCPU" (was keke). Job: keke perf-rc1024-sed-utf8-4-static (xtask static + clippy). Next step: CI.
