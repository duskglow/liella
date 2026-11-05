# LIBM-SPECIAL-FAST-TIER handoff (run 2: LIBM-SPECIAL-FAST-TIER-2, erf/erfc double done)

State: branch `task/libm-special-fast-tier-2` (stacked on run 1's `task/libm-special-fast-tier`, cut from origin/next
984a2ddc), role shiki:~/w/sh1 (Haswell E5-2697 v3). Report: handoff/libm-special-fast-tier-2-report.md and
handoff/reports/LIBM-SPECIAL-FAST-TIER-2.json. Accuracy contract unchanged: results stay correctly rounded (Ziv
first step + unchanged mp fallback), so no differences row changes (ruling #2760 option A).

## Done (erf/erfc, double)
* `crates/math/src/fast_special.rs`: evaluator + proof in the module doc. `erf_kernel(x)` / `erfc_kernel(x)` return
  `(y, eps, m)`; `erf` / `erfc` round with `fast64::settle`. Wired in `special::erf/erfc` for `F::PREC == 53`
  (`special::erf_accurate/erfc_accurate` = the old 384-bit body, public for the tests).
* `fast64::exp_dd` (exp kernel with pow's double-double reduction) and two fixes to `settle`'s subnormal branch
  (cap 2^52 instead of 2^51; ties of a half-integer `z.hi` adjusted): results in `[2^-1023, 2^-1022)` settle now.
* Tests `crates/math/tests/fast_special.rs` (3 tests, ~2 s): oracle in 4 modes (random, slot edges, switches,
  subnormal), bit+status identity with the mp path (`LIELLA_ERF_SWEEP=N` per function and mode, default 2000),
  kernel error within bound on every slot (worst 0.19 / 0.10 of eps), fallback ceiling 1/10^4, the 2^-1022 binade.
* Bound 2^-71 (ruling #2916). Measured fallback (10^5 random calls each, Nearest/Up): erf 1, erfc 0.

## PATTERN for LIBM-SPECIAL-GAMMA-TIER / -BESSEL-TIER
* Tables: `crates/mathtools/src/special_tables.rs` (`emit`, called by `cargo xtask tables`): slots of 16 a binade,
  index `(bits >> 48) - base`, centre `bits & 0xffff<<48 | 1<<47` (t = x - c exact), row
  `[P0h,P0l..P3h,P3l,p4..p14]`, per-slot bound (truncation + f64 Horner + dd steps) relative to the slot minimum,
  table eps = max, `choose` picks (M, D) for 2^-71.
* Evaluator: `fast_special::taylor(row, t)`; result `(y, eps_abs, m)` with `y.hi` normalized into [1, 2) when the
  result can be subnormal; `fast64::settle(y, eps, m, mode)`; negative results via the mirrored mode.
* Wiring: `if F::PREC == 53 { from_fast(name, fast(..)) }` then `<f>_accurate`. Tests: copy fast_special.rs
  (oracle check, accurate identity, kernel-bound ratio per slot, `stats` fallback ceiling).

## Counts (Haswell host, instr/call, Liella / reference)
erf 174,247 -> 377.9 / 153.8 (2.46x); erfc 174,738 -> 440.6 / 157.8 (2.79x); erff, erfcf unchanged (mp).

## Not done / next steps
0. Target 1.05x not met: profile one erf call per phase (validation/wl_percall.sh) before any change.
1. Float (`erff`/`erfcf`, still mp, ~174k instr): round the same dd result to float with a float rounding test.
2. Optional: the `mret::elem` entry overhead (see counts in the report); 10^7 accuracy sweep job
   (`LIELLA_ERF_SWEEP=2500000 cargo test -p liella-math --release --test fast_special matches`).
3. Gamma, Bessel: separate stories (ruling #2915).

## Things not to redo
* Degree/slot choice (run 1): 16 slots a binade, D 14, M 4 is the cheapest layout meeting 2^-71.
* The fallbacks seen first (8 and 7 in 10^5) were all subnormal results near 2^-1022 refused by `settle`; fixed.
