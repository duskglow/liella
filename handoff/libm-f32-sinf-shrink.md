# LIBM-F32-SINF-SHRINK: handoff (sinf, cosf, tanf fast path)

State: branch `task/libm-f32-sinf-shrink` (stacks on LIBM-F32-DOUBLE-KERNELS-2), code head `6347789c`; this note
and `handoff/reports/LIBM-F32-SINF-SHRINK.json` on top. Role keke:~/w/n2 (Zen 1). Verdict INCOMPLETE: one
measured row got slower (below); do not land before that is fixed or ruled on.

## Done (3665b7ff, 6347789c)
* Kernel (`crates/math/src/fast32.rs`, `trig_core`): same single π/64 Cody–Waite reduction and table; the
  polynomials are now Remez minimax (D-10.3): sin odd 1..5, r fixed at 1 (`fits/fast32_sin.fit`, 2^-49.07 =
  15.24 double ulp), cos even 0..6, 1 fixed (`fits/fast32_cos.fit`, 2^-64.59). They replace degree-7/8 Taylor.
  Proof updated: sin/cos ≤ 24.3 ulp (TRIG_BOUND 24.4, TRIG_DELTA 40), tan ≤ 49.7 (TAN_BOUND 50, TAN_DELTA 80).
  Large arguments (|x| ≥ 2^19) still take the accurate path.
* In-place nearest entries `sin_nearest`/`cos_nearest`/`tan_nearest` (eval_nearest arms; inexact-only flags
  argument in the comment). `s10_math.tsv` f rows now use `mret::elem32_fast` like expf; `cargo xtask math-gen`
  regenerated `s10_math_gen.rs`.
* Tests: bound test on the new bounds plus margin check; `trig_nearest_entries_match_the_accurate_path`
  (new); the exhaustive sweep is split into `sinf_/cosf_/tanf_fast_path_agrees_exhaustively` (one job each).
  `cargo test --release -p liella-math --lib fast32`: 14 passed; workspace clippy -D warnings clean; static clean.

## Profile / counts (keke, cbench_libm, instructions per call, Liella / glibc)
| row | before (`-before`, base kernel) | after (`-after`, 6347789c) |
|---|---|---|
| sinf | 167.2 / 42.0 | 90.2 / 42.0 |
| cosf | 167.3 / 42.1 | 90.2 / 42.1 |
| tanf | 173.3 / 106.3 | 98.2 / 106.3 |
| sinf_upward | 246.6 / 42.0 | **258.6** / 42.0 (slower) |
168 other rows: none slower (1% + 0.5 tolerance). Split: the envelope (`mret::elem` vs the in-place entry) held
about 73 of the 125 surplus; the polynomial shrink only 4. No per-phase wl_percall trace was run (budget).
The 1.05x target is NOT met for sinf/cosf (2.15x); tanf is under glibc.
Why sinf_upward got slower: a non-default MXCSR now goes through `elem_fast` → `elem_fast_cold` instead of
`elem` directly (expf has the same shape). Not profiled further.

## Open
* RULING NEEDED: accept sinf_upward +12 (+4.9%) for −77 on sinf/cosf/tanf nearest, or (recommended) a
  continuation that makes the directed-mode route of `elem_fast` cost no more than `elem` (check expf_upward
  too: the same route), then recount.
* Still to reach 1.05x: the in-place kernel is ~90 vs 42. Next levers: the 3-part reduction (6 flops) and the
  two table loads + combine; a per-phase trace (validation/wl_percall.sh) first.
* The baseline report §3.5 the brief names lives only on `origin/task/libm-slow-path-cause`; not read (the
  rules forbid `git show` of others' branches).

## Jobs (keke, on 6347789c)
`libm-f32-sinf-shrink-before`, `-after` (rc 0). Detached at hand-back: `-gate2` (direct tier, --base
origin/next), `-plants2` (`~/work/LIBM-F32-SINF-SHRINK/plants.txt`: bound, coef-s5, red-pi64-3, tan-nearest;
dir /dev/shm/claude/LIBM-F32-SINF-SHRINK/plants-2), `-sweep-sinf`, `-sweep-cosf`, `-sweep-tanf` (from the RAM copy
/dev/shm/claude/LIBM-F32-SINF-SHRINK/sweep). Superseded: `-gate`, `-plants` (stopped by me: `jobrun` runs in the
foreground and my chained start would have been cut by the 10-min task limit; start jobs with setsid).
