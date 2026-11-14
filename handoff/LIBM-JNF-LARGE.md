# LIBM-JNF-LARGE handoff (run 2, LIBM-JNF-LARGE-2: low orders done, typical rows level with next)

State: branch `task/libm-jnf-large-2` (carries run 1's `task/libm-jnf-large` commits), base origin/next 630f3afb2,
product head 833dd6654 (later commits handoff/ only). Role mei:~/w/s3 (Zen 1). Briefs: ~/state/briefs/LIBM-JNF-LARGE.md
(the change), ~/state/briefs/LIBM-JNF-LARGE-2.md. Decisions in force: D-10.109 (large band); pending record
records/pending/LIBM-JNF-LARGE-1.toml (D-NEW, series D-10; rewritten for this form, bare `|x|` fixed: run 1's head
failed `cargo xtask static` on it).

## The change (final form, 9a95f99a0 + 30520a1bc + 833dd6654)
`special::jn` / `special::yn`, `float` only: next's code until the recurrence tier (`fast_bessel_n::jnf/ynf`) refuses;
the refusal tail is ONE out-of-line `#[cold]` call `jn_low(n, x, mode)` / `yn_low` (same arguments as next's
`jn_accurate`/`yn_accurate` tail). Inside: |n| > 1 -> accurate path as before; orders 0, ±1 with an operand the tier
already ran `fast32::eval` on (jnf: normal x; ynf: normal x > 0) -> `bessel32_rest` directly (the large band, then its
table, then the accurate path: what j0f..y1f do after their eval refuses), order −1 at −x (J) or in the mirrored mode
negated (Y); other operands (zeros, subnormals, inf, NaN) -> `j0`/`j1`/`y0`/`y1` themselves as run 1 did.
Why this form: run 1 tested |n| <= 1 ahead of the tier (jnf3_mid +2.0, ynf1_mid +4.1). With the test after the tier
inside `yn`, ynf rows were still +4.0 on every answered call (jn level): the extra branch changed yn's frame. Moving
the test into the out-of-line call made all typical rows identical to next. (wl_trace could not split it: internal
symbols are unnamed in the product and a role directory cannot trace a base revision.)
Tests: `jnf_ynf_low_orders_take_the_large_band` now also asserts the band answered (stats fast count of j0f..y1f grows;
a lost route gives the same values, so values alone could not catch it) and covers ±0, ±1e-40, ±inf (the fallback
arms). Exhaustive `jnf_ynf_low_orders_are_the_order_functions_exhaustively` unchanged (long-sweeps Exhaustive group).
Plants: handoff/LIBM-JNF-LARGE-plants.txt, 13 (routes, signs, mirrored mode, direct-to-band arms, sweep).

## Counts (wl_percall instr/call, Zen 1, base next 630f3afb2; job libm-jnf-large-2-percall3, SSE2 -percall3-sse2)
| row | glibc | next | run1 v3 | 9a95f99a0 | 30520a1bc | 833dd6654 (final) |
|---|---|---|---|---|---|---|
| jnf0_large | 329.2 | 175480.4 | 434.5 | 477.5 | 461.5 | 459.5 |
| jnf1_large | 326.7 | 178368.1 | 437.0 | 485.0 | 468.0 | 466.0 |
| jnfm1_large | 329.8 | 178370.1 | 441.0 | 491.0 | 472.0 | 470.0 |
| ynf0_large | 341.5 | 175474.4 | 436.0 | 497.0 | 485.0 | 479.0 |
| ynf1_large | 383.2 | 178367.1 | 447.5 | 512.5 | 492.5 | 486.5 |
| jnf0_mid | 319.5 | 268.3 | 262.3 | 268.3 | 268.3 | 268.3 |
| ynf1_mid | 367.3 | 291.6 | 295.7 | 295.6 | 295.6 | 291.6 |
| jnf3_mid | 693.0 | 2747.1 | 2749.1 | 2747.1 | 2747.1 | 2747.1 |
| ynf3_mid | 701.8 | 2411.6 | 2396.6 | 2415.6 | 2415.6 | 2411.6 |
| jnf3_large (new row) | 679.3 | 35853.2 | - | 35854.2 | 35854.2 | 35854.4 |
| ynf3_large (new row) | 710.8 | 35716.8 | - | 35721.8 | 35721.8 | 35719.8 |
j0f_large 405.5 / j0f_mid 233.3 unchanged. SSE2 (GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA, env key in the log header):
Liella columns identical (Liella does not read that tunable for these paths), glibc ~20-45 higher.
Cost accepted: jnf3_large +1.2, ynf3_large +3.0 (0.003% / 0.008%): NOT VERIFIED where; likely the n = 3 operands the
tier refuses (they cost ~1.78e5 each on the accurate path) now pass through the cold call first. Putting the test
back in `yn` would cost every answered ynf call 4 instructions instead. The large low-order rows are ~25 above run 1's
v3 (the tier's own refusal runs first); the typical rows cannot be level otherwise.
Note: run 1's SSE2 job ran 1 s (NOT VERIFIED whether it reused cached default rows); this run's -sse2 jobs print
their own env key (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`) and differ from the default in the glibc column.

## n >= 2 at |x| >= 2^19 (NOT implemented; the brief scoped n <= 3 to this run, it did not fit the context budget)
Measured: jnf(3)/ynf(3) on the band cost ~35,850 instr/call (glibc ~690, 52x: MAGNITUDE trigger territory). Analysis
for the Hankel band at n = 2, 3 (DLMF 10.18.17-18, μ = 4n²):
- Phase truncation: first dropped term (μ−1)(μ²−114μ+1073)/(5120x⁵): n = 2: 7425/5120 -> 2^-94.5 rad at 2^19; n = 3:
  60725/5120 -> 2^-91.4 rad = 2^-92.1 quadrants. Both inside LG_GABS (3e-25 ≈ 2^-81.5, of which 2^-84 is δ''s cut).
  n = 4: 2^-90.3 rad: needs x >= ~2^19.4 or the x^-5 term; adding that term for every n moves the truncation to ~n^8/x^7.
- Modulus: P²+Q² = 1 + (μ−1)/(8x²) + O(μ²x^-4): W = (μ−1)/8 (15/8, 35/8); dropped part <= ~2^-66 relative at n = 3,
  far under LG_B (8·2^-53).
- Coefficients: D1 = (μ−1)/(4π): 15/(4π), 35/(4π); D3 = (μ−1)(μ−25)/(192π): −135/(192π), 385/(192π).
- BLOCKER found: `dfix = ((dq·2^84) as i64) << 42` needs |dq| < 2^-21; at 2^19 |dq| reaches ~2^-18.7 (n = 2) and
  ~2^-17.5 (n = 3). Needs a two-part conversion (hi = (dq·2^52) as i64 << 74, lo from the exact remainder) or a
  coarser scale with a larger GABS. The 5·U53·|dq| term of the sin branch's bound grows to ~2^-68 (only near zeros).
- Quadrant: shift (n + [Y]) mod 4 in place of `nu + (f >> 1)`; J_n(−x) = (−1)^n J_n(x); refused operands (no table)
  fall back to today's recurrence, then the accurate path.
- Routing at zero cost for mid rows: fold the band test into the tier's existing `!x.is_normal()` test (one unsigned
  compare: a − 0x0080_0000 < 0x4880_0000 is normal-below-band; else a cold branch). Measure all rows.

## Jobs (mei), final head 833dd6654
libm-jnf-large-2-lint3 rc 0 (xtask clippy, all-features clippy, static, the two band tests), -percall3 rc 0,
-percall3-sse2, -gate5, -plants5, -sweep5 (see the hand-back for their state).
Superseded: -lint0 (rc 1: inherited record's bare `|`), -lint1/-percall1 (9a95f99a0), -lint2/-percall2 (30520a1bc),
-trace1-new/-trace1-base (diagnostic; base rc 2: a role directory cannot trace a base rev).
Run 1's -sweep4 had no jobs.tsv row: its log stops while the test runs (the role was released with it running).
Driver scripts: mei:~/work/LIBM-JNF-LARGE-2/.

## Next step
1. If any -gate5/-plants5/-sweep5 is red, read why first. 2. A follow-up story for n = 2, 3 on the band (above), then
n > 3 with the x^-5 phase term.
