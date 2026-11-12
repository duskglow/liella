# LIBM-CEXP-COLD-MIDDLE handoff (run 3: -3, design 1 landed per ruling #4912)

State: branch task/libm-cexp-cold-middle-3 (continues -2's branch; cut from origin/next 7eaa318c8, which has
LIBM-CEXP-ROUTE-2); HEAD in handoff/reports/LIBM-CEXP-COLD-MIDDLE-3.json. Role mei:~/w/s2 (Zen 1). Decisions in
force: D-10.99, D-10.53, rulings #4885 and #4912. Plants: handoff/LIBM-CEXP-COLD-MIDDLE-plants.txt.

## What the branch holds (design 1)

`crates/math/src/complex_fast.rs`: the hot in-place `cexp_nearest` (both builds) no longer calls `middle`; a part
its quick test declines makes it return `None`, and the boundary's existing cold fallback (`mathret::cplx_fast_cold`
-> `cplx` -> `complex::cexp` -> `complex_fast::cexp`) runs the quick tier WITH the middle tier. Results unchanged.
New in -3: `crates/boundary/tests/s10_math_symbols.rs::cexp_cold_fallback_matches_the_route` checks the export
against the 384-bit route (bits, flags, errno) for the bench operand (kernel declines, middle tier answers; the
MXCSR flag bits are exactly inexact there) and for operands outside the quick tier's range.

## Counts (wl_percall, validation/percall/cplx_exp.c, instructions per call, mei, base 7eaa318c8 primed there)

| | cexp | cexp_bench | cexpf | cexpf_bench |
|---|---|---|---|---|
| glibc | 250.8 | 249.8 | 172.6 | 170.5 |
| base 7eaa318c8 | 397.0 | 400.4 | 188.0 | 185.0 |
| design 1 (final head) | **396.0** | 402.1 | 188.0 | 185.0 |

Same numbers as -2 on the Broadwell role. Against the pre-ROUTE base (396 / 845) design 1 is as good or better on
both rows (ruling #4912's fallback reasoning).

## Design 4 (tried once, reverted: b2cd405cd, revert 68d684626)

A cexp-only `mathret::cplx_fast_retry` whose cold function first ran `complex_fast::cexp` in place (settle to
the caller's word | inexact), then the envelope. It could not be measured: the new generic code in the boundary
crate changed the code generation of three unrelated libc functions, and `cargo xtask so` refused the build
("hot-text.list: 3 key(s) match no function section": timefmt `TmRead::zone`, `catalog::textdomain`,
`nscache::real_secs`). Building it needs `tools/hottext/regen.sh . --build` in both profiles, a libc layout
change outside this story; per #4912 it was not pursued. Its expected gain was about 2 instructions on cexp_bench
only (one operand in 256). NEGATIVE result, not yet in the saturation register (no SR id reserved).

Probe-settled on the way: for (1, 1e10) (outside the quick tier's y range, envelope path) the export leaves the
MXCSR denormal-operand bit set (flags as fetestexcept sees them match the route). Not an exception flag; the base
does the same (same path); not chased.

## Next step

None for this story. Optional: if cexp_bench's +1.7 matters, design 4 together with a hot-text regen (both
profiles, LAYOUT rules apply), as its own story. Do not redo: designs 2 and 3 (-2: 412/417), design 4 (above).
