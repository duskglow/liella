# LIBM-F64-FAST-KERNELS handoff (run 2: the libm FMA selection, #2920)

Branch `task/libm-f64-fast-kernels-2` (run 1's branch `task/libm-f64-fast-kernels` plus this run's commits;
cut from origin/next 2c1e25b9). Role mei:~/w/s1 (Skylake-SP, FMA). Decisions in force: D-10.13, D-10.25,
**D-NEW** (placeholder id, the libm product's own FMA decision: plan/07-decisions/s10-mathematics.md, last row),
rulings #2760 A, #932.

## Run 1 (kept): the exp10 slice
`fast64::reduce_exp10` (double Cody-Waite log10(2)/64 into the shared exp kernel), in-place entry
`exp10_nearest` + FMA build. Its gate `libm-f64-fast-kernels-gate` (FULL, s3) ended rc 0; plants rc 0.

## Run 2: #2920 settled
* **Cause:** `libm.a` / `libm.so.6` link their own copy of `liella-boundary` (crates/libm-so), with every
  non-export symbol made local (`xtask so` run_libm). The `dispatch::FMA` byte that libc's startup sets
  (`cpu::init_with`) is libc's copy; libm's copy was never written, so every program ran the baseline
  builds of exp, exp10 and log (cbench_libm counted the same with and without `-FMA`).
* **Fix:** in the libm build the byte starts undecided; the first math call decides it with the startup's rule
  (`dispatch::fma_wanted`: CPUID FMA with the OS-saved AVX state, and `glibc.cpu.hwcaps` from `GLIBC_TUNABLES`
  read through libc's `secure_getenv`: `caller_alloc::secure_getenv_addr` (extern category) and
  `conv::env_value_text` (slice category)). No new symbol. `fma()` tests OFF first: the baseline costs one load and
  one test as before (an FMA-first order measured +3-4 instructions on the `-FMA` rows; fixed in ddaed22f).
* **Test:** `crates/boundary/tests/libm_fma_select.rs`: a static `-lm` program counts its own user-mode
  instructions (perf_event_open) over 4000 exp + 4000 log calls with and without `-FMA`; on mei 1,220,031 vs
  1,672,031. Skips with a printed reason without FMA or without the counter.

## Counts (cbench_libm, instr/call Liella / glibc; mei; job libm-f64-fast-kernels-2-counts3, final head)
| row | before run 2 (FMA = -FMA) | FMA default | -FMA | glibc FMA / -FMA |
|---|---|---|---|---|
| exp | 181.3 | **129.3** (1.93x) | 181.4 | 67.1 / 77.3 |
| exp10 | 218.9 | **147.4** (2.63x) | 220.0 | 56.1 / 56.1 |
| log | 217.5 | **167.5** (2.65x) | 217.6 | 63.1 / 78.3 |
| exp2, exp_upward, log2, log10, pow, sin | 186.3, 407.1, -, -, 481.4, 340 | 186.4, 407.2, 228.7, 242.9, 482.1, 338.6 | same | unchanged rows |
exp10 `-FMA` is +1.1 over run 1 (218.9 -> 220.0), not explained (not profiled: budget). The other `-FMA` rows
are within 0.1. Superseded counts: counts2 (FMA-first order: exp 126.3 / 184.4, exp10 144.4 / 223.0, log 164.5 / 220.6).

## Not done / next step (in order)
1. Read the final gate `mei:~/bin/jobwait libm-f64-fast-kernels-2-gate2` (FULL: the branch carries s10_math.tsv
   and gen.rs from run 1) and plants2 if not read in the hand-back.
2. Profile exp10 `-FMA` +1.1 (wl_percall, base primed) before anything else on that row; then the per-phase split
   of exp10 / exp (FMA build now: 147.4 / 129.3 vs glibc 56.1 / 67.1) to push exp10 toward 1.05x.
3. The `decision-not-stored` plant (drop the `FMA.store` in `decide_fma`) was left out: it only re-decides per
   call (slower, same selection), which libm_fma_select cannot see; an instruction ceiling per call would catch it.
4. Trig and exp/log/pow redesigns are separate stories (LIBM-F64-TRIG-TIER, LIBM-F64-EXPLOGPOW-TIER).

## Jobs (mei)
Final head: libm-f64-fast-kernels-2-gate2, -plants2 (file handoff/LIBM-F64-FAST-KERNELS-2-plants.txt: 3 new
plants on the libm decision + run 1's 6), -counts3. Superseded: check1 (unsafe audit: categories not permitted
in dispatch.rs, fixed), check2 (green, older head), counts (role tree is not a git checkout), counts2, gate and
plants (older head, stopped).
