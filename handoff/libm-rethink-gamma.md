# LIBM-RETHINK-GAMMA: handoff (after run 2, LIBM-RETHINK-GAMMA-2)

State: branch `task/libm-rethink-gamma-2` (run 1's `task/libm-rethink-gamma` plus run 2's commits), head in
handoff/reports/LIBM-RETHINK-GAMMA-2.json. Role keke:~/w/r5. Decision in force: D-10.105 (record written this run,
records/decisions/D-10/D-10.105.toml), on top of D-10.13 (tiers, correct rounding) and D-10.87 (lgammaf edge).

## Run 2: the share-assert failure (found, not a code defect)
The tiny test's failing assert was the **lgamma** share (run 1's "line 64"), not tgamma. The asserts now print the
operands they miss; every miss was about -4.2e293: the test's `spread` helper wrapped `(e + 1023) as u64` into the
sign and exponent for e < -1022, so about 5% of the "tiny" operands were negative integers near -2^974. Fixed: the
helper makes real subnormals. settle was not the cause.

## Run 2: code
* tgamma tiny now includes subnormal x (exact 2^512 scaling, q <= 2^562); `fast64::settle` takes m = 1023 (a rounded
  |v| >= 2 is the overflow in the mode); `settle_wide` takes m >= 1024 as overflow, except |y.hi| = 1 at m = 1024
  (x = ±2^-1024: value just under 2^1024, toward-zero roundings give DBL_MAX without overflow -> accurate path).
* lgamma Stirling band reaches the overflow threshold: from 2^512 the form runs on the scale 2^-512 and is normalized
  (`X_BIG` is gone; X_OVER = 2^1015 stays the no-evaluation overflow).
* special::lgamma/tgamma call the band forms only on their range (the after-probe showed NaN/inf and x <= -16
  paying 17-39 instructions for a call that fell back).
* Probe-settled limit: x = 2^k(1 + j·2^-50), small j, and powers of two below about 2^-100 have 1/x within settle's
  2^-100 slack of a double: directed tgamma there stays on the 384-bit path (correct; isolated inputs).

## Measurements (probe tool, keke, one input per 1/64 binade; instr/call, Liella before -> after, glibc)
| band | before | after | glibc | ratio |
|---|---|---|---|---|
| tgamma 0<\|x\|<2^-60 | 426 160 | 225.5 | 140.0 | 1.6x |
| lgamma 0<\|x\|<2^-60 | 393 275 | 408.0 | 102.1 | 4.0x |
| lgamma x>=16 | 79 395 | 518.1 | 121.6 | 4.3x |
| lgamma / tgamma x<=-16 | 11 524.9 / 11 609.2 | 11 529.9 / 11 612.4 | 127 / 133 | +5 / +3 (cost) |
| lgamma / tgamma typical | 681.5 / 724.9 | 683.5 / 726.9 | 222 / 350 | +2 / +2 (cost) |
| lgamma / tgamma NaN/inf | 110 / 105 | 115 / 108 | 46 / 67 | +5 / +3 (cost) |
Final head, probe `~/work/libm-rethink-gamma/after3` (job probe-after3). Without the range guard the x<=-16 and
NaN rows paid +17..39 (after2); the +2..5 left is the guard's compare plus codegen in special::lgamma/tgamma.
Fix-forward: move the band branch into a `#[cold]` out-of-line helper so the domain path's code is unchanged.

## Jobs (keke)
Final head: gate3 (records check, clippy, static, affected --run --base origin/next: ok, peak 1.4G), plants3
(6 CAUGHT, 1 EQUIVALENT: margin halved; 1 WARN-NO-MSG: settle-1023-refused was caught by the tgamma share assert,
"tgamma tiny: 3239 of 3316", before the edge assert its msg names), probe-after3.
Diagnostics: lint0 (inherited head, rc 0), diag1-5, probe-after (after2, pre-guard).
SUPERSEDED: gate, gate2 (stopped for later commits), plants (pre-guard tree), plants2 (settle m=1023 plant
SURVIVED: no band operand reaches that branch; a unit test now checks it).

## Not done / exact next step (LIBM-RETHINK-GAMMA-3)
1. lgamma tiny (3.9x) and x >= 16 (4.2x) are above the 2x target: profile one call (wl_trace) before designing;
   suspects: log_dd's cost, and special::lgamma's dispatch before the band (typical rows pay ~680 vs glibc 222).
   lgamma x>=16 max 83 118 at 1.2e11: a residue of settle refusals (~0.3%); print the refusal rate per binade.
2. Bands left: f64 x <= -16 for both (reflection Γ(x)Γ(1-x) = π/sin(πx), needs sin(πr) in dd; 11.5k mean, 25% of
   the bit patterns), f64 tgamma 16 <= x < 171.6 (pos_ge16 mean 2 567), and the f32 bands (tgammaf |x| < 2^-20,
   x <= -34 / >= 35, lgammaf x <= -42) with exhaustive sweeps.
## Not to redo
The before measurement and the band traces (~/work/libm-rethink-gamma/before on keke); the design comparison
(D-10.105); the spread-helper diagnosis.
