# LIBM-F64-LOG-QUICK(-2): binary64 log quick phase (tier 1), wired, gated

State: branch `task/libm-f64-log-quick-2` (stacks on `task/libm-f64-log-quick`, cut from origin/next at 799313e1e),
code head 3d8ca0c6a; handoff commits on top. Role mei:~/w/s3. Brief `/home/claude/state/briefs/LIBM-F64-LOG-QUICK.md`
("## The change"), continuation LIBM-F64-LOG-QUICK-2. Decisions in force: D-10.51 (two-phase double), D-10.60 (this
kernel: method, table, bound, measured numbers; coordinator ruling #3970 accepted degree 9), D-10.25/D-10.50 (two
builds), D-10.3 (correct rounding).

## What is in (log only; log2/log10/log1p not started)

* `crates/math/src/fast64/quick_log.rs`: Tang's table-driven log, 128 intervals of `m` in `[45/64, 90/64)`, 8-bit
  reciprocals so `r = c·m − 1` is exact (Gal & Bachelis), `−ln c` and `ln 2` high parts on the 2^-42 grid, Taylor
  degree 9 in Estrin's scheme, `r − r²/2` as an exact pair, Ziv's one-sided test, `RHO = 2^-64`. SSE2 and `fused`
  FMA builds. `log_tiered` = quick phase, then `fast64::log_nearest` (tier 2, unchanged) on a miss or outside the
  domain. Table `LOG_Q128` from `crates/mathtools/src/tables.rs::log_quick` (`cargo xtask tables`).
* Wired: `crates/boundary/src/hot/exp_log.rs::log_nearest`; selftest hooks `log_quick`, `log_tiered`,
  `log_quick_parts`. Tests (clause s10_0_f_0): `quick_log/tests.rs`, `crates/boundary/tests/log_quick.rs`.
* Later refinement (ruling #3970): a rigorous minimax fit (Brisebarre-Chevillard) that saves a term or two; degree 6
  Taylor leaves 2^-45 next to 1, so any lower degree needs a fit relative to `r`, not Taylor.

## What -2 fixed (why the first run's jobs were red)

* Gate rc 3: clippy (doc list continuation; `i64::cast_signed` above the 1.85 MSRV, now `i64::from_ne_bytes`;
  `panic!` in two tests, now `assert!` + `unwrap_or_default`). Then static: two boundary tests without a clause ID,
  a lab host name in D-10.60 (now "Skylake-SP host"), KERNELS.md stale (regenerated with `cargo xtask kernel-index`).
* Plants rc 2 (baseline failed): the hard-case test's non-vacuity check found no misrounded unchecked `zh` in 2·10^6
  operands away from 1, where the error is far below RHO. Cause, not a loosened check: the sample now also covers
  `c_j = 1` next to 1 (case A), where the error is close to the bound; the near-dropped plant is CAUGHT with it.

## Step 1 (the serialising step): trace and counts, timing pair NOT done

`wl_percall` (spec mei:~/work/LIBM-F64-LOG-QUICK-2/log_family.c, FMA build, head 580b567a7 = same code): instructions
per call Liella / glibc: log 97.0 / 66.0 (1.47; brief's before 190), log2 195.2 / 66.0, log10 205.4 / 105.9, log1p
245.0 / 86.0 (the last three unchanged by this story). `wl_trace` of one log call: 87 instructions = entry `logf64`
18 + `liella_machine_mxcsr_get` 3 + a 4-instruction hop into the FMA build + the kernel about 62 (ends in the Ziv
compare, no division). The `--list` listing kept only the last 16 instructions, so whether the entry writes MXCSR was
not read from the trace (NOT VERIFIED). The envelope (21 of 87) is the same one LIBM-F64-EXP-QUICK-WIRE measured; its
two items (inline `stmxcsr`, register-only `black_box`) still wait on rulings #3938/#3939. The proto_time
latency/throughput pair was not run (budget went to the red-gate fixes).

## Jobs on the final code head 3d8ca0c6a (mei unless named)

* `libm-f64-log-quick-2-plants4`: 5 of 5 CAUGHT, no WARN (`handoff/LIBM-F64-LOG-QUICK-plants.txt`: near-dropped,
  index-off-by-one, ln2-low-dropped, coeff-one-ulp, fma-square-low-dropped).
* `libm-f64-log-quick-2-gate4`: full clippy, xtask clippy, static, the new tests, `affected --base origin/next --run`.
* `libm-f64-log-quick-2-counts4`: cbench_libm `--compare --counters`, plain then `-FMA`
  (mei:~/work/LIBM-F64-LOG-QUICK-2/counters-{fma,sse2}.tsv). Before (same base, LIBM-F64-EXP-QUICK-WIRE-2's table):
  log 182.5 / 63.1 instructions.
* `libm-f64-log-quick-2-stage4` (tarball sha256 afc17a9e...), timing `kotori:libm-f64-log-quick-2-ivb` and
  `nozomi:libm-f64-log-quick-2-n100` (`./xtask-bin sweep run --only cbench_libm`, bookings 126/127 cancelled after).
  Ratios Liella/glibc, instr / cycles / time (spread, rounds): Ivy Bridge log 1.552 / 1.948 / 1.935 (0.196, 5),
  before 5.18; N100 log 1.477 / 2.137 / 2.113 (0.091, 3), before 5.88. Target 1.6x NOT reached. Unchanged rows as
  a control: log2 4.55 / 5.56, log1p 3.67 / 3.79, log10 3.61 / 3.52 (IVB / N100 time). exp 1.69 / 2.17.
* Superseded: `-gate` (rc 3 clippy), `-gate2` (rc 3 clippy), `-gate3` (rc 5 static), `-plants`, `-plants2`,
  `-plants3` (older heads), `-counts`, `-counts2`, `-counts3`, `-stage`, `-stage3` (stopped with their gate),
  `-trace`, `-kindex` (helper). `-trace2` is the step-1 evidence above.

At hand-back (13:24 UTC) gate4 had passed lint, static, the new tests and the affected suites liella-boundary (both
modes), liella-legacy and liella-math (1641 s at load 44), and was still running; counts4 waits on it. The instruction
claim rests on wl_percall above (log 97.0 vs 66.0 per call, FMA build); the SSE2 build's count is in counters-sse2.tsv
once counts4 ends (NOT VERIFIED here; LIBM-F64-EXP-QUICK-WIRE found the `-FMA` tunable may not reach cbench).

## Next steps

1. If timing shows log still above 1.6x: the envelope rulings #3938/#3939, then the latency/throughput pair
   (`handoff/libm-rethink/proto/proto_time.c`) to see whether the kernel or the entry serialises consecutive calls.
2. log2 / log10 on the same table (`Lh`/`Ll`, `Kh`/`Kl` pre-scaled by 1/ln b, or a scaled final double-double
   product), log1p (Fast2Sum of 1 + x into this kernel, own short series for |x| < 2^-5): separate -WIRE stories.

## Not to redo

The table design (8-bit c, exact r in both builds), the bound derivation and its test, the fallback model, the
hard-case sample next to 1.
