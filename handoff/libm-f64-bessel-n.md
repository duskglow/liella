# LIBM-F64-BESSEL-N handoff (run 2: LIBM-F64-BESSEL-N-2)

State: branch `task/libm-f64-bessel-n-2` (stacks on `task/libm-f64-bessel-n`, cut from origin/next), head: see
`git log -1`; role shiki:~/w/sh1. Decision in force: D-10.76 (`records/decisions/D-10/D-10.76.toml`).

## What exists
* `crates/math/src/fast_bessel_n.rs` (submodule `fast_bessel::n`): recurrence tier for `jn`/`yn`/`jnf`/`ynf`, orders
  `|n| >= 2` (`|n| <= 1` go to the j0/j1/y0/y1 fast paths). Regimes (x > 0): yn forward from Y0, Y1; jn series when
  x² <= n+1; jn forward when n <= x; otherwise J_⌊x⌋ (forward) times v_n/v_⌊x⌋ (Miller). Rounding tests `near64`,
  `near32`. `special::jn`/`yn` try the tier, then `jn_accurate`/`yn_accurate` (the 384-bit bodies).
* Run 2 added `tight` (modulus bound): on the oscillatory side (m <= x) both recurrences run and
  `|Y_k J_m − J_k Y_m| <= M_k M_m <= M_m²` (Cauchy-Schwarz; M increasing in ν by Nicholson). M_m <= a + c·M_m² with
  a = |J̃_m|+|Ỹ_m|, c = (πx/2)(S_J+S_Y); the run-1 bounds give an a priori cap M0, and when c·M0 <= 1/4 and u = 4ca <= 1/2,
  M_m <= 2a/(1+√(1−u)) <= a(1+u). Before it, |J_k| <= 1 made the relative bound grow like x^1.5 (declines from x ~ 1e5:
  jn forward settled 172 of 293 sampled, yn x>=n 158 of 318); after, 293/293 and 318/318.
* `crates/math/tests/fast_bessel_n.rs`: (1) tier vs 384-bit path, bits and status, 4 modes, 600 operands per function
  over the regimes, settled share >= 95% per regime (measured: j series 158/158, j forward 293/293, j miller 149/149,
  y x<n 244/253, y x>=n 318/318; float all 100%); (2) the doubles around the first two zeros of J_n, Y_n (n in
  2,3,7,20,60), ±n; (3) kernel bounds vs the 240-bit oracle (worst 0.035 of the bound for jn, 0.069 for yn);
  (4) near64/near32 straddles; (5) `#[ignore]` sweep, `LIELLA_BESSEL_N_SWEEP` (default 10^7) per function, all cores.

## Error bounds (proof summary; module doc has the same)
* Forward: error δ injected at f_{k+1} reaches f_m as δ(πx/2)(Y_k J_m − J_k Y_m) (A&S 9.1.16); bounded with
  |J_k| <= min(1,(x/2)^k/k!) (A&S 9.1.60/9.1.62), |Y_k| <= M_k <= M_m <= |Y_m|+|J_m|; or by `tight` above. Step error
  <= 2^-100(|c f_k|+|f_{k+1}|) (Joldes-Muller-Popescu 2017). Y's own bound solves E <= a + βE (β <= 2^-20).
* Backward: r_k = J_k/J_{k−1} in (0, q_k], q_k = x/(2k−x); ε_k <= 2^-99 + q_k q_{k+1} ε_{k+1}, ε_{N+1}=1; N with
  (n−m)Π q_j q_{j+1} <= 2^-110 (Gautschi 1967).
* Series: alternating, ratio <= 1/4, sum in [3/4,1].

## Declines (go to the 384-bit path, by design)
Orders > 4096; |x| outside [2^-990, 2^500]; forward values > 2^960 (yn near overflow); oscillatory side above about
x = 2^100 (the a priori cap fails); f64 operands within a few ulps of a zero of J_n/Y_n (tier settles 0 of 1920 there
for jn/yn: the absolute bound ~2^-100 is above the ulp of the tiny value; float settles all).

## Results of run 2 (head e4fe94f14 code; later commits are handoff/report only)
* clippy (workspace, all targets, -D warnings), `cargo xtask clippy`, `cargo xtask static`: clean
  (libm-f64-bessel-n-2-lint for clippy, libm-f64-bessel-n-2-static).
* Gate `cargo xtask affected --base origin/next --run`: ok, peak 2.5G (libm-f64-bessel-n-2-gate, 1815 s).
* Plants (handoff/LIBM-F64-BESSEL-N-plants.txt): 4 CAUGHT with msg (direction forced up; Miller normalisation
  dropped; near64 bound zeroed; modulus bound shrunk 2^20) (libm-f64-bessel-n-2-plants).
* 10^7 sweep per function x 4 modes vs the 384-bit path (libm-f64-bessel-n-2-acc): every settled result equal.
  jn settled 39,997,332 of 40M; yn 37,092,301 (declines in x<n: near-overflow region); jnf 29,002,812 (declines are
  subnormal float results in the series/Miller regimes: near32 takes normal results only); ynf 32,927,812 (declines: x<n, results beyond the float range).
* Counts `cargo xtask bench --compare --counters cbench_libm` (libm-f64-bessel-n-2-counts; Zen 1 build host, not
  booked): instructions/call Liella vs glibc: jn 2949 vs 976 (3.02x), yn 2455 vs 981 (2.50x), jnf 2896 vs 698
  (4.15x), ynf 2379 vs 698 (3.41x); before: the 384-bit route, ~155k (brief). Unbooked time ratio on that host:
  jn 4.12, yn 3.30, jnf 5.65, ynf 4.38. Target <= 3x time: NOT reached yet.

## Open / next step
1. Booked timing on Ivy Bridge (nico or kotori, less busy) and N100 was NOT done in run 2 (budget): next run.
2. Toward <= 3x: profile one call per phase (base kernels vs recurrence vs rounding test); candidates: yn on the
   oscillatory side runs both recurrences (only `tight` needs J: skip it when the plain bound settles); jnf/ynf could
   use a float-only bound (2^-30 relative is enough) with fewer double-double steps; near32 for subnormal results.
3. Possible follow-ups: generic-order Hankel expansion (DLMF 10.17(iii) remainder bound) for x beyond 2^100; a
   cheaper yn on the oscillatory side (it runs both recurrences now).

## Not to redo
The Casoratian runtime bound (interval/absolute recurrences blow up by (1+√2)^n). Landau-type x^{-1/3} bounds on J_ν
were considered for the oscillatory side and rejected: their stated constant does not hold for ν = 0, 1 near the
first maximum, so they need a source check before use; `tight` needs no new theorem.
