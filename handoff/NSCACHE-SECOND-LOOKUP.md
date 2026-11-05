# NSCACHE-SECOND-LOOKUP (handoff, after run 2 = NSCACHE-SECOND-LOOKUP-2)

**State.** Branch `task/nscache-second-lookup-2` (run 1's `task/nscache-second-lookup` + 3 commits), from `origin/next`
ff125cce. Code head 2a40c53a (the handoff/report commit is on top). Role keke:~/w/n3 (Zen 1).
Commits: 4507467b (run 1: p_nss probe, fast-profile hot-text regen, grouping test), e460117f (probes under
`validation/probes/nscache/`: percall.c, steps.c, measure.sh, timing.sh), 877bd3c9 (release-profile regen merged into
the list: +7 keys, ~20 keys moved into the release trace's first-touch order; grouping test made profile-independent),
2a40c53a (plants file for both profiles).
**Decisions in force.** D-16.10 unchanged (first lookup uncached; attach + first validation on the 2nd; every amendment
check kept). Ruling #3234: no attach at break-even. Ruling #3233 (skip boot_id on tmpfs) is with the OWNER: not implemented.
No new D-row (layout of the products only).

## Result (release products, static `percall` probe; base = ff125cce's list, after = this branch)
Counts (Zen 1 host, deterministic): first getpwnam_r 41 -> 30 faults, ~420k -> ~305k kernel instr; cached 2nd
11 -> 8 faults, ~168k -> 129.5k kernel instr; uncached and hit calls unchanged (19.5k+37.6k; 4.1k+6.0k incl. markers).
The fast-only list already gave the release 2nd call its 8 faults; the release regen adds 2 faults off the 1st call.

Timing (booked, interleaved A/B, 400 rounds, medians, `validation/probes/nscache/timing.sh`):

| host class | 1st cached | 1st uncached | 2nd cached | 2nd uncached | later uncached | hit | attach extra | break-even |
|---|---|---|---|---|---|---|---|---|
| Ivy Bridge E3-1220L v2, base | 376.5 us | 375.6 | 230.7 | 44.4 | 35.68 | 6.15 | 187.2 us | n > 8.34 |
| Ivy Bridge, after | **307.5** | **306.7** | **200.4** | 42.6 | 34.62 | 5.89 | **158.6 us** | **n > 7.52** |
| N100 (AVX2), base | 151.3 | 143.7 | 91.9 | 15.7 | 11.92 | 2.20 | 83.9 us | n > 10.63 |
| N100 (AVX2), after | **117.2** | **114.5** | **78.3** | 14.9 | 11.47 | 2.09 | **66.1 us** | **n > 9.06** |

attach extra = c1 + c2 - u1 - u2; break-even: a process gains from the cache from lookup ceil(2 + extra/(later - hit)):
the 8th (was 9th) on Ivy Bridge, the 10th (was 11th) on N100. The first-lookup gain (-18% / -20%) holds for every process
with any lookup, cache on or off. By instructions run 1 said break-even 4.7 -> 3.7; time puts it at 7.5-9: the
attach's kernel work (stats, /proc, mmap, faults) costs more time per instruction than the user-mode lookup it saves.
**Target (2nd cached <= uncached) not reached: the floor is the attach's security system calls** (~95k kernel instr:
5 config stats, /proc/self/ns/user, boot_id, open/fstat/mmap of the region, 4 id calls; see steps.c). Layout cannot
remove them; only a change to the checks could (ruling #3233, with the owner).

## Jobs (keke unless named)
- `nscache-second-lookup-2-regen` rc 101: the grouping test failed on release products (region_path, stamp_files,
  Table::get are inlined into attach's closure in fat LTO). Fixed in 877bd3c9 (test now: every section of an attach-path
  function is grouped, >= 2 present). Superseded.
- `nscache-second-lookup-2-bounds` rc 0: hot_text_products 6/6 on release (startup 17, setlocale+printf 58, tool 61) and
  fast products (19, 58, 60); bounds 23 / 68 / 71. Counts above.
- `nscache-second-lookup-2-plants` rc 0: 3 CAUGHT (hot-region-path fast, hot-map-shared-rel and hot-attach-rel release),
  0 WARN-RC-ONLY/NO-MSG; one WARN-DEV-SHM line (copy under /dev/shm; harmless for this test).
- `nscache-second-lookup-2-gate` rc 0 (escalated to FULL: hot-text.list is linker input): 32 suites ok, 10873 passed,
  0 failed; conformance 117 passed; peak 12G. clippy (`--all-targets --all-features -D warnings`) and static rc 0 on 2a40c53a.
- `nico:nscache-second-lookup-2-nico`, `nozomi:nscache-second-lookup-2-nozomi` rc 0 (timing; bookings 44, 45 cancelled).
- Run 1's `shiki:nscache-second-lookup-gate` (4507467b) passed: 10873 passed, 0 failed.

## INFORMATIONAL
- The cached 1st call is 1-8 us slower than the uncached 1st on N100 (151.3 vs 143.7 base, 117.2 vs 114.5 after) though
  both do the same system calls (TIMING-NSCACHE-FIRSTCALL); Ivy Bridge shows 1 us. Not investigated.
- No saturation-register file covers the name-service cache yet.

## Next step
None required for this story. If the owner allows #3233, the boot_id read (13.4k kernel instr) goes; re-time with
timing.sh. Rows: `grep -rn "^| D-16.10 " plan/`.
