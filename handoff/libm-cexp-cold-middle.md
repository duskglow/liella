# LIBM-CEXP-COLD-MIDDLE handoff (run 2: -2, INCOMPLETE, ruling needed)

State: branch task/libm-cexp-cold-middle-2, cut from origin/next 7eaa318c8 (has LIBM-CEXP-ROUTE-2); HEAD in
handoff/reports/LIBM-CEXP-COLD-MIDDLE-2.json. Role kanon:~/w/kn1 (Broadwell Xeon). Decisions in force: D-10.99
(body corrected here), D-10.53, ruling #4885. Plants: handoff/LIBM-CEXP-COLD-MIDDLE-plants.txt.

## What the branch holds (design 1, the final head)

`crates/math/src/complex_fast.rs`: the hot in-place `cexp_nearest` (both builds, `cexp_kernels!`) no longer calls
`middle`; a part its quick test declines makes it return `None`. The boundary's existing cold fallback
(`mathret::cplx_fast_cold` -> `cplx` -> `complex::cexp` -> `cexp_any` -> `complex_fast::cexp`) already runs the
quick tier WITH the middle tier, so results are unchanged (same bits; the tests check in place vs `cexp` vs the
384-bit route). Tests (complex_fast_tests.rs): `compare` accepts an in-place decline only where the quick test
declines a part; `cexp_middle_tier_decides_the_bench_hard_case` asserts the in-place kernel declines the bench
hard case (guard against the call coming back inline); the bench-box decline test counts declines of the tier
with its middle part. D-10.99's body says who calls the middle tier.

## Counts (wl_percall, validation/percall/cplx_exp.c, instructions per call, base 7eaa318c8 on kanon)

| design (head) | cexp | cexp_bench | cexpf | cexpf_bench |
|---|---|---|---|---|
| glibc | 250.8 | 249.8 | 172.6 | 170.5 |
| base next 7eaa318c8 | 397.0 | 400.4 | 188.0 | 185.0 |
| 1: hot kernel declines, envelope decides (fdd334707 = final head 1611683ce) | **396.0** | 402.1 | 188.0 | 185.0 |
| 2: cold 2nd attempt per build in the boundary wrapper (6ee005f30) | 412.0 | 416.9 | 188.0 | 185.0 |
| 3: same, one baseline `cexp_nearest_middle` (a13776374) | 412.0 | 417.8 | 188.0 | 185.0 |

Design 1 meets the typical-row target (396) but cexp_bench is +1.7: the one declined bench operand (of 256) now
pays the envelope plus the baseline `parts` again, about 690 instructions more than the inline middle call.
Designs 2/3 put a `#[cold] #[inline(never)]` call after the kernel in `hot::cexp_nearest` (inlined into the
export): every call got ~15 instructions dearer. 3 ruled out my first guess (FMA `parts`, only `#[inline]`, being
outlined by a second caller); the cost sits in the export. NOT VERIFIED: no disassembly was done (budget).

## RULING NEEDED

Neither design meets both targets (typical <= 396 AND cexp_bench <= 400.4). Recommend: do not land design 1 alone
(it trades -1 on the typical row for +1.7 on the bench row); a -3 continuation does design 4 below. Alternative
for the coordinator: accept design 1 (typical rows weigh more than one hard operand in 256).

## Exact next step (design 4, untested)

Keep design 1's hot kernel and leave the export's hot part as it is (kernel, `is_some`, tail to the cold
function). Put the second attempt INSIDE the cold fallback, before the envelope: a cexp-only `cplx_fast`
variant (or an extra `retry` closure argument) whose cold function first calls
`complex_fast::cexp_nearest_middle(x, y)` (= `Some(cexp(x, y, Rounding::Nearest)?.0)`, cold, in place, as in
a13776374) and only then settles MXCSR and runs `cplx`. The export is `@generated` (`cargo xtask math-gen` from
crates/boundary/src/posix/s10_math.tsv): change the generator/tsv, not the file. Before measuring, objdump
`cexp` in ~/wl/products/<sha>/libm.a for design 1 and design 4 to confirm the hot part is the same. Expected:
cexp 396, cexp_bench about 400.4 - 1 + (baseline parts)/256.

Do not redo: designs 2 and 3 (measured above), the ROUTE operand search and +1 diagnosis.
