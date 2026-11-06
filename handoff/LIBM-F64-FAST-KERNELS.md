# LIBM-F64-FAST-KERNELS handoff (run 3: the libm FMA decision moves to load time)

Branch `task/libm-f64-fast-kernels-3` (run 2's branch plus this run's commits; cut from origin/next 2c1e25b9).
Role kanon:~/w/kn1 (Broadwell-EP E5-2699 v4, FMA). Code head e976b48c. Decisions in force: D-10.13, D-10.25,
**D-10.50** (reserved by ruling #2965; replaces run 2's D-22.14 everywhere), rulings #2760 A, #932, #2964.

## Runs 1-2 (kept)
Run 1: the exp10 slice (`fast64::reduce_exp10` into the shared exp kernel, `exp10_nearest` + FMA build).
Run 2 (#2920): libm.a / libm.so.6 carry their own copy of `dispatch::FMA`, which libc's startup never wrote,
so no program had ever run an FMA build. Run 2 decided it on the first math call. Its final jobs on mei
(gate2 FULL, plants2, counts3) all ended rc 0.

## Run 3
* **Load-time decision (coordinator note (1), owner failure-path principle):** `csys/src/adapter/libm_init.c`
  (linked only with feature `libm`, build.rs) puts one `.init_array` entry, `liella_libm_start(argc, argv, envp)`
  (dispatch.rs, `export! { libm "LIELLA_PRIVATE" }`, local in both libraries). It applies `fma_wanted` to CPUID
  and the `GLIBC_TUNABLES` entry of **envp**. It does not use libc's `secure_getenv`, because a dynamic program's loader runs
  libm.so.6's constructors before `__libc_start_main` publishes `environ` (runtime.rs step 3). It asks
  `process::secure()` only when the entry exists (fails safe). `fma()` is now `FMA.load() != FMA_OFF`, the same
  one load and one test as libc. The static FMA byte starts OFF, which is the baseline and gives bit-identical results. There is no
  per-call check left anywhere. The `secure_getenv` extern (caller_alloc.rs) is gone. Checked: libm.a's single object
  and libm.so.6 both carry the entry (`.rela.init_array` -> `liella_libm_start`; DT_INIT_ARRAY, 8 bytes).
* **exp10 `-FMA` +1.1 (#2964), settled:** the cause was run 2's second test in `fma()` (the undecided byte and its cold call). With
  that test gone, exp10 `-FMA` counts 218.9 again, run 1's exact number. The FMA builds also gained 3-6 instr/call (the
  cold call cost spills on the inlined path, not just one cmp/jcc). Settled by count; no separate profile was run.
* **Test:** `libm_fma_select` (static `-lm`, user-mode instruction count): kanon FMA 1,204,032 vs `-FMA` 1,648,032.
  **NOT VERIFIED:** the dynamic path (loader-passed envp in libm.so.6). libm.so.6 cannot be loaded before phase 14.

## New libm baseline (cbench_libm instr/call, Liella / glibc, kanon, job libm-f64-fast-kernels-3-counts, head e976b48c)
| row | FMA default (run 2 -> run 3) | glibc FMA | `-FMA` (run 2 -> run 3) | glibc `-FMA` |
|---|---|---|---|---|
| exp | 129.3 -> **126.2** (1.88x) | 67.1 | 181.4 -> 181.3 | 77.1 |
| exp10 | 147.4 -> **141.4** (2.52x) | 56.1 | 220.0 -> **218.9** | 56.1 |
| log | 167.5 -> **161.4** (2.56x) | 63.1 | 217.6 -> 217.5 | 78.1 |
| exp2 / exp_upward | 186.3 / 407.4 | 51.1 / 67.1 | 186.3 / 407.2 | 51.2 / 77.1 |
| log2 / log10 | 228.6 / 242.7 | 63.1 / 102.9 | 228.6 / 242.7 | 83.2 / 118.3 |
| pow / sin / cos / tan / atan | 481.5 / 338.6 / 336.0 / 494.9 / 949.7 | 141.2 / 100.2 / 104.7 / 107.7 / 78.8 | same | 191.3 / 122.5 / 126.2 / 124.2 / 109.8 |
| sqrt / expf / logf | 33.1 / 70.2 / 77.1 | 17.1 / 39.1 / 43.1 | same | 17.1 / 43.1 / 50.1 |
The other rows are in `~/logs/libm-f64-fast-kernels-3-counts.log` on kanon (two tables: default, then `-FMA`).
**Timing (NOT booked):** this is the unbooked wall time from the same job on a loaded host, so it is only a guide. exp10 31.4 ns (FMA) / 42.5 ns (`-FMA`)
against glibc 6.8 ns, and exp 23.4 / 29.4 against 8.0. The time ratio (4.6x) is worse than the instruction ratio (2.5x).

## Not done / next step (in order)
1. Read the FULL gate `kanon:~/bin/jobwait libm-f64-fast-kernels-3-gate` (escalated FULL, as in runs 1-2).
2. TIME exp10 on a booked timing host (`ops timing-book`, coordinator note (4)). This run did not do it: the booking
   wait (30-60 min) did not fit the budget. The unbooked time gap (4.6x against 2.5x in instructions) makes this urgent.
3. Per-phase split of exp10 / exp in the FMA build (141.4 / 126.2 vs 56.1 / 67.1) before any redesign.
4. Trig and exp/log/pow redesigns are separate stories (LIBM-F64-TRIG-TIER, LIBM-F64-EXPLOGPOW-TIER).

## Jobs (kanon)
Final head e976b48c: libm-f64-fast-kernels-3-gate (FULL, left running), -plants (5/5 CAUGHT, file
handoff/LIBM-F64-FAST-KERNELS-3-plants.txt), -counts. Diagnostic only: -so (build check), -t1 (test, head f62bc304).
