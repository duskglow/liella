# handoff: libm-f128-trig-int-4 (LIBM-F128-TRIG-INT-4)

**State.** Branch `task/libm-f128-trig-int-4` from origin/next 672acd9aa, role mei:~/w/s2. Code commits 7c68841a8..5609a44f2
(`crates/math/src/wide_int/quad/arc.rs` new, `quad.rs` `div3` pinned inline and test helpers `pub(super)`, `arc.rs`
dispatch and `ATAN_J` `pub(super)`, `validation/percall/libm_f128_trig.c` rows); later commits are the record, plants
file and this note. Decision record D-10.96 (records/decisions/D-10/D-10.96.toml).

**Decisions in force.** D-10.96 (this story: binary128 atan/atan2 three-limb tier), D-10.85/D-10.91/D-10.92 (the
sin/cos/tan tier whose `W3`, `div3`, `round` it shares), D-10.52/D-10.75 (x87 arc tier: method and `ATAN_J`), D-10.19.

## Done: atanf128, atan2f128 on the integer tier
* Method: Tang's table `j/128` with exact `y − cx`, `x + cy` on the 113-bit significands (denominator carry kept as a
  129th bit), `div3` quotient, ten-term odd series in 64/128/192-bit Horner steps, 192-bit table (x87 `ATAN_J` + signed
  third limb `ATAN_J_LO`). Proof in the module doc: within `2^-160.2` relative; the rounding test assumes `2^-152`.
* Counts (wl_percall, instructions:u, glibc / before / after): atanf128 5869.8 / 161186.4 / **1076.0** (0.18),
  atanf128_1 ([-1, 1]) 5853.7 / 157325.9 / **1033.0**, atan2f128 6220.7 / 168731.9 / **1266.9** (0.20). sin/cos/tan/
  sincos rows unchanged (1253 / 1703.5 / 1785.3). asinf128 9271.5 / 176044 and acosf128 9604.8 / 179131 (unchanged,
  baseline for -5).
* Cause found and fixed on the way: `div3` went out of line once it had three callers; tanf128 measured 1721.5 (+18)
  on 7b335d390 (`-percall`), back to 1703.5 with `#[inline(always)]` (`-percall2`).

## Jobs (mei unless noted)
* `libm-f128-trig-int-4-lint3` (head 4f5288287): xtask clippy, static, records check (505 decisions, 0 pending): rc 0.
* `-gate` (5609a44f2, code equal to the final head): `cargo xtask affected --run --base origin/next` under ~/bin/gate:
  2 suites, 208 passed, 0 failed, 33 ignored, peak 1.4G.
* `-sweep2` (5609a44f2): 10^7 operands (half atan, half atan2), 4x10^7 calls in the four modes, 0 undecided, worst
  error 2^-161.36; hard cases 6344 calls, 8 undecided (by design: near a rounding boundary), worst 2^-161.88.
* `-plants3` (handoff/LIBM-F128-TRIG-INT-4-plants.txt): 6 CAUGHT, 0 SURVIVED, 0 WARN (limb dropped, rounding test
  skipped, series term dropped, index floored, denominator carry dropped, x < 0 composed with π/2).
* `-percall2` (5609a44f2): the counts above. `-hottext3` (final head): `cargo xtask so` + hot_text_products, 14 passed.
* `-stage` (final head): `cargo xtask bench --stage`; timing nico `libm-f128-trig-int-4-ivb` (Ivy Bridge), nozomi
  `libm-f128-trig-int-4-n100` (N100): staged cbench_libm `--compare --rows atanf128,atan2f128,tanf128`, glibc /
  Liella ns: Ivy Bridge atanf128 2269.65 / 495.82 (0.22), atan2f128 2639.08 / 581.82 (0.22), tanf128 3023.23 / 684.75
  (0.23, control); N100 atanf128 1292.95 / 286.04 (0.22), atan2f128 1419.94 / 315.01 (0.22), tanf128 1667.46 /
  406.86 (0.24; -3 measured 406.8: unchanged). Before: about 27x and 24x glibc (timing-sweep-1110).
* Superseded: `-t1` (test sign error in the expected value: `mp::atan` takes a magnitude), `-lint` (clippy:
  `unwrap()` in tests), `-plants`/`-sweep` (stopped: tests changed after start), `-percall` (tanf128 +18, fixed),
  `-plants2` (pi-units plant WARN-NO-MSG: caught by the rounded comparison, regex corrected), `-hottext` (record had a
  bare `|`), `-hottext2` (hot_text passed; the chained stage lacked its tarball argument).

## Next: LIBM-F128-TRIG-INT-5 (asinf128, acosf128)
Still on the 384-bit route (about 19x glibc in instructions). `√(1 − a²)` has 192 significant bits, so: (1) `1 − a²`
exact in 256 bits (`(1 − a)(1 + a)` or the full 226-bit square), normalised with an even shift; (2) a three-limb root:
x87 `arc::sqrt` on the top 128 bits (`2^-100.3`) plus one 192-bit Newton correction whose reciprocal needs about
`2^-63` or better (refine the `double` reciprocal once in 128 bits); (3) an `atan_le` variant whose `x` (or `y`) is a
`W3`: `y − cx`, `x + cy` then need three limbs, exact to `2^-170` of `x`. LIBM-LD-ARC-RETHINK-2's lesson (asin needed
a route without the atan2 kernel) was for a 2.5x target on long double; here glibc is ~9300 instructions, so the
atan2 route (estimated ~1500-1700) is well under it: check with a percall before choosing a direct asin kernel.
**Do not redo.** The `div3` inline cause; `mp::atan` takes a magnitude (sign the expected value in tests).
**Lesson.** A shared `#[inline]`-less helper gaining callers can leave other hot paths: recount every row of the
shared spec after adding a caller.
