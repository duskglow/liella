# GATE-TIME-PROFILE: where the FULL CI gate spends its time (planning; no code changed)

State: branch task/gate-time-profile from origin/next 944c13c9 (the commit the last profiled train gated).
This story only measured. Decisions in force that it reads: GATE-SPEED / GATE-SPEED-3 / GATE-THREADS-REQUIRED
(.config/nextest.toml), CI-GATE-DIET (handoff/ci-gate-diet.md: run-once binaries, shards), D-10.12 (the remez check).

## Sources and method
- The 40 most recent green FULL CI gates (jobs.tsv kind `ci-gate`, rc 0, 2026-11-05 05:47Z .. 11-07 02:39Z) on the CI
  host: the `.log` (step times) and the `.affected.log` (nextest PASS lines with per-test seconds, the two `Summary [`
  lines, cargo `Finished` lines). Scripts (read-only): scratch `gtp.py` / `gtp2.py`, copies in mei:~/work/gate-time-profile/.
- The logs carry no timestamps between `cargo xtask test`'s steps. The steps of the last train (944c13c97) were
  dated from file mtimes in the CI tree's target/ (read-only `find -printf`), and the one unexplained step was timed
  by a measurement job: `gtp-mathgen-time` (jobrun on the CI host, own copy of 944c13c9 in ~/work, release remez,
  one fit at a time). It was stopped by me (rc 124, `kill-label`) after the per-fit timings were in: its remaining
  steps only repeated the whole-list remez run and timed `tables --check` (bounded below at <= ~20 s by the
  mtime gap).
- The jobs.tsv `cpu_s` column of a ci-gate counts the jobrun wrapper only (the gate runs in its own systemd scope),
  so the gate's real CPU use is NOT VERIFIED; the utilisation figure below is test-seconds / (wall x 24 threads).

## Phase table (40 green FULL trains; seconds)
| phase | p50 | p10 | p90 |
|---|---|---|---|
| job wall (jobs.tsv) | 2517 | 1746 | 3495 |
| clippy --workspace | 14 | 10 | 24 |
| `cargo xtask test` total | 2464 | 1709 | 3438 |
| nextest pass 1 (Summary) | 742 | 602 | 1129 |
| nextest pass 2, exports interposed (Summary) | 807 | 670 | 1318 |
| test builds (`Finished test`) | 41 | 25 | 71 |
| rest of `xtask test` (audits, generators, remez, products, doctests, conformance, native, rust-dylib, symgap) | 598 | 193 | 1020 |
| wrapper (job wall - clippy - xtask test) | 32 | 26 | 45 |
| pass 1 / pass 2 tests run | 5962 / 4928 | | |
| pass 1 / pass 2 test-seconds | 12100 / 12414 | | |
| pass 1 / pass 2 utilisation (test-s / (wall x 24)) | 0.68 / 0.63 | | |
| privileged group summed per pass (1 thread; aio 3 threads) | ~300 + aio ~280 | | |

Last train (944c13c97, 2441 s), dated from target/ mtimes:
| step | from | to | s |
|---|---|---|---|
| clippy | 02:39:30 | 02:39:47 | 16 |
| audits, trace, fuzz-check, csys/abi/unicode/locale/native/compat/math-gen | 02:39:47 | ~02:40:20 | ~33 |
| **remez --check, 22 fits, serial** | ~02:40:20 | ~02:53:00 | **~770** (measured 772) |
| tables --check, kernel index | (inside the gap) | | <= ~20 |
| so (products, profile fast) | 02:53 | 02:54:43 | ~90 |
| test build, pass 1, doctests | 02:54:43 | ~03:05:45 | 33 + 628 |
| pass-2 build + run-once listing, pass 2, doctests | ~03:05:45 | ~03:18:00 | ~60 + 677 |
| conformance, native, rust-dylib, symgap, status | ~03:18:00 | 03:20:11 | ~130 |

The "rest" row grew from 100-250 s (11-05, 2-5 fit files) to ~1000 s (11-06 on, 20-22 fit files): about 13 min
per train were added in two days by the remez check alone, and it grows with every new fit.

Remez `--check` per fit (s, one core, measured): fast32_tgamma 241.7, fast32_lgamma 167.2, fast32_erfcx2 74.4,
fast32_y1 58.2, fast32_y0 57.6, fast32_erfcx1 47.2, fast32_erfcx3 37.1, fast32_erfcx4 19.0, fast32_j1 18.0,
fast32_j0 17.6, fast32_erfcx5 14.6, fast32_y1_reg 6.2, fast32_y0_reg 5.6, the other 11 <= 1.8 each; sum 772.0.

## Serial vs parallel
- Serial: everything before pass 1 (~950 s on 11-07 trains, ~770 s of it one single-threaded remez process on a
  12-CPU slot) and the ~130 s after pass 2. That is ~45% of the gate.
