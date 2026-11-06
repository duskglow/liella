# TIMING-N100-MEMCMP handoff (INCOMPLETE: the branch must not land as it stands)

State: branch `task/timing-n100-memcmp` from origin/next e119b8bc; code commit e53ba22d (fold.S + small_compare.rs +
plants file), handoff commit on top. Role keke:~/w/n4 (Zen 1). Timing: N100 host nozomi (booking 82, cancelled),
Ivy Bridge host kotori (booking 83, ran out at 12:40 while the second IVB script was still running; host load 1.26 at its end).

## Change (e53ba22d)
* `.Lcmp_long` (AVX2, n > 256, also 2049+ through `.Lcmp_avx_long`): 256-byte steps (eight blocks ANDed, one vptest)
  from byte 32, a negative index counting up to 0 from the bases a + n - 256 / b + n - 256, ONE branch per step
  (`sbb` of the equal flag ANDed with the advanced index, `js`). After it, at most 256 bytes are left: a 128-byte chunk
  at the cursor if more than 128 remain, then the last chunk at n - 128 as before. A differing step's first half goes
  to `.Lcmp_long_diff`; the second half's masks are moved into ymm1-4 (`.Lcmp_long_wide_diff`).
* `.Lcmp_256`: loads and compares interleaved (same instructions). Measured NEUTRAL (below): revert it in the continuation.
* small_compare.rs: `compares_match_a_byte_loop_in_the_long_classes_in_each_build` (sizes 289-4097, three alignment
  pairs, first difference at offsets 0/17/31 of every 32-byte block plus the edge positions, every build).

## Results
* Mispredict hypothesis CONFIRMED (N100, perf stat -r 5, Liella only, per call = (row - setup-only run) / 84000):
  memcmp_4k branch-misses 1.00 -> 0.002 per call; cycles 279.7 -> 261.2 per call (glibc 258 in the brief's profile).
  memcmp_256 18.18 -> 18.18 cycles (the interleave does nothing).
* N100 time (`--compare --runs 5`, two rounds before/after, both rounds identical), Liella ns / glibc ns:
  memcmp_4k 116.52 -> 108.97 (ratio 1.09 -> 1.02); memcmp_256 7.53 -> 7.53 (1.06); memcmp_64k 1977.6 -> 2096.0
  (0.83 -> 0.88): **64k SLOWER by 6%**, fewer instructions. Not explained yet (64k is an L2-resident stream; 16 loads
  per step instead of 8 is the obvious suspect).
* Ivy Bridge time (SSE2 class; same staged trees): memcmp_4k 141.41 -> 144.75 (0.98 -> 1.00), both rounds: **SLOWER by 2.3%**
  although no SSE2 instruction changed. Suspect: the SSE2 classes after `.Lcmp_long` moved (code alignment of the
  `.Lcmp_sse_long` loop). NOT VERIFIED. 144/256/64k unchanged.
* Instructions (keke, `bench --compare --counters cbench_memref`): memcmp_4k 554.0 -> 500.1, memcmp_64k 8245.4 -> 7233.0;
  every other row within +-0.3 (memcmp_256 52.4 -> 52.5 with identical static code; untouched memchr/memcpy rows
  moved the same amount: harness noise, not a gain).

## Jobs
keke: timing-n100-memcmp-count-before (rc 0, tree e119b8bc, also staged before.tar), timing-n100-memcmp-count-after
(rc 0, e53ba22d, after.tar), timing-n100-memcmp-plants (rc 0: 5 plants, 4 CAUGHT, 1 WARN-NO-MSG: step-loop-sign died by
SIGSEGV, the mutant loop reading past the region, which the msg regex did not name), timing-n100-memcmp-gate (clippy,
static, direct-tier affected; still running at hand-back, partial lines all ok).
nozomi: timing-n100-memcmp-n100-2 (rc 0). kotori: timing-n100-memcmp-ivb-2 (rc 0).
Superseded: nozomi:timing-n100-memcmp-n100 and kotori:timing-n100-memcmp-ivb (the script passed `--rows`, which
`--compare` refuses; every compare failed; the perf stat lines of the first are kept, the same as the second's).
Staged trees and scripts stay in ~/work/TIMING-N100-MEMCMP/ on nozomi and kotori (before/, after/, run2.sh).

## Next step (continuation)
1. Revert the `.Lcmp_256` interleave (neutral). The 256 row still needs its own idea (N100 1.06, IVB 1.26 is
   TIMING-IVB-MEMREF-ENTRY's).
2. Keep the 256-byte one-branch step only up to a bound (try n <= 8 KiB; 4k keeps its win) and the old 128-byte
   loop above it, OR find why 64k slows (try a 128-byte one-branch step: same branch count per byte as now, 8 loads per step).
3. Restore the SSE2 path's IVB time: `.p2align` the `.Lcmp_sse_long` loop or place the new AVX2 code after the SSE2
   classes; check with the same staged before/after pair on Ivy Bridge.
4. Re-time on both classes (stage with `cargo xtask bench --stage <tar> --only cbench_memref`; `--compare` takes the
   bench name positionally, not `--rows`), then counts, gate, plants.
Plan rows: none new (no D-row written; the change is local to fold.S).
