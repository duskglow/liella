# LIBM-JNF-LARGE handoff (run 1: INCOMPLETE, two typical rows a few instructions slower)

State: branch `task/libm-jnf-large` from origin/next 630f3afb2, product head 3d5b50c89 (later commits handoff/ only).
Role mei:~/w/s1 (Zen 1). Brief: ~/state/briefs/LIBM-JNF-LARGE.md. Decision in force: D-10.109 (large band); new
pending record records/pending/LIBM-JNF-LARGE-1.toml (D-NEW, series D-10).

## The change (3d5b50c89)
`special::jn` / `special::yn`, `float` only: `if n.unsigned_abs() <= 1` answer order 0 by `j0`/`y0`, order 1 by `j1`/`y1`,
order -1 by `j1(-x)` (NaN: accurate path, keeps its sign) and `-y1(x)` in the mirrored mode (finite x > 0; else the
accurate path). So jnf/ynf orders 0, ±1 take the fast path, then the large band and its table (bessel32_rest), exactly
as j0f..y1f. `fast_bessel_n::jnf/ynf` are next's again (3 lines of doc only); `fast_bessel::mirrored` is pub(crate).
Tests: `jnf_ynf_low_orders_take_the_large_band` (gate; band and off-band x, orders -1..1, 4 modes, against
jn_accurate/yn_accurate), `jnf_ynf_low_orders_are_the_order_functions_exhaustively` (ignored, long-sweeps Exhaustive
group: every float, 4 modes, bit and flag equality with j0f/j1f/y0f/y1f, order -1 against the mirrored identity).
Percall rows added to validation/percall/libm_bessel.c (bes_jnf0/1/m1_large, bes_ynf0/1_large, bes_jnf0_mid,
bes_ynf1_mid, bes_jnf3_mid, bes_ynf3_mid).

## Counts (wl_percall, instr/call, mei Zen 1, base next 630f3afb2, glibc dynamic). Three forms tried:
| row | glibc | next | v1 e1de849 (match n in special) | v2 01668d9 (in fbn order match) | v3 3d5b50c (one range test) |
|---|---|---|---|---|---|
| jnf0_large | 329.2 | 175480 | 428.5 | 449.5 | 434.5 |
| jnf1_large | 326.7 | 178368 | 431.0 | 460.0 | 437.0 |
| jnfm1_large | 329.8 | 178370 | 431.0 | 465.0 | 441.0 |
| ynf0_large | 341.5 | 175474 | 433.0 | 456.0 | 436.0 |
| ynf1_large | 383.2 | 178367 | 444.5 | 473.5 | 447.5 |
| jnf0_mid | 319.5 | 268.3 | 256.3 | 276.3 | 262.3 |
| ynf1_mid | 367.3 | 291.6 | 292.7 | 321.7 | 295.7 |
| jnf3_mid | 693.0 | 2747.1 | 2748.1 | 2745.1 | 2749.1 |
| ynf3_mid | 701.8 | 2411.6 | 2418.6 | 2413.6 | 2396.6 |
j0f rows unchanged (405.5 large, 233.3 mid). Self-check: base and every head give the same values; glibc differs in
the last digits for jnf(±1,1e6), ynf(0,1e6), ynf(1,1.5), jnf(3,1.5) (pre-existing, its column is no reference).
SSE2 (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`): job libm-jnf-large-percall4-sse2, not read in this run.

## Why INCOMPLETE
The brief: typical rows of a touched function must not get slower. v3 leaves jnf3_mid +2.0 and ynf1_mid +4.1 (v1:
jnf3 +1.0, ynf3 +7.0, ynf1 +1.1). Not profiled (NOT VERIFIED where the 2-4 instructions go; likely the inlined
j0..y1 bodies changing the frame/register allocation of special::jn/yn<f32>). Next step: wl_trace.sh one call of
bes_ynf1_mid and bes_jnf3_mid on v3 vs next; candidates: `#[inline(never)]` low-order helper (one call for orders
0/±1, the n>=2 body as next's), or dispatch from the exported jnf/ynf wrappers (s10_math_gen.rs) straight to
math::special::j0/j1/y0/y1 so special::jn<f32> keeps next's code. Measure all nine rows each try.

## n >= 2 for |x| >= 2^19 (asked by the brief; not implemented)
Today these take fast_bessel_n's forward recurrence from double-double J0/J1/Y0/Y1 (x up to 2^500), not the
accurate path. The Hankel form of D-10.109 (DLMF 10.17.3, 10.18.17-18, μ = 4n²) holds for any n, but its truncation
must be re-bounded: the first dropped phase term is about (μ-1)(μ²-114μ+1073)/(5120x⁵), ~n⁶·2^-101.3 at x = 2^19,
below the band's 2^-90 only for n <= 3; the dropped modulus term ~μ²/x⁴ is fine further. So: n <= 3 shares the
same truncation from 2^19; larger n needs either a threshold x >= c·n^(6/5)·2^19-scale or more terms of P and Q.
The fixed-point phase works unchanged: (2n+1)/2 is a half integer (only n mod 4 enters the quadrant) and δ' gains a
factor ~μ, still added at 2^-84 for μ << 2^40. Refused operands: a per-order exception table needs an exhaustive
enumeration per n (feasible for a few small n only); for general n fall back to the existing recurrence or the
accurate path (rare, ~50 per function per 2^30). Measure bes_jnf3_large first: the recurrence may already be close.

## Jobs (mei)
Final head 3d5b50c89: libm-jnf-large-lint4 rc 0 (xtask clippy, all-features clippy, static), libm-jnf-large-percall4
rc 0 (table above), libm-jnf-large-percall4-sse2, -gate4, -plants4 (handoff/LIBM-JNF-LARGE-plants.txt, 6 plants),
-sweep4 (full exhaustive) were running at hand-back: read their rows in ~/logs/jobs.tsv.
Superseded: -lint1 (rc 1, long-sweeps runner row missing; fixed c4531bec3), -gate2/-size1/-plants1 never ran or
killed (older heads), -percall2 (v1), -lint3/-percall3 (v2), -gate3/-size3/-percall3-sse2 killed (old head).
Driver: mei:~/work/LIBM-JNF-LARGE/run.sh <k>.
