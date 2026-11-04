## 6. Printf, regex, wordexp, iconv

| Row | Now | Claimed limit | Reason class | Basis | Source story | Tried, measured negative | Flag |
|---|---|---|---|---|---|---|---|
| `printf` `%02x` on a stream | i 529.4 vs 612.1 (0.86x; 2026-10-20 spot check) | ahead of glibc | n/a | M | PRINTF-LAYERS | PRINTF-PARSE: single-directive formats did not gain, dropped. PRINTF-FLOAT: `g6_sum` +2.1% (63 inline instr vs 22 for `u128` div), table +0.3 to 1.2%, reverted `79fb043d` | none |
| `snprintf` `%d`, `%x08`, `%ld` large | t 1.13, 1.06, 1.00; i 0.96, 1.04, 0.91 | at or near parity | n/a | M | PRINTF rows | see above | **measured 2026-10-23** (section 10) (10-07) |
| `snprintf` `%e` | t 3.28 (2.68 with x86-64-v3) | not at a limit | n/a | C | | a multiversioned float core flagged "worth a look", not planned | **measured 2026-10-23** (section 10) |
| `regexec` sed tail and neighbours | +4.4% on `regexec_sed_tail`, over the 1% limit | "Do not retry" (REGEX-SED) | design | R | REGEX-SED; also RX-T4C (anchored line failing late 2.25x overall pass, aud1 +24%), REGEX-WL (saves 1 `jmp` for 1 instr), `regexec` stage 1 (no gain) | see left | none |
| `regexec_backref` and the capture rows: a lazy DFA (X-3) | i 1.11 on `regexec_backref` (24,460 vs 21,950 instr/call); capture rows i 0.07 to 0.27, `re_sed_*` 0.08 to 0.21 (job `regex-lazy-dfa-profile`, 60-vCPU x86-64 build host, 2026-11-04) | a DFA cannot decide back-references; a relaxed-pattern span filter removes at most about 10% of the backref row (estimate, not measured), not parity; capture rows already far below glibc | design | M | X-3a `REGEX-LAZY-DFA` (`plan/07-decisions/x3-regex-lazy-dfa.md`) | negative for the DFA on back-references; the one-pass capture DFA is deferred until a capture row is above parity. What could pay: the matcher's fixed cost per call (`attempt` 14.3%, scratch clearing 7.7%), story X-3b | none |
| sed-utf8 workload, regex prefilter one-byte search (`conv::find_core`, m == 1) | instructions:u 2456411104 at fa513bd3, +0.40% vs fa678f74 (2446684937); sed-s +0.14% (Zen 1 host, 60 vCPU, wl_count, 2026-11-04) | accepted (ruling #2333) | design | M | PERF-RC1024-SED-UTF8, -2, -3 | Probe (LD_PRELOAD log of sed's `re_search`, -2): lines without the required byte make no regex call; a matching line makes 3 calls (start 0, end of match 1, end of match 2), two alternating line buffers, so a start never repeats and a 'present' cache cannot hit (not built). -3: a scalar scan of the first 16 bytes before `memchr` (m == 1): sed-utf8 +1.20% (2485913088), sed-s +0.22% vs fa513bd3, other rows 1.0000, reverted. The `memchr` scanner is already cheaper on these windows. | no |
| `wordexp` | t 2.9 to 4.2 (stale; Ivy Bridge host A 2026-10-23: t 0.72 to 1.76, section 10; Haswell-EP host 2026-10-23: t 0.6 to 1.7, instructions 0.3 to 2.0, `docs/wordexp-profile.md`) | OPT-STAGE1: "no measurable gain, reverted"; "no design for a larger change"; X-5 profile: a larger change is not justified | n/a | R | OPT-STAGE1, X-5 | see left | **measured 2026-10-23** (section 10; Zen 1 and Broadwell-EP not run) |

### X-3b `REGEX-BACKREF-FIXED`: ring of eight for back-reference patterns

`regexec_backref` (`\(b[a-z]*\)@\1`, 13-byte subject): a pattern with back-references set up a
ring of sixteen frontiers per call; it now starts with eight (`BR_RING`), and a thread sent further
than that waits on the pending list as before. Instructions:u per call on a Skylake-class build
host, `cargo xtask bench --compare --counters --runs 3 cbench_patterns cbench_opt`:
24,493 to 22,913 (glibc 21,970: ratio 1.11 to 1.04). No other regex row moved (every row within 0.1%).
Not at parity: the matcher's own steps (`run`, `insert`, `walk`) are the rest and are not fixed cost.
Not tried: lazily built frontiers (the pool layout is fixed up front). A back-reference of eight
bytes or more costs one pending-list allocation (`regex_backref_ring_*` tests cover 0 to 40 bytes).
