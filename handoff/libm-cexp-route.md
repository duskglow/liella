# LIBM-CEXP-ROUTE handoff: cexp's middle tier (no bench operand falls to the 384-bit route)

State: branch task/libm-cexp-route (cut from origin/next e167f4952), HEAD in handoff/reports/LIBM-CEXP-ROUTE.json.
Role keke:~/w/n3. Decisions in force: D-10.99 (this story, records/decisions/D-10/D-10.99.toml), D-10.53, D-10.55.

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

## Jobs (keke, chain ~/work/LIBM-CEXP-ROUTE/chain.sh, sequential in the role; plants in /dev/shm)

check3 (complex_fast tests, workspace clippy -D warnings, xtask static, records check) -> gate (direct tier,
--base origin/next) -> sweep (cexp_quick_tier_sweep: 10^7, cexp and cexpf, four modes) -> hottext
(hot_text_products) -> percall (wl_percall, validation/percall/cplx_exp.c, static) -> counts (cbench_libm
--compare --counters, dynamic). plants2: handoff/LIBM-CEXP-ROUTE-plants.txt (4 plants). Labels: prefix
`libm-cexp-route-`. Superseded: probe1 (diagnostic), check1 (bound-tightness threshold), check2 (clippy panic lint),
plants (killed: stale tree). Results: see the report json / hand-back.

## Expected counts

cexp_bench should drop by ~450 (846 -> ~400; the 1-in-256 route call becomes a middle-tier call of ~1-2k).
cexp (typical 32) and cexpf unchanged (the hot path gains only a never-taken branch to `middle`).

## Next steps

1. Read the chain's results: `~/bin/jobwait libm-cexp-route-gate libm-cexp-route-sweep libm-cexp-route-hottext
   libm-cexp-route-percall libm-cexp-route-counts libm-cexp-route-plants2` on keke.
2. Booked timing on Ivy Bridge (nico or kotori, whichever queue is shorter) was NOT run (context budget): the
   common path's instruction mix is unchanged, so the periodic timing sweep can cover it; book it if the
   coordinator wants it in this story.
3. cexp typical to <= 300 (quick tier's Dd assembly, see handoff/libm-cplx-exp-tier.md step 3); cexpf to <= 120.

Do not redo: the operand search, the bound derivation, the constants.
