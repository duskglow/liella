# TEST-VALUE-AUDIT: which tests earn their gate time (planning; no code changed)

State: branch task/test-value-audit from origin/next e0c6b146. Nothing in the gate is changed here: every
SHRINK/MOVE/CUT below is a RULING NEEDED for the owner; the CONSOLIDATE briefs keep every case and may run
without a ruling (said per brief). Out of scope and not re-measured: the remez check (GATE-REMEZ-CHECK-CACHE),
test ordering (GATE-NEXTEST-PRIORITY-REFRESH), the math suites in pass 2 (GATE-PASS2-MATH-SHARD).

## Method and sources
- **Cost**: the 40 most recent green FULL CI gates (jobs.tsv kind `ci-gate`, rc 0, 2026-11-05 05:47Z ..
  11-07 02:39Z, the same window as GATE-TIME-PROFILE) on the CI host (mei), nextest PASS lines of both passes.
  Per test: median seconds over the trains that ran it, weighted by the share of trains it ran in; per suite
  (nextest binary id): the sum. Script (read-only) mei:~/work/test-value-audit/tva.py, outputs suites.tsv,
  tests.tsv, fails.tsv there; shape.py for the per-suite spread. Totals: pass 1 11990, pass 2 12477 test-s.
- **Wall conversion**: both passes are throughput-bound (GATE-TIME-PROFILE: ~16 tests in flight), so a cut of
  T test-s in one pass is estimated at T/16 s of that pass's wall unless the pass falls to its privileged chain
  (~400 s per pass). Every minute figure below is an ESTIMATE from that rule; NOT VERIFIED by a before/after gate.
