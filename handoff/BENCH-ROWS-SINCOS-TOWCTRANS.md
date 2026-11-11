# BENCH-ROWS-SINCOS-TOWCTRANS: cbench rows for sincos, sincosf, sincosl and towctrans

State: branch `task/bench-rows-sincos-towctrans`, code commit 293336cdf (head: the handoff commit on top). Built and
tested on mei, role s4 (Zen 1 host class; the role tree is on zram).

## What landed

* `bench/c/cbench_libm.c`: rows `sincosf`, `sincos` and `sincosl` with the sin/cos operands ([-10, 10], the same
  xorshift fill), called through volatile function pointers (macro `RSC`). Before a row is timed it checks once, on its
  own 256 operands, that sincos gives exactly the bits of sin and cos from the same library (memcmp of 4/8/10 bytes).
  A mismatch prints `cbench_libm: sincos identity check failed: row <row> operand <i>` and exits 1. Each row prints
  `cbench_libm: check <row> digest 0x...` (FNV-1a of the results) on stderr, so the two builds can be compared.
* `bench/c/cbench_locale.c`: rows `towctrans_toupper_utf8` (operands 0xE0..0xFF, like `towupper_utf8`) and
  `towctrans_tolower_utf8` (0xC0..0xDF), in C.UTF-8, through a volatile pointer. Before they are timed (only when one
  of them is selected), towctrans with the "toupper" and "tolower" descriptors is checked against towupper/towlower
  for every code point 0..0x10FFFF and for WEOF. A failure prints `cbench_locale: towctrans identity check failed`
  and exits 1. The digest line is `cbench_locale: check towctrans_utf8 digest 0x...`.
* `bench/budgets.tsv`: ratio_host rows for the five new rows (libm 1.0, locale 1.10). They are informational, with
  no host_limits entry yet: the next timing sweep calibrates them.
* The plants file is committed as `handoff/BENCH-ROWS-SINCOS-TOWCTRANS-plants.txt`.

No decision record: no rule changed.

## Probe-settled behaviour (black box, glibc 2.39 on the build host)

* sincosf/sincos/sincosl give exactly the bits of sinf/cosf, sin/cos and sinl/cosl. There was no mismatch in 100000
  operands per range over [-10, 10], [1e6, 1e300] and [-1e-3, 1e-3]. That is why the identity check is exact.
* towctrans(c, wctrans("toupper"/"tolower")) equals towupper/towlower for every code point, in both C and C.UTF-8.

## Liella vs glibc output on the same inputs

* towctrans: the digests are identical (every code point and WEOF, both descriptors).
* sincos: the digests differ. A probe (scratch only, not in the tree) linked the Liella products and glibc on the
  bench's 256 operands. All differences are 1 ulp: sincos differs in 1 of 512 results, sincosf in 5 of 512 and
  sincosl in 18 of 512. Liella is also bit-identical to its own sin/cos, because the identity check passes on the
  Liella build.

## One run on next's products (information only; not booked, Zen 1 host under normal load)

`cargo xtask bench --rows ...` (job bench-rows-sincos-towctrans-rows2). The products are the same as origin/next's,
because this branch changes only bench files. Median Liella/glibc ratio over 3 rounds:

| row | Liella ns | ratio | reference rows (same run) |
|---|---|---|---|
| sincosf | 31.13 | 4.841 | sinf 2.316, cosf 2.272 (10.5 ns each) |
| sincos | 25.68 | 1.592 | sin 1.772, cos 1.560 |
| sincosl | 135.45 | 0.763 | sinl 0.519, cosl 0.518 |
| towctrans_toupper_utf8 | 6.81 | 2.076 | towupper_utf8 1.600 |
| towctrans_tolower_utf8 | 7.10 | 2.432 | |

INFORMATIONAL: Liella's sincosf (31 ns) costs more than sinf + cosf called separately (about 21 ns), and its ratio is
4.8x glibc. This is a candidate libm story (one shared range reduction for both results). Wall time on a loaded
host only; the timing sweep gives the verdict.

## Jobs (mei)

* bench-rows-sincos-towctrans-clippy: `cargo xtask clippy && cargo xtask static`, rc 0.
* bench-rows-sincos-towctrans-gate: `cargo xtask affected --run --base origin/next`: 2 suites, 341 passed, 0
  failed; peak 1.3G.
* bench-rows-sincos-towctrans-plants: 5 plants (wrong function in each sincos row, towupper in place of towctrans,
  the tolower row given the toupper descriptor), each with `msg:` naming the identity-check failure.
* bench-rows-sincos-towctrans-rows2: the one-off run above. Superseded: bench-rows-sincos-towctrans-rows1 failed to
  compile (a comma inside a macro argument), fixed before the commit.

## Next step

None needed for this story. Optional follow-ups: a libm story for sincosf (above), and host_limits calibration of
the five rows after the next timing sweep.
