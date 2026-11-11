# handoff: libm-ld-arc-speed (LIBM-LD-ARC-SPEED; follows libm-ld-arc-chain.md)

**State.** Branch `task/libm-ld-arc-speed` (cut from origin/next a43eb7c3c), role mei:~/w/s2. Code commit fb5721fb6
(asinl/acosl: the root seed's reciprocal beside the root); plants file fix 4ebd8badb; this handoff on top. No rule
changed: D-10.75 keeps its 2^-94 bound (the proof's inner figures moved, below), so the reserved D-10.86 is not used.
Decisions in force: D-10.52, D-10.56, D-10.19, D-10.73, D-10.75.

**Profile (mei:libm-ld-arc-speed-trace0, wl_trace one asinl call, base a43eb7c3c, 737 instructions).** atan_le 329,
root_one_minus_sq 129, finish<asin> (val, inv_top, round) 104, mathret::elem 58, asin_sqrt 54, mathret::exit 31,
asinl/bits wrappers 23, mxcsr/x87 cw get/set 9. Cycles (Skylake-SP build host, --counters, base): asinl 662 cycles for
683 instructions (IPC ~1.0), acosl 675 / 710, against atan2l 424 / 739 (IPC 1.7) and atanl 350 / 566. asinl/acosl run the
same kernel as atan2l plus the root, yet cost ~240 cycles more: the root alone (sqrtsd, divsd, Newton) explains
~50. The rest is NOT EXPLAINED (the trace's `--list` lines stop after the symbol name in the job log, so no
instruction text; a per-phase rdtscp build was not done).

**Change.** Step 2's Newton seed V = 2^127/s0 was a divsd of the seed, after sqrtsd; now V = 2^90/r = r·(2^90/w'),
whose divsd needs only w' and runs beside sqrtsd (one serial divider op less, two conversions fewer). Proof (arc.rs):
V within 2^-49.6 (r²/w' = (1+e)², |e| <= 2^-52, two roundings), root within 2^-100.3 (was 2^-101.4), index estimate
within 2^-48.7, asin/acos within 2^-99.4 (was 2^-100.1), all under the 2^-94 the rounding test assumes; unit test
bound 2^-100.3. **Tried and dropped:** branch-free sign selects (masks) for the Newton/quotient residuals and the
kernel's `num < 0`: +8 to +13 instructions on atanl/atan2l/tanl, atan2l +11 cycles, no gain (mispredicts are not
the gap).

**Counts** (mei:libm-ld-arc-speed-counts, wl_percall libm_ld_arc.c, glibc / base / new): asinl 109.3 / 763.0 / 738.0;
asinl_near1 125.7 / 772.7 / 746.6; acosl 49.0 / 789.8 / 765.4; acosl_near1 49.0 / 763.7 / 738.6; atan2l, atanl
(622.7), atanl_big (487.4), tanl, tanl_1e300 unchanged. No row slower.

**Timing** (time ratio Liella/glibc, three runs, `~/work/LIBM-LD-ARC-SPEED/run_timing.sh` under `~/bin/timing`,
stage mei:libm-ld-arc-speed-stage commit fb5721fb6; bookings 165/166 cancelled; load <= 0.15):

| row | IVB kotori:libm-ld-arc-speed-ivb2 | before (arc-chain) | N100 nozomi:libm-ld-arc-speed-n100-2 | before |
|---|---|---|---|---|
| asinl | 6.52 / 6.46 / 6.54 | 6.69 / 6.65 / 6.74 | 4.82 / 4.82 / 4.82 | 4.92 / 4.92 / 4.94 |
| acosl | 3.10 / 3.10 / 3.10 | 3.27 / 3.27 / 3.28 | 2.74 / 2.75 / 2.75 | 2.83 / 2.84 / 2.84 |
| atan2l | 2.74 / 2.74 / 2.74 | 2.73 | 2.46 / 2.47 / 2.46 | 2.45-2.49 |
| atanl | 2.37 / 2.34 / 2.37 | 2.35 | 2.25 / 2.24 / 2.24 | 2.24 |
| tanl | 0.81 / 0.81 / 0.81 | 0.81 | 0.91 / 0.92 / 0.92 | 0.92 |

The "before" columns are arc-chain's runs (same hosts, other session). **Targets NOT reached:** asinl/acosl <= 2.5x
(now 6.5x / 3.1x IVB); atanl <= 607 / atanl_big <= 480 (no atanl change: atan_small was already inlined by
LIBM-LD-ARC-CHAIN, so the brief's "outlined atan_small" no longer holds on next).

**Accuracy** (mei:libm-ld-arc-speed-accuracy, 10^7 x 4 modes, release): passed, 0 refused; undecided atan 0, asin 4,
acos 12, atan2 0, tan 0; worst 2^-103.38 / 2^-103.13 / 2^-103.20 / 2^-103.40 / 2^-96.85. **Gate**
mei:libm-ld-arc-speed-gate (`affected --base origin/next --run`): 184 passed, 0 failed, 31 ignored, peak 1.2G.
**Plants** mei:libm-ld-arc-speed-plants2: 2/2 CAUGHT, no WARN (first run -plants: root-iw-noroot did not compile,
`powi` is not in core; fixed). clippy and static clean (mei:libm-ld-arc-speed-lint).

**Next step.** Find the ~200 unexplained cycles of asinl/acosl over atan2l before any more tweaks: a build with
rdtscp around the phases (entry, root, kernel, round, exit) on a booked host, or wl_trace with the instruction text
(check why `--list` lines lose it in the job log). Suspects to rule out: the mxcsr/x87 boundary when SSE ops set flags
(ldmxcsr in exit), store forwarding of the F80 result, the sqrt dependency through `s0`'s f64->u64 conversion.
atanl: `wide::atan`'s decode/nan_in/cubic_tiny (~82) and `div` in `atan_le_in`. A rut is near for asinl (three
stories, each < 20%): if the phase split shows no single cause, the next story should be a RETHINK of the asin/acos
method (for example a direct asin polynomial on a table, no root before the kernel).

**Do not redo.** The masks (above), the trace and counts above (heads a43eb7c3c, fb5721fb6).
