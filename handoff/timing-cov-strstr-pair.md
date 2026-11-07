# TIMING-COV-STRSTR-PAIR handoff (strstr on 64-byte haystacks)

State: branch task/timing-cov-strstr-pair-2 (= task/timing-cov-strstr-pair 84ac99f1 + this handoff), from origin/next
a09927ac. Code unchanged since 9e5ac7f6. -2 role mei:~/w/s2 (Skylake-SP, AVX2 + AVX-512); first run keke:~/w/n2 (Zen 1).
Reports: handoff/reports/TIMING-COV-STRSTR-PAIR.json, handoff/reports/TIMING-COV-STRSTR-PAIR-2.json.
Plants: handoff/TIMING-COV-STRSTR-PAIR-plants.txt (8 plants). No new D-row (a local entry change; strstr's behaviour and
the strpair scan contract are unchanged; strpair's header states the register contract the entry relies on).

## Change (first run)
`liella_machine_strstr` (csys/src/machine/scan.S) is the exported `strstr` (Rust twin `posix::strstr` calls it):
short needles (<= 15 bytes, both strings' first 16 bytes inside their page) are measured and settled in registers around
one strpair call; everything else tail-jumps to `liella_hot_strstr_slow` / `liella_hot_strstr_next` (strz.rs).
Test added: strstr_needle_ending_on_the_last_mapped_byte_is_measured_inside_its_page.
Profile before (hit_64 125.5): strpair kernel ~45, Rust frame/needle loop/KResult return ~80; the entry removed the latter.

## Counts (cargo xtask bench --compare --counters --runs 3 cbench_memref, instructions per call)
| row | before (Zen 1) | after (Zen 1) | glibc Zen 1 | after (Skylake-SP) | glibc Skylake-SP |
|---|---|---|---|---|---|
| strstr_hit_64 | 125.5 | 100.5 | 164.6 | 100.4 | 99.4 |
| strstr_miss_64 | 128.5 | 88.5 | 337.7 | 88.3 | 173.4 |
| strstr_hit_4k | 651.3 | 626.1 | 826.4 | 625.9 | 2243.5 |
| strstr_miss_4k | 911.2 | 872.0 | 7523.2 | 870.7 | 6448.2 |
hit_64 at parity by instructions on Skylake-SP (100.4 vs 99.4, 1.01). Skylake-SP -AVX2 section: Liella 100.4 / 88.4 /
626.1 / 870.7 and glibc 99.3 / 173.4 / 2243.5 / 6447.9, i.e. the same as the default section on both sides, so the
tunable did not move either side here (as on Zen 1): the SSE2 level is still NOT confirmed by a count. The entry
itself is SSE2-only, so its saving carries over by construction. How Liella's level is forced for a count is open
(handoff/timing-cov-strstr-nonavx2.md says its counts were the same with and without the mask).
ns columns of the same run (unbooked host at load ~20; decides nothing), glibc / Liella: hit_64 7.95 / 10.01 (1.26),
miss_64 12.81 / 8.25, hit_4k 161.5 / 74.4, miss_4k 543.4 / 117.4.
Other rows on Zen 1 within +-1.9 (untouched code). Timing not run (instruction mix unchanged in kind: SSE2 compares
replace scalar loops); left to the periodic sweep.

## The gate3 red (keke, liella::popen_system_eintr system_signal_state_during_and_after, line 225)
Not caused by this branch: a race in the test.
- Line 225 asserts `hits=1` right after `system` returns. The body runs on libtest's spawned test thread (the panic names
  thread 'system_signal_state_during_and_after' in a one-test process); the main thread waits with SIGCHLD unblocked.
  The shell's SIGCHLD is process-directed and the caller blocks it during `system`, so the kernel queues it for the
  main thread, whose handler can still be in flight when the caller unblocks SIGCHLD and reads HITS -> hits=0.
  Liella's behaviour is right (POSIX: the mask is per thread); the test assumes the handler runs on the caller.
- The same test failed the same way in the CI gate of its own landing (mei ~/logs/ci-gate-3a13253f8-1792860442.log,
  2026-10-24, before this branch's strstr change existed). `system`'s path (spawn.rs, s18_processes.rs) never calls strstr.
- Repeat loops of the re-executed case on this head (exports interposed build, mei, load 8-25): 800 runs unpinned and
  800 pinned to 2 CPUs, all `hits=1` (the window is narrow; a gate at load 21 hit it once in ~10 600 tests).
  No separate origin/next rerun: the 10-24 CI failure is the evidence on a tree without this change.
- Suggested fix (its own story): after `system` returns, wait for HITS to reach 1 under an iteration budget (hang
  guard per crates/core/tests/support/timeout.rs) and then check it stays 1, instead of reading it once.

## Jobs (mei, final code = 9e5ac7f6)
lint (clippy + static, inherited head) rc 0; eintr-head, eintr-head-pin (diagnostic loops, evidence only);
plants rc 0 (8 CAUGHT, 0 SURVIVED, 0 WARN); gate rc 0 (clippy, static, affected --run --base origin/next: 13 suites
ok, 10639 passed, 0 failed, 1230 ignored, conformance 124/124, peak 15G, popen_system_eintr green); counts rc 0.
Jobs ran on tree 84ac99f1; the final commit adds only handoff/ files.
keke (first run, same code): plants4 rc 0 (8 CAUGHT), counts-final2 rc 0; gate3 rc 1 (the flake above).

## Next step
1. Its own story for the popen_system_eintr race (above).
2. If the periodic timing sweep shows strstr_hit_64 slower in time on either class, profile the strpair heads
   (TIMING-N100-STRSTR-64 / TIMING-COV-STRSTR-NONAVX2 territory) before another entry change.
Not to redo: the Rust frame around the first scan (this story), the kernel heads (TIMING-N100-STRSTR-64,
TIMING-COV-STRSTR-NONAVX2).
