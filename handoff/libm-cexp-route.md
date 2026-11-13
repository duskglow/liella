# LIBM-CEXP-ROUTE handoff: cexp's middle tier (no bench operand falls to the 384-bit route)

State: branch task/libm-cexp-route-2 (LIBM-CEXP-ROUTE's code b0f8ab1f0 + merge of origin/next 019004ab5 at
65418f7ba; no code change in -2), HEAD in handoff/reports/LIBM-CEXP-ROUTE-2.json. Role shiki:~/w/sh1 (-1 ran on
keke:~/w/n3). Decisions in force: D-10.99 (this story, records/decisions/D-10/D-10.99.toml), D-10.53, D-10.55.

## Finding (probe job libm-cexp-route-probe1, bench-box test with a distance print)

cbench_libm's one declined cexp operand is (-1.609657171678382, -1.9221928333335072) =
(0xbff9c127e0e21690, 0xbffec14d45bc5e2d). Its real part e^x·cos y = -0.5506141599989904·2^-3 lies **2^-70.26**
(relative) from a midpoint; the quick tier's bound there is **2^-66.34** (the trig kernels' 2^-68 absolute over
|cos y| = 0.34). So it is NOT a hard case for correct rounding, only for the quick bound: about one part in a
thousand falls in that band, and each cost the full route (~115k instructions).

## Change

`crates/math/src/complex_fast.rs`: `middle` (cold, out of line, shared by the baseline and FMA builds) is
called only when a part's quick `round` declines. It recomputes that part near 2^-99: `wide_dd::exp_dd` (new
pub(crate) wrapper of the wide formats' exp kernel, 2^-100 relative), `sin_or_cos` (pi/2 in three doubles,
exact two_prods, Dd additions; Taylor series in Dd by `dd::horner`, 2^-101.4 proved, 2^-100 used), the Dd
product, eps = (A·B + A)·2^-99, then `fast64::settle` in every mode and `scale`. Proof in the comment block above
`Part`. Constants: computed exactly with Python fractions (π by Machin), scratch script consts.py (not in tree);
the bound test checks them against the 384-bit values.

Tests (complex_fast_tests.rs): `cexp_middle_tier_bound_covers_the_error`, `cexp_middle_tier_decides_the_bench_hard_case`
(all four modes + in place vs `cexp_route`), `cexp_middle_tier_declines_next_to_a_boundary` (plant target);
`cexp_quick_tier_declines_the_bench_box_only_when_forced` now checks declines against the middle bound and prints
each decline's distance.

## Jobs on the final head (LIBM-CEXP-ROUTE-2, shiki, all rc 0; scripts ~/work/LIBM-CEXP-ROUTE-2/ on shiki)

- `libm-cexp-route-2-check`: workspace clippy -D warnings, xtask static, records check, complex_fast unit tests.
- `libm-cexp-route-2-plants`: handoff/LIBM-CEXP-ROUTE-plants.txt through `cargo xtask plants` (zram copy):
  4 CAUGHT, 0 SURVIVED, no WARN lines.
- `libm-cexp-route-2-gate`: direct tier, --base origin/next: 6 suites ok (1 skipped: no test matched in
  liella-rt), 1513 passed, 0 failed; peak 2.2G.
- `libm-cexp-route-2-hottext` (hot_text_products), `libm-cexp-route-2-sweep` (cexp_quick_tier_sweep, 10^7, ok).
- `libm-cexp-route-2-percall`, `libm-cexp-route-2-counts`: below.

-1's keke jobs (check3, plants2, gate, sweep, hottext) were green on the pre-merge head; its percall failed rc 2
in one second because the WL base e167f4952 was not primed on keke (no code fault). -2 primed the new base
019004ab5 on shiki (`bash validation/wl_prime_base.sh shiki <sha>`, printed `primed`) before the percall.

## Counts (instructions per call, deterministic)

wl_percall, validation/percall/cplx_exp.c, static, base 019004ab5 (origin/next) vs new 65418f7ba vs glibc:

| case | glibc | base | new | new/glibc |
|---|---|---|---|---|
| cexp_bench | 249.8 | 845.0 | 400.4 | 1.60 |
| cexp | 250.8 | 396.0 | 397.0 | 1.58 |
| cexpf_bench | 170.5 | 185.0 | 185.0 | 1.09 |
| cexpf | 172.6 | 188.0 | 188.0 | 1.09 |

cbench_libm --compare --counters (dynamic): cexp 403.9 (glibc 248.2), cexpf 181.5 (glibc 166.9).

**cexp typical +1 (396 -> 397):** found by disassembling both products (objdump -dr of ~/wl/products/<sha>/libm.a,
function `complex_fast::fused::cexp_nearest`): the fused kernel now keeps `x` alive for the inline `middle` call,
so it stores xmm0 to the stack in the prologue (`vmovapd %xmm0,0x20(%rsp)`, frame 0x90 -> 0xc8) and the register
allocation shifts. The hot entry `cexp` itself is identical. Fix (not made: -2's brief said no code change):
call `middle` from the cold side (where the declined quick result already goes to the 384-bit route, the
`cplx_fast_cold` path) instead of inside `fused::cexp_nearest`, so the hot kernel is the base's again; then
re-run percall and expect cexp 396 and cexp_bench ~400.

## Next steps

1. RULING NEEDED (coordinator): land as is (cexp_bench -445, cexp +1), or a short code story to move the `middle`
   call to the cold path first (recommended: it removes the +1 at no cost; see above).
2. Booked timing on Ivy Bridge was not run: the common path's mix changes by one store; the periodic timing sweep
   can cover it.
3. cexp typical to <= 300 (quick tier's Dd assembly, see handoff/libm-cplx-exp-tier.md step 3); cexpf to <= 120.

Do not redo: the operand search, the bound derivation, the constants, the +1 diagnosis.
