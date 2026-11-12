# LIBM-RETHINK-HYPOT(-2): f64 hypot, one correctly rounded path for every finite input

State: branch `task/libm-rethink-hypot-2` (stacks on `task/libm-rethink-hypot`, from origin/next c32adc349),
head 1410fd5d6 (code) + this note. -2 stopped at its context checkpoint (RETHINK: 180k) after implementing
and proving the kernel; the gate was started, plants, counts and the record are not done.

## Done (-2)

* Step 0: clippy + static on the inherited head green (shiki job `libm-rethink-hypot-2-lint0`).
* Kernel per the continuation brief `handoff/LIBM-RETHINK-HYPOT/LIBM-RETHINK-HYPOT-2.md` (D-10.104), in
  `crates/math/src/invtrig.rs`: `hypot_root` (both builds, Fast2Sum since a^2 >= b^2), the fast entry
  `hypot_nearest` (both builds; zero operand, gap >= 27 -> `a·(1+2^-60)`, refuses NaN/inf, subnormal b in the
  gap window, top binade, and roots within 2^-98 of a double or midpoint), and the plain `hypot_rounded`
  (every direction, explicit flags/errno: both-subnormal integer root with after-rounding tininess, gap exit,
  Ziv test with factor 1+2^-40, exact wrapping-u128 `sq_cmp` at yh or the midpoint, ties to even, overflow
  inf+ERANGE (nearest/up) or DBL_MAX). `pow::hypot::<f64>` routes to it (entry slow path, cabs, cpow; #4947);
  the 256-bit path is `pow::hypot_exact` (other formats, test oracle). Boundary unchanged.
* Deviation from the design: `liella_math` cannot call SSE2 intrinsics without `unsafe` (E0133), so the slow
  kernel takes `sqrt_soft` (inverse-root Newton, within 2^-50; the `sqrt_dd` bound with that first root stays
  below 2^-99.5 < 2^-98, comment at `sqrt_soft`). The fast entry keeps the boundary's inline `sqrtsd`.
* ERANGE follows `float::range_errno` (only for an infinite result), not the design note's "DBL_MAX + ERANGE".
* Test `double_hypot_kernel_is_the_exact_path_in_every_direction` (crates/math/tests/invtrig.rs): kernel vs
  `hypot_exact`, 4 directions, value+flags+errno, ~9.5k pairs x 2 orders (bit patterns, near-binade, gaps
  25-29, ties and exact roots from triples at 5 scales, DBL_MAX overflow, two subnormals at the 2^-1022 edge,
  subnormal beside normal), fast entry and exported entry in both builds. Green with the existing fast-tier
  test (job `libm-rethink-hypot-2-t4`, rc 0, static clean). Earlier t1-t3 were compile/lint fixes.

## Next step (-3), in order

1. Read the gate `libm-rethink-hypot-2-gate` (shiki, head 1410fd5d6); fix what it finds.
2. Plants via `cargo xtask plants` on invtrig.rs: `HYPOT_GAP` 27 -> 26; midpoint `2 * ym + 1` -> `2 * ym`;
   `EPS_HYPOT * r.hi` halved/doubled in `hypot_rounded`; tie pick `yb & 1 == 0` -> `!= 0`; tiny nearest edge
   `1 << 51` -> `1 << 50`; ZIV `pow2_bits(-40)` -> `pow2_bits(-60)`. `checked_shl` -> `wrapping_shl` is likely
   EQUIVALENT (shifts >= 128 occur only in pairs that cancel: m = a), say so in the plants file.
3. `wl_percall.sh --spec validation/percall/hypot.c` after (before: typ 110.0 / near 2306.9 / bits 3703.4 vs
   glibc 65.9 / 68.8 / 41.5); add a directed-mode row if cheap. Target: near and bits within 2x glibc.
4. `cargo xtask records new decision D-10.104`, citing Borges 2021 and Ziv 1991 (body: the design above).

Do not redo: the design, the -1 baseline/trace, the kernel and its test. Plan rows: D-10.13, D-10.22, D-10.25.
Report: handoff/reports/LIBM-RETHINK-HYPOT-2.json.
