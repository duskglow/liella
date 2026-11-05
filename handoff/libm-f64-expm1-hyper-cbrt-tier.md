# LIBM-F64-EXPM1-HYPER-CBRT-TIER handoff (run 1: CHECKPOINT, WIP, not compiled)

Branch `task/libm-f64-expm1-hyper-cbrt-tier` (cut from origin/next 2879c355). Role kanon:~/w/kn1 (Broadwell-EP, FMA).
This run hit the context checkpoint (~145k of 180k) after 28 calls, so it **built nothing, synced nothing and ran no jobs**.
Decisions in force: D-10.13 (the accurate path is the reference), D-10.25 (FMA build through `dispatch::fma`), ruling #2760 A.

## State (WIP commit, NOT compiled, NOT wired)
* `crates/math/src/fast64x.rs` (new; `pub mod fast64x` in lib.rs): round-to-nearest in-place entries
  `expm1_nearest`, `log1p_nearest`, `sinh_nearest`, `cosh_nearest`, `tanh_nearest`, `cbrt_nearest`, each with its proof
  in the doc comment. Both builds come from one macro (`entries!`). The plain build uses `kern` = fast64's kernels with
  `dd::two_prod`. `fused` uses `fast64::fused`'s kernels with `dd::two_prod_fma`. Rounding is `fast64::settle` (Nearest).
  * expm1: `|x| < 2^-6` uses a degree-9 series with an exact head. Otherwise exp kernel, then `v = y·2^m`, `two_sum(v.hi, −1)`,
    then Fast2Sum (the folded −1).
  * log1p: `|x| < 2^-7` uses the log series with `r = x`. Otherwise `two_sum(1, x)`, then `log_kernel(w.hi) + w.lo/w.hi`.
    The domain is `−1 < x < 2^960` (keeps `w.lo/w.hi` normal).
  * sinh/tanh: `|x| < 2^-6` uses an odd series. Otherwise `exp_pm` builds `e^a` and `e^-a` from ONE reduction, as two kernel
    calls `(r, k)` and `(−r, −k)` (two table loads, not one: a deviation from the brief, worth measuring). Above 40 only `e^a/2` is used.
    tanh is `(p − qs)/(p + qs)` with one corrected division, error `1.01·E·2^m`.
  * cosh: `exp_pm` for all `|x| >= 2^-60`.
  * cbrt: `|x| = 2^(3q)·z`. Seed `CBRT_SEED − bits(z)/3` (`0x5540…`, 6%), then two degree-3 refinements of `(1−h)^(-1/3)`
    (no division), `y = z·g²`, an exact residual, one division for `δ`, and a run-time bound `1.02·δ²·g + 2^-50·|δ| + 2^-104·y`.
    It declines results with the low 35 significand bits clear (exact-cube check). The brief asked for one Newton step after
    the polynomial: one Newton step only reaches about 2^-22, so the second refinement is degree 3.
* `fast64.rs`: `exp_kernel`, `log_kernel` (both builds), `reduce_exp`, `log_eps` are now `pub(crate)`. No numeric change.
* Bounds: worst at the series/kernel switch, about 2^-63.6 relative (the `2^-49·|tail|` term of the series). It falls as
  `x²` below the switch and is about 2^-66 to 2^-70 above it. Accepted as "about 2^-66"; record it in the saturation row.

## Exact next steps (in order; about 45 calls)
1. Sync (`sync_role.sh --story … --base origin/next`), then run `cargo clippy -p liella-math --all-targets` on kn1 and fix
   compile errors. The things most likely to fail are the macro (`$k:ident` = local `mod kern` re-export; the target_feature
   fns called from target_feature fns) and the `odd_series` const generic.
2. Unit tests in fast64x (`#[cfg(test)] mod tests`, `no spec clause`, D-10.13): for each entry and both builds (fused only
   if `is_x86_feature_detected!("fma")`), check bit-equality with `exp_log::expm1` / `exp_log::log1p` / `hyper::*` /
   `pow::cbrt` in Nearest over LCG sweeps per range (series range, kernel range, negative expm1 down to −37, log1p near −1,
   cbrt over all exponents mod 3, plus exact cubes `n³·2^(3k)`). Also check that the answer rate is at least 99.9% (this
   catches the seed plant).
3. Wire them in: `crate::hot::{expm1,log1p,sinh,cosh,tanh,cbrt}_nearest` (copy `hot::exp_nearest`), then split the
   s10_math.tsv rows into `d` (elem_fast) and `flq`, then `cargo xtask math-gen` on the host.
4. Plants file (`cargo xtask plants`): bound (`eps` → 0 in expm1's kernel branch), folded −1 (drop `w.lo +`), `1 + x` split
   (drop `+ w.lo / w.hi`), seed constant (`0x5540` → `0x5500`, caught by the answer-rate test), perfect-cube check
   (mask → 0, caught by the exact-cube test).
5. Gate (direct tier, `--base origin/next`), counts before/after (`cbench_libm` rows expm1, log1p, sinh, cosh, tanh, cbrt,
   f64 and f32), then a docs/saturation row for any row still above 1.05x.
6. asinh/acosh/atanh are not started. The design is in the brief: log of `a + √(a²+1)` with a dd square root and the
   `log_kernel(W.hi) + W.lo/W.hi` fold; acosh near 1 uses `x²−1 = d(2+d)`; atanh is `½·log1p(2a/(1−a))`. The hardware `sqrtsd`
   must come from the boundary, as `fast32::asinh_nearest_sqrt` does.
7. Mode-taking variants hooked into the accurate paths (`fast(...)`), so the tier also serves directed modes: not done. Only
   the in-place (Nearest) entries exist.

## Not to redo
* I did not read the overlap branches' code (PERF-LIBM-EXPM1-LOG1P, PERF-LIBM-HYPERBOLIC); only their handoffs. Lessons kept:
  log1p must decline huge x (subnormal `w.lo/w.hi`); cbrt must decline results of ≤ 18 bits.
