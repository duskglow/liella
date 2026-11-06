# BENCH-MEMMOVE-OVERLAP-SHORT (handoff, 2026-11-06)

State: branch `task/bench-memmove-overlap-short`, from origin/next 957f58bd. Commits: 5dcf45ef (bench rows),
54db10a8 (D-4.64 + saturation row), then this handoff commit. No product code changed.

## What was done
* `bench/c/cbench_memref.c`: new `overlap_rows()` with `memmove_overlap8_4k` (`memmove(d, d+8, 4096)`) and
  `memmove_overlap64_4k` (`memmove(d, d+64, 4096)`); d = 16 bytes past a 4096-aligned allocation (fixed alignment).
* `bench/budgets.tsv`: both rows as `ratio_host 1.00`, report only (no host_limits / instr_limits row: not calibrated).
* Decision D-4.64 (plan/07-decisions/s04-byte-arrays-and-strings.md); saturation row in docs/saturation/03.

## Measured (`cargo xtask bench --compare --counters cbench_memref`, staged tree of 5dcf45ef; glibc / Liella ns, ratio)
| host (CPU) | memmove_overlap8_4k | memmove_overlap64_4k | job labels |
|---|---|---|---|
| nozomi (N100), booking 90 | 87.92 / 86.80 (0.99); repeat 87.92 / 87.30 (0.99) | 126.04 / 65.59 (0.52); repeat 0.52 | nozomi:bench-memmove-overlap-short-n100, -n100b |
| nico (Ivy Bridge E3-1220L v2), booking 91 | 175.20 / 136.90 (0.78); repeat 0.78 | 125.52 / 119.28 (0.95); repeat 0.95 | nico:bench-memmove-overlap-short-ivb2, -ivb3 |
| mei (Zen 1, build host, not booked) | 52.71 / 52.96 (1.00) | 50.01 / 51.48 (1.03) | mei:bench-memmove-overlap-short-counts |

Instructions: Zen 1 425.7 vs 420.6 (both rows); N100 overlap8 418.8 vs 418.8, overlap64 418.7 vs 41.0 (glibc
takes `rep movsb` at distance 64 there, and is slower in time); Ivy Bridge 709 vs ~27 (glibc `rep movsb`).
The n100 run started while booking 90 was still "settling" (host already idle, load 0.68); n100b repeated it
with the same numbers. Neighbouring rows in the same runs: memmove_4k N100 0.97, Ivy Bridge 1.01.

Verdict: Liella is not slower than the reference on either row on either timing host; the reported ~3% N100
forward-path slowdown did not reproduce on this row shape (0.99). No fix made (brief step 3 not triggered).

## Gate
mei:bench-memmove-overlap-short-gate (clippy, static, affected --base origin/next): 2 suites, 215 passed,
0 failed, peak 1.3G, on 5dcf45ef. Final gate on the handoff head: mei:bench-memmove-overlap-short-gate2.
Plants: none (no product code changed; bench data only).

## Superseded
mei:bench-memmove-overlap-short-stage (rc 1, `--stage` needs a tarball path); nico:bench-memmove-overlap-short-ivb
(rc 1, `--staged` is not a bench option: the staged tree takes `--no-build` only).

## Open / next
* Next step: none needed for this story. If the 3% report came from another shape (other distance, other alignment,
  size), add that shape as a row: the source story's exact call would settle it.
* A host limit for the two rows needs a calibration run (owner's limits), not done here.