- Each nextest pass: the privileged chain (one at a time; aio_products 3 at once) is ~300 s + >= ~95 s per pass,
  against pass walls of 742 / 807 s, so the passes are throughput-bound rather than chain-bound today. Average
  ~16 tests in flight on 12 CPUs.
- Tails: pass 1 ends on a special_ld_products shard (117-150 s, threads-required 4) and xtask unit tests
  `affected::tests::csys_falls_back_to_full_when_unsu...` (51-80 s) and `bench::tests::counters_files_hold_only_...`
  (36 s), none of which are in the GATE-SPEED priority list; pass 2 ends on the same two xtask tests.

## Where the test-seconds go (median per train)
Pass 1: complex_ld_cr_products 3210, special_cr 1969 (pass-1 only), complex_f128_products 1183, special_ld_products
913, special_f128_products 632, fast_gamma 556 (one test 343), xtask unit tests 497, liella-math lib 336, aio 280.
Pass 2: complex_ld_cr_products 4396, complex_f128_products 1735, special_ld_products 1142, special_f128_products 981,
complex_products 743, xtask 485, strstr_find 455, aio 272. The five math product targets are ~9000 of pass 2's
12414 test-seconds.

## 20 slowest tests (median s over the trains that ran them; p = pass)
1. p1 fast_gamma double_gamma_kernels_stay_within_their_bounds_and_rarely_fall_back 343 (p90 454; 20 trains)
2. p1 special_cr long_double_bessel ..._j0_s4 261
3. p2 complex_ld_cr_products ..._s15 258
4. p1 special_cr ..._j0_s12 258
5. p1 special_cr ..._j1_s4 254
6. p1 special_cr ..._j0_s3 254
7. p1 special_cr ..._j0_s13 254
8. p2 complex_ld_cr_products ..._s13 251
9. p1 special_cr ..._j1_s13 248
10. p2 complex_ld_cr_products ..._s8 248
11. p2 complex_ld_cr_products ..._s6 246
12. p2 complex_ld_cr_products ..._s2 244
13. p2 complex_ld_cr_products ..._s0 242
14. p2 complex_ld_cr_products ..._s14 242
15. p2 complex_ld_cr_products ..._s19 235
16. p2 complex_ld_cr_products ..._s11 231
17. p2 complex_f128_products ..._s7 228
18. p2 complex_f128_products ..._s0 223
19. p2 complex_ld_cr_products ..._s4 222
20. p2 complex_f128_products ..._s6 221
(pass-1 copies of the complex_ld_cr shards run 165-204 s; liella-math fast32::tests::inverse_trig_cores_are_within_their_bounds 184.)

## Ranked list (minutes per train; same coverage unless marked RULING)
1. **Remez check: content-hash verdict cache + one process per fit in parallel** -> about -12.5 min (-12.8 on a
   train that touches no fit, coefficient file or mathtools code; -8.8 when it does: longest fit 242 s). Brief:
   handoff/gate-time-profile/GATE-REMEZ-CHECK-CACHE.md.
2. **RULING NEEDED: pass 2 re-runs the math product sweeps** (complex_ld_cr, complex_f128, special_ld, special_f128,
   complex_products: ~9000 test-s). Putting them in `nextest::PASS0_ONLY` like special_cr changes what pass 2
   tests (their harness process would no longer run on the interposed exports) -> est. -5 to -6 min (pass 2 would
   drop toward its ~400 s privileged chain). NOT VERIFIED. Owner's call; no brief.
3. **Priority refresh for the tail** (ordering only): fast_gamma, the complex_f128 shards, special_ld/special_f128
   shards, liella-math fast32 inverse_trig, and the two xtask tail tests -> est. -1 to -3 min (both passes end on
   them today). NOT VERIFIED. Brief: handoff/gate-time-profile/GATE-NEXTEST-PRIORITY-REFRESH.md.
4. Overlap the ~130 s after pass 2 (conformance, native, rust-dylib, symgap) with pass 2, or the ~90 s products build
   with the generator checks -> <= ~2 min, competes for the same 12 CPUs; NOT VERIFIED; no brief (below threshold).
5. Between-pass overhead (two doctest invocations with 0 tests, run-once listing, pass-2 build): ~1.5 min; small.
6. Step timestamps in `cargo xtask test` output (one `xtask test: <step> <s>` line per step) so the next profile
   needs no mtime archaeology: no time saved, but item 1 went unseen for two days for lack of it.

Not cut: clippy (14 s), the audits/generators (~33 s), test builds (41 s). Variance: pass walls p90 is 1.5x p50
with the same tests (e.g. 11-06 03:10 pass 1 1377 s); cause NOT VERIFIED (host load from other slots is the
likely suspect; needs the gate scope's cpu.stat, which jobs.tsv does not record).

## Next step
Coordinator: queue GATE-REMEZ-CHECK-CACHE first (largest, coverage-neutral), then the priority refresh; take item 2
to the owner. Do not redo: the 40-train log parse (scripts in scratch / mei ~/work/gate-time-profile), the per-fit
timing.