- **Plants**: test targets named by the 103 handoff/*-plants.txt files (command field), and the plant logs on
  the four build hosts (~/logs/*plants*.log; 1,715 verdict lines). Per-plant logs that name the failing test
  survive for only 40 plants (the RAM dirs were cleared), so "which suite caught it" is mostly read from the
  plant command and its msg regex. Scripts: plantscan.py (same dir, also copied to each build host's
  ~/work/test-value-audit/).
- **History**: FAIL/TIMEOUT lines of all 328 ci-gate logs (any rc) on the CI host, plus `---- <binary> <test>
  stdout ----` and nextest FAIL lines in every non-plant log on the four build hosts.
- **Setup cost** (coordinator addition): (a) the per-process floor from the CI timings; (b) one measurement job,
  `tva-setup2` on keke role r5 (synced to e0c6b146), ran shard 0 of the four C-oracle math sweeps with
  `--nocapture` (each prints its own "building and running the programs X s, checking Y s" line) and timed the
  cached oracle programs alone. Job `tva-setup` before it was stopped by me (my output filter cut its output;
  no result used).

## Findings
1. **The test count is not where the time goes.** 11,148 test runs per train (both passes); 10,337 take < 1 s
   and together cost 1,206 test-s (10%); 8,557 take < 0.2 s. Median test 0.03 s, p05 0.01 s, so the nextest
   per-process start is ~10-20 ms. Merging small tests into table-driven ones saves at most ~85-170 test-s per
   train (~0.1-0.2 min): **not proposed**; it would also cost isolation.
2. **Twelve suites hold 77% of the test-seconds**, led by the math accuracy sweeps: pass 1 runs ~9,000 test-s
   of them (C-oracle sweeps 6,366: complex_ld_cr 3,403, complex_f128 1,196, special_ld 936, special_f128 646,
   complex 136, elem_ld 32, special 17; pure-Rust sweeps 2,643: special_cr 1,989, liella-math lib 374 (one test
   184 s), fast_gamma 280 (one test 343 s)), of pass 1's 11,990.
3. **Their setup is small; the check is the cost.** Shard 0 on keke: complex_ld_cr 2.8 s setup / 75.6 s
   check; complex_f128 1.8 / 65.4; special_ld 1.0 / 40.3; special_f128 1.1 / 21.3 (setup 1-4%). The oracle
   programs themselves run 0.01-2.4 s. Running the programs once per pass instead of once per shard saves
   ~180 test-s per train (~0.2 min): **rejected as a story** (folded into the cache below, which skips it).
   The check is the multiprecision oracle over the lines where Liella's result differs from the reference
   (3,879 lines in complex_ld shard 0, 4,920 in complex_f128 shard 0).
4. **They have no plant on record and no real-defect history after landing.** No plant in any handoff plants
   file runs these targets; the plant logs show only shard-plumbing plants (GATE-MATH-SHARD-3,
   CI-GATE-DIET-2: "shard N emptied", shard formula). In 328 CI gates none of them failed. In developer logs
   they failed only during the stories that wrote their functions (math-m5-cplx-f128-*, math-m5-cplx-ld-2-*,
   10-23..11-04; a link failure and new-function work). Their value is real (the only end-to-end proof that
   long double / _Float128 complex and special functions are correctly rounded, D-10.3) but it is realised
   when their inputs change, not on every train.
5. **Pass 2 re-runs xtask's 236 unit tests for nothing** (454 test-s). xtask has no dependencies
   (xtask/Cargo.toml: "deliberately has no dependencies"), so the exports-interposed build cannot change it,
   but CI-GATE-DIET's run-once rule keeps it because xtask's own source holds the string `liella_boundary`
   (nextest.rs BOUNDARY_MARK, main.rs, policy.rs, forkalloc.rs, hottext.rs). Pass 2 also ends on its slowest
   test (`affected::tests::csys_falls_back_to_full_when_unsure`, 51-80 s).
6. **Symbol tests re-run conformance programs** that `cargo xtask test` runs once anyway (its conformance step,
   xtask/src/main.rs:290), in both passes: s22_threads_symbols calls `conformance(&["s22_symbols"])` in 102 of
   its 201 tests, "s22_sync" in 41, "s22_more" in 37; s12_stdio_symbols has 110 calls, s01_startup_symbols 36
   (19 x s01_symbols_options), s18_processes_symbols 14. The products (libc.a, the start object) are the same in
   both passes, so each repeat runs the same program on the same products. All `_symbols` suites cost 933 test-s
   per train (5,252 runs), `_clauses` 825 (2,555); the conformance share is NOT VERIFIED (the brief measures it).

## Classification (suites >= 30 test-s per train; both passes; "p1/p2" = test-s per pass)
| suite | p1/p2 test-s | evidence | class | saving/train (est.) |
|---|---|---|---|---|
| liella-math::complex_ld_cr_products | 3403/4461 | no plants; 0 CI fails; dev fails only in its own M5 stories | MOVE (verdict cache) p1; p2 out of scope | with the 6 below: -4 to -6 min on trains that change no math input |
| liella-math::complex_f128_products | 1196/1723 | same | MOVE p1 | (in the row above) |
| liella-math::special_ld_products | 936/1179 | same | MOVE p1 | (above) |
| liella-math::special_f128_products | 646/1002 | same | MOVE p1 | (above) |
| liella-math::complex_products | 136/737 | same | MOVE p1 | (above) |
| liella-math::elem_ld_products | 32/40 | same | MOVE p1 | (above) |
| liella-math::special_products | 17/78 | same | MOVE p1 | (above) |
| liella-math::special_cr | 1989/0 | pure Rust (PASS0_ONLY); shard-plumbing plants only; 0 fails | MOVE (verdict cache) | (above) |
| liella-math (lib unit tests) | 374/0 | 18 plants run `--lib` filters (erf, fast32 bessel/gamma, wide_dd, trig) | MOVE only its slow sweeps (fast32 inverse_trig 184 s); keep the rest | (above) |
| liella-math::fast_gamma | 280/0 | 0 plants in files; 0 fails; one test 343 s, 3000 random operands | MOVE (verdict cache) | (above) |
| xtask::bin/xtask | 462/454 | 74 plants; 6 dev fails (real) | KEEP p1; CONSOLIDATE (run once) p2 | -0.5 to -1.3 min (-454 test-s and the pass-2 tail test) |
| liella-boundary::strstr_find | 144/464 | 7 plants; exhaustive length x position proof | KEEP | 0 |
| liella::aio_products | 281/272 | 6 distinct kernel configurations (uring, EPERM, ENOSYS); 1 plant | KEEP | 0 |
| liella-boundary::small_compare | 87/185 | 45 plants | KEEP | 0 |
| liella::s12_stdio_clauses | 172/176 | 23 plants in files, 71 host plant lines | KEEP | 0 |
| liella-boundary::s22_threads_symbols | 133/142 | 2 dev fails; 102 x the same program | CONSOLIDATE (conformance once per run) | with the 4 rows below: <= -0.6 min |
| liella-boundary::s12_stdio_symbols | 98/84 | 110 conformance calls | CONSOLIDATE | (above) |
| liella-boundary::s18_processes_symbols | 20/21 | 14 calls | CONSOLIDATE | (above) |
| liella-boundary::s01_startup_symbols | 18/12 | 36 calls | CONSOLIDATE | (above) |
| liella-boundary::s00_conventions_clauses | 45/41 | has its own conformance() | CONSOLIDATE (if its calls repeat) | (above) |
| liella::dns_products | 123/122 | single end-to-end product; no overlap found | KEEP | 0 |
| liella-boundary::heap_threads | 70/98 | 1 dev fail; statistics test 6 sizes x 100,000 samples (92 s), purpose of the count not stated | KEEP; SHRINK candidate only after plants | 0 now |
| liella-boundary::s10_math_symbols | 92/97 | 5+15 plants; 3 CI fails (mxcsr); 168 tests at ~0.58 s each, 1 conformance call | KEEP; per-test 0.58 s unexplained (INFORMATIONAL) | 0 |
| liella-rt::core_contracts | 70/76 | mutex stress 8 x 20,000 (75 s), COLD-REWRITE equivalence | KEEP | 0 |
| liella (lib) | 147/0 | 15 plants | KEEP | 0 |
| liella-boundary::small_copy | 43/82 | 37 plants; 3 dev fails | KEEP | 0 |
| liella::regex_subpatterns | 99/0 | regression guard of a cubic case (counted bounds) | KEEP | 0 |
| liella::time_gaps_probe | 43/41 | 3 CI + 4 dev fails | KEEP | 0 |
| liella::catalogs_products | 48/45 | 2-8 plants; 2 dev fails | KEEP | 0 |
| liella-boundary::wcsstr_find | 15/71 | exhaustive unit x alignment proof (STRSTR-VEC) | KEEP | 0 |
| liella::s22_threads_clauses, liella-math::s10_math_clauses | 44/39, 21/62 | clause tests; 3 plants (s10) | KEEP | 0 |
| liella-rt::heap_and_sync | 32/35 | 3 plants | KEEP | 0 |
| liella-boundary::machine_scanners | 27/34 | 65 plants (the most of any target) | KEEP | 0 |
| liella-boundary::reload | 24/30 | 4 plant lines | KEEP | 0 |
| liella::fstree/syslog/nscache/patterns_products, sync_eintr | 17-30 each per pass | single end-to-end products; nscache 3 dev fails, syslog 1 | KEEP | 0 |
| liella::sigev_thread | 24/24 | 1 CI fail | KEEP | 0 |
| liella-boundary::casecmp_avx2 | 17/19 | 4 plants (14 host lines) | KEEP | 0 |
| liella-boundary::search_nomem | 17/19 | 8 CI fails (6 + 2) | KEEP | 0 |

No suite is classed CUT: nothing measured adds no information. No SHRINK is proposed now: no plant evidence
exists for the big sweeps' math code, so no subset can be shown to catch the same plants; a SHRINK would first
need libm mutation plants run against the full sweeps (recorded in TVA-MATH-VERDICT-CACHE as a later option).

## Briefs (handoff/test-value-audit/)
- TVA-MATH-VERDICT-CACHE (MOVE, RULING NEEDED): skip a math sweep shard whose complete input key matches a
  green verdict; -4 to -6 min per train that changes no math input, 0 otherwise.
- TVA-XTASK-RUN-ONCE (CONSOLIDATE, no ruling needed: same cases, same binary): -0.5 to -1.3 min.
- TVA-CONFORMANCE-ONCE (CONSOLIDATE, no ruling needed: every program still runs once per run on the same
  products): <= -0.6 min.

## INFORMATIONAL
- strstr_find costs 3x more in pass 2 (464 vs 144 test-s), wcsstr_find 5x (71 vs 15): cause NOT VERIFIED
  (the interposed exports in the test process are the likely reason).
- aio_products: each of its 6 tests takes ~46 s in both passes; where the time goes is NOT VERIFIED.
- s10_math_symbols: 168 tests at ~0.58 s each with one conformance call: per-test cost NOT VERIFIED.
- xtask unit tests of 36-80 s (affected::tests::csys_falls_back_to_full_when_unsure, bench::tests::counters_
  files_hold_only_...) end both passes; ordering is GATE-NEXTEST-PRIORITY-REFRESH's.
- CI failures over 328 gates (fails.tsv): stdio_short_io pipe_getline_interrupted_then_resumes 9,
  search_nomem hcreate 6, xtask audit_modules_are_leaves 5, cold_rewrite_far02 tunables_probed_shapes 5,
  hot_text_products page-fault bounds 4+3+3; whether these were defects or flakes is not judged here.
- Plant logs: per-plant logs that name the failing test are kept for only 40 of ~1,700 plants (RAM dirs are
  cleared). Keeping a one-line "caught by <binary> <test>" per plant in the plants summary would make the next
  audit exact.

## Checks
`cargo xtask static` rc 0 on f9edbd3a (keke r5, foreground). No gate: planning story, no code change.
Jobs: keke:tva-setup2 rc 0 (the setup measurement); keke:tva-setup stopped by me (kill-label), no result used.

## Next step
Owner: rule on TVA-MATH-VERDICT-CACHE. Coordinator: TVA-XTASK-RUN-ONCE and TVA-CONFORMANCE-ONCE may be queued
without a ruling. Do not redo: the 40-train cost parse, the plant scan, the setup measurement (scripts in
mei/keke ~/work/test-value-audit and ~/work/TEST-VALUE-AUDIT).
