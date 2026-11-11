# LIBM-INVALID-CONVERT-SCAN (-2 .. -5): handoff

State: branch task/libm-invalid-convert-scan-5 (stacked on -4), origin/next 28a903a01 merged (commit 37dd4e4df), head =
last commit on the branch. Decision in force: D-10.95 (records/decisions/D-10/D-10.95.toml). Full -4 report:
handoff/libm-invalid-convert-scan-4-report.md (lgammaf(-2.5) settled: Liella correctly rounded, pinned; sign from Gamma(x)).

## -5 (merge only)
- Merge: quad.rs conflicted on its `use` line only: kept next's `pub(crate) mod arc;` (binary128 arc kernels,
  wide_int/quad/arc.rs) and this branch's `big_u64` import. arc.rs merged cleanly. One fix per site: the new kernels
  reach the reciprocal seed only through div3 (already big_u64).
- float-casts found one new cast from next: quad/arc.rs atan_le `h ... as usize`. Range stated (tops in [2^52, 2^53),
  y <= x, h in [128, 512)); comment only (04bbff653). Now 15 casts, all ranged.
- Plants file: gamma-sign-flip's `lit:` held ` | ` (field separator) and xtask plants REFUSED the whole file in -4
  (mei h2-plants rc 3). Rewritten as `perl:` with `\|` and a lookbehind (5deb9a396).

## Jobs on shiki, head 04bbff653 (role sh1)
| label | result |
|---|---|
| libm-invalid-convert-scan-5-h2-clippy, -h2-static, -h2-flags | rc 0 (flags: math_kernel_no_invalid 2 passed) |
| -h2-plants | 6 of 8 CAUGHT with their messages when last read; gamma-sign-flip, lgammaf-oracle-ulp were still running |
| -h2-gate | affected --base origin/next: running at hand-back |
| -h2-accuracy2 | queued after the gate: s10_math_clauses, gamma lib, and the three ignored arc/quad/quad-arc tier sweeps (exact names) |
| -h2-counts2 | wl_percall lgammaf.c vs base 28a903a01 (primed on shiki this run): running at hand-back |
Superseded: -h2-accuracy (rc 101: the filter also matched wide_dd's long sweep, not touched here; the s10, gamma and
all four wide_int sweeps passed), -h2-counts (rc 2: base not primed), -h2-hottext (rc 101 on stale products: the
chain ran no `cargo xtask so` before it), the first chain (static red on the cast above) and -plants (stopped, stale head).
-4's counts on its pre-merge head (mei): lgammaf_pos 315.8 -> 314.6, neg 8411.0 -> 8398.1, r_neg 8419.0 -> 8406.1.

## Next step
Read -h2-plants, -h2-gate, -h2-accuracy2, -h2-counts2 (lgammaf_pos must be <= base). Green: COMPLETE. hot_text_products
on built products is in the gate if affected; else run `cargo xtask so` then the test.

## INFORMATIONAL
- wide_dd's ignored tier sweep fails on this tree: Expm1 operand sig 11193749041240441045 exp -71, error 2^-100 against
  the bound 2^-100 (wide_dd.rs:745). wide_dd.rs is not changed by this branch.
- lgammaf on negative operands is 21x the reference by instructions: LIBM-F32-REST-NEXT's.
