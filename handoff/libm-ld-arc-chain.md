# handoff: libm-ld-arc-chain (LIBM-LD-ARC-CHAIN; follows libm-ld-arc-cycles.md)

**State.** Branch `task/libm-ld-arc-chain` (cut from origin/next f1f5358d5), role keke:~/w/n3. Code commits
ba396bda0 (asinl/acosl: tier before the 384-bit value; index scale off the root) and 6148e843c (scale as a closure,
`atan_small` inlined); plants ed9d0427a. No rule changed, so no decision record (D-10.81 not used). Decisions in force:
D-10.52, D-10.56, D-10.19, D-10.73, D-10.75.

**Profile (wl_trace.sh, one call, base f1f5358d5; job keke:libm-ld-arc-chain-trace0).** asinl_near1 1008 instructions:
`mp::pack` 217, `asin_sqrt` entry 112, `finish<asin>` 219, `atan_le` 234, boundary (elem/exit/bits) ~115. acosl 1074:
`mp::pack` 217, entry 117, `finish<acos>` 279, `atan_le` 235. The entries built the operand's 384-bit value and
compared it with 1 BEFORE the tier (which takes only 0 < |a| < 1): a fifth of each call. atanl 621 (`atan_approx`
313, `wide::atan` 82), atanl_big 483 (192 / 82). Cycles per phase were not measured (no per-phase cycle tool; the
trace counts instructions only).

**Changes.** (1) `wide::asin_sqrt` / `acos_sqrt`: NaN, zero, Inf, tiny, then the tier; the 384-bit value only on
the fallback (domain error and +-1 unchanged). (2) The chain: asin/acos decide the numerator from `a` alone (`a < s`
exactly when `a^2 < 1/2`, no tie at 64 bits), and the kernel's index scale `256/x` comes from `1/a` (a `divsd` that
does not wait for the root) or from the root seed's own reciprocal `2^127/s0` times `2^-108` (no `divsd` after the
root's Newton step): serial divider ops after `sqrtsd` 3 -> 2. Proof steps 2, 3, 5 of arc.rs updated (index estimate
within 2^-49; `y/x <= 1 + 2^-100` near `a = 1/sqrt 2`). (3) `atan_small` `#[inline(always)]` (atanl ratchet).

**Counts** (keke:libm-ld-arc-chain-check2, head 6148e843c, instructions per call, glibc / base / new): asinl 109.3 /
1029.0 / 763.0; asinl_near1 125.7 / 1036.5 / 772.7; acosl 49.0 / 1066.0 / 789.8; acosl_near1 49.0 / 1037.6 / 763.7;
atan2l 53.0 / 869.3 / 863.3; atanl 26.0 / 632.1 / 622.7; atanl_big 26.0 / 498.4 / 487.4; tanl 768.7 / 768.7;
tanl_1e300 773.5 / 773.5. No row slower (the first commit alone had atan2l +9.5 from an `Option` scale; fixed by
6148e843c). **Ratchet NOT reached**: atanl 622.7 > 607, atanl_big 487.4 > 480 (16 / 7 left).

**Accuracy** (keke:libm-ld-arc-chain-accuracy, `arc_tier_agrees_with_the_wide_route_long`, 10^7 x 4 modes, release):
passed, 0 refused; undecided atan 0, asin 4, acos 16 (was 8), atan2 0, tan 0; worst 2^-103.38 / 2^-103.41 /
2^-103.88 / 2^-103.38 / 2^-96.85 (same as before). **Gate** keke:libm-ld-arc-chain-gate (`affected --base
origin/next --run`): 543 passed, 0 failed, 242 ignored, peak 1.4G. **Plants** keke:libm-ld-arc-chain-plants
(handoff/LIBM-LD-ARC-CHAIN-plants.txt): 4/4 CAUGHT, no WARN. clippy and static clean (in check2).

**Timing.** Stage keke:libm-ld-arc-chain-stage2 (`cargo xtask bench --stage ~/work/LIBM-LD-ARC-CHAIN/stage.tar`,
commit ed9d0427a, tarball sha256 d1b1956c05e59600...; `-stage` failed rc 1: no tarball argument). Bookings 163
(kotori, IVB) and 164 (nozomi, N100); `~/work/LIBM-LD-ARC-CHAIN/run_timing.sh` under `~/bin/timing` (sweep run
--only cbench_libm x3, Liella and glibc interleaved). Cycle ratios Liella/glibc, three runs:

| row | IVB (kotori:libm-ld-arc-chain-ivb, load <= 1.42) | before (arc-cycles) | N100 (nozomi:libm-ld-arc-chain-n100) | before |
|---|---|---|---|---|
| asinl  | 6.69 / 6.65 / 6.74 | 7.90 | 4.92 / 4.92 / 4.94 | 5.37 |
| acosl  | 3.27 / 3.27 / 3.28 | 3.91 | 2.83 / 2.84 / 2.84 | 3.14 |
| atan2l | 2.73 / 2.73 / 2.72 | 2.80 | 2.45 / 2.46 / 2.49 | 2.50 |
| atanl  | 2.35 / 2.34 / 2.35 | 2.41 | 2.24 / 2.24 / 2.25 | 2.25 |
| tanl   | 0.81 / 0.81 / 0.81 | 0.80 | 0.92 / 0.92 / 0.93 | 0.92 |

N100 load <= 1.09. Bookings 163 and 164 cancelled after the runs. **Target asinl/acosl <= 2.5x NOT reached**
(asinl -15% IVB / -8% N100, acosl -16% / -10%; no row slower).

**Next step.** (a) Read the timing logs; asinl/acosl still ~7x/16x by instructions: the remaining cost is `finish`
(round + the composition, ~220-280) and `atan_le` (~235). Profile both by address (wl_trace --list printed only 16 lines
of the listing here: check that tool) and cut the 128-bit multiply chain in `atan_poly`/`atan_small`. (b) atanl's last 16:
`wide::atan`'s 82 (decode, nan_in, cubic_tiny, then `val` recomputes the normalisation), `div` outlined in
`atan_le_in`. The older head was not disassembled (the rules forbid checking out another story's work; the brief asked
for it: RULING NEEDED in the report).

**Do not redo.** The trace and counts above (heads f1f5358d5 and 6148e843c).
