# handoff: libm-f128-trig-int-3 (LIBM-F128-TRIG-INT-3)

**State.** Branch `task/libm-f128-trig-int-3`, stacked on LIBM-F128-TRIG-INT-2 (head 0ae8e05fd; it lands with
this branch). Role keke:~/w/r5. Code: `crates/math/src/wide_int/quad.rs` only; decision record D-10.92
(records/decisions/D-10/D-10.92.toml). Final code commit b5adfde0f (code equal to baeaed02c).

**Decisions in force.** D-10.85 (three-limb sin/cos kernel), D-10.91 (tan/sincos from one reduction, `div3`),
D-10.92 (this story: the window written out, the kernel's helpers pinned inline), D-10.19 (correctly rounded).

## Step 0: -2's head re-verified (0ae8e05fd, before any change here)
* Sweep `libm-f128-trig-int-3-s0-sweep` (release, `tier_agrees_with_the_wide_route_long`): 10^7 operands,
  2x10^8 calls in the four modes, 0 undecided, 0 refused, worst 2^-158.26; near multiples 327 680 calls, 131 072
  undecided by design, worst 2^-166.22, closest |f| 2^-124.
* Plants `libm-f128-trig-int-3-s0-plants` (handoff/LIBM-F128-TRIG-INT-2-plants.txt): 6 of 6 caught, no WARN-NO-MSG
  or WARN-RC-ONLY (the tool's WARN-DEV-SHM notice only).
* Percall `libm-f128-trig-int-3-s0-percall`: sinf128/cosf128 1332, tanf128 1749, sincosf128 1831 (as -2 reported).

## Step 1: sinf128/cosf128 back under 1257 (cause found, not guessed)
* Static: `objdump -dr` of `quad::kernel` in ~/wl/products: at 7fcb6acda (before -2) one straight-line function,
  0x15c5 bytes, no calls; at ada683d87 (-2) 0xb6b bytes calling `reduce`, `sin_poly`, `cos_poly`, `mulhi3` (x5).
* Pinning those inline (aa455efac, percall1) measured **1375**, worse. The one-call trace (`wl_trace.sh --list`,
  trace1) then showed the rest: `kernel` calls `core::array::try_from_fn` out of line (the 2/pi window
  `core::array::from_fn(|k| ...)` in `reduce`), about 150 of the call's 1230 instructions.
* Window as an array literal (baeaed02c, percall2): sinf128 **1253**, cosf128 1253, tanf128 1703.5, sincosf128
  1785.3. Without the pins (ee982ff35, percall4): 1325 / 1743 / 1825, so both halves stay (b5adfde0f).
* wl_trace on a base revision needs a git tree (not a role dir or ~/wl/src copy): the base was compared by
  disassembling its products only.

## Final-head jobs (head b5adfde0f; later commits are handoff/report only)
* keke `libm-f128-trig-int-3-lint`: xtask clippy, static, records check (501 decisions, 0 pending): rc 0.
* keke `-hottext`: `cargo xtask so` + `cargo test -p liella --test hot_text_products`: 14 passed. Run on this branch's
  head, not a merge with next (the rules allow no merge; the train runs the merged head).
* keke `-gate`: `cargo xtask affected --run --base origin/next` under ~/bin/gate: 4 suites, 889 passed, 0 failed,
  243 ignored, peak 1.3G.
* keke `-plants` (handoff/LIBM-F128-TRIG-INT-3-plants.txt): 8 CAUGHT, 0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG
  (-2's six plus the window low word dropped and the window words out of order).
* keke `-sweep`: 10^7 operands, 2x10^8 calls in the four modes, 0 undecided, 0 refused, worst 2^-158.26; near
  multiples 327 680 calls, 131 072 undecided by design, worst 2^-166.22 (same as step 0: same arithmetic).
* keke `-percall` (instructions:u per call, glibc / before -2 / -2 / now): sinf128 7251 / 1257 / 1332 / **1253**,
  cosf128 7248 / 1257 / 1332 / **1253**, tanf128 8173 / 89 475 / 1749 / **1703.5**, sincosf128 8756 / 2439 / 1831 /
  **1785.3**; the 1e10/1e300 rows likewise (1247-1252, tan_1e300 1697.2).
* Timing (booked, staged cbench_libm `--compare --rows`, stage `-stage` of b5adfde0f), glibc / Liella ns (-2's
  Liella in brackets): Ivy Bridge (kotori `libm-f128-trig-int-3-ivb`) sinf128 1828.3 / 383.4 (404.6) 0.21,
  cosf128 1810.4 / 376.1 (399.0) 0.21, sincosf128 2237.7 / 492.7 (526.9) 0.22, tanf128 2264.6 / 515.3 (549.4)
  0.23; N100 (nozomi `libm-f128-trig-int-3-n100`) sinf128 1395.5 / 314.5 (326.3) 0.23, cosf128 1385.7 / 309.0
  (322.2) 0.22, sincosf128 1719.2 / 410.3 (419.5) 0.24, tanf128 1669.5 / 406.8 (414.7) 0.24.

## Step 2 not started: LIBM-F128-TRIG-INT-4
atanf128, atan2f128, asinf128, acosf128 on the integer tier (about 27x, 24x, 18.5x, 18.5x glibc on Ivy Bridge,
results/timing-sweep-1110-table.md). Budget went to step 0 and the cause hunt (four percall cycles of ~9 min each).
Start: profile atanf128 per call (add atan rows to validation/percall/libm_f128_trig.c), then take
`wide_int::arc`'s x87 design (Tang's table, D-10.52/D-10.75) to three limbs; `quad::div3` (D-10.91 step 6) is the
quotient, a three-limb Newton root is still needed for asin/acos.

**Do not redo.** The cause hunt above; inline pins without the window fix (1375); window fix without pins (1325).
**Lesson.** `core::array::from_fn` and other generic core helpers are not reliably inlined in hot kernels with
several callers: check `objdump -dr` for `try_from_fn`/`map` calls after adding a caller.
