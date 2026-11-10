# LIBM-F64-QUICK-PROFILE (handoff)

**State.** Branch `task/libm-f64-quick-profile`, code head `c6b24f311` (on origin/next 9af6dff86). Decision in force:
D-10.77 (new; amends D-10.51's exp test, keeps D-10.64's guard for exp2/exp10). Plants: `LIBM-F64-QUICK-PROFILE-plants.txt`.

**Changes (crates/math/src/fast64/quick.rs).**
1. `exp_quick`/`exp_tiered`: the rounding test drops `w != 0` (exp has no exact result for a double x != 0; with
   zl = 0 the error <= rho·zh < 2^-61·zh is under a quarter ulp; proof in the module doc and D-10.77).
2. `side` (exp, exp2, exp10, expm1, hyperbolics): `tl·(1 + rh)` -> `ma(tl, rh, tl)`: one FMA in the FMA build
   (an add less); SSE2 keeps two operations. tl <= 2^-53·th, so roundings stay < 2^-104·th: bounds unchanged.

**Counts** (keke Zen 1, `validation/wl_percall.sh --base none --spec <scratch libm_f64_quick.c>`, cbench_libm's
ranges, instructions per call; SSE2 = `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`; glibc / Liella before / after change 1):

| row | FMA glibc | FMA before | FMA after 1 | SSE2 glibc | SSE2 before | SSE2 after 1 |
|---|---:|---:|---:|---:|---:|---:|
| exp | 69.0 | 91.0 | 88.0 | 79.0 | 104.2 | 102.2 |
| log | 65.0 | 92.0 | 92.0 | 80.0 | 119.0 | 119.0 |
| log2 | 65.0 | 110.0 | 110.0 | 85.0 | 143.0 | 143.0 |
| log1p | 85.3 | 112.0 | 112.0 | 104.8 | 144.0 | 144.0 |
| pow | 142.0 | 172.0 | 172.0 | 192.0 | 242.0 | 242.0 |
| sin | 102.1 | 130.7 | 130.7 | 124.1 | 173.0 | 173.0 |

Change 2's count is job `lfqp-count-after2` (log `~/work/LIBM-F64-QUICK-PROFILE/after2/percall-*.log` on keke):
NOT READ by me at hand-back; expected exp FMA -1, SSE2 0.

**Profile of one call** (`validation/wl_trace.sh --list`, call 11, keke; logs in scratch base/). Only the exported
entry is a named symbol; the kernels are inlined or unnamed, and the `--list` output kept only the last ~60
instructions of each call (tool limit, below), so the split is entry vs kernel, not per phase:

| fn | FMA total | entry+envelope | kernel+test | SSE2 total | entry | kernel+test |
|---|---:|---:|---:|---:|---:|---:|
| exp | 82 | 21 | 61 | 94 | (inlined) | 94 |
| log | 83 | 17 | 66 | 110 | 17 | 93 |
| log2 | 101 | 17 | 84 | 134 | 17 | 117 |
| log1p | 103 | 17 | 86 | 135 | 17 | 118 |
| pow | 162 | 18 | 144 | 232 | 18 | 214 |
| sin | 116 | 17 | 99 | 158 | 17 | 141 |

Reading: the entry/envelope (17-21) is now about the size of the whole gap on exp/log (glibc's own entry is in its
total). log2 and log1p carry about 18-20 instructions over log in the same build: their extra reduction is where
the log-family surplus lies, not in log's kernel. pow's SSE2 build costs 70 more than FMA (exact products by Dekker).

**Candidates of DESIGN 4/4.1, checked.**
- One-sided `near` test: already in exp, log, pow; exp's extra `w != 0` guard removed (change 1). sin keeps
  `w != 0 && (sh != 0 || |rh| >= R26)` (exact/tiny cases; not re-proved here).
- Minimax degree 5 for exp: REJECTED on the bound. A degree-3 minimax of (e^r - 1 - r)/r² on |r| <= 2^-8.53
  leaves about R^4/720/8·r² = 2^-63.7 (vs Taylor 6's 2^-72), above the FMA bound 2^-67.7 (fallback ~0.0076% ->
  ~0.1%) to save one FMA. Not worth it.
- Separate `1 + rh` product: done as change 2 (FMA build).
- N100 FP-op count of exp: about 33 before; changes 1 and 2 remove a compare and an add (FMA build).

**Not done / NOT VERIFIED.** Booked timing (Ivy Bridge, N100) not run (budget); the brief's "keep only what booked
timing confirms" is open: both changes only remove instructions (no new instruction kinds). The function's separate
accuracy jobs (four modes) not run; the exp quick tests (400k-operand CR match, hard cases, bounds) run in the gate.
Per-phase split blocked by the trace tool: `wl_trace.sh --list` logs kept only the tail of each call.

**Next step.** Book Ivy Bridge (nico/kotori) and N100 timing for exp/expm1/exp2 at c6b24f311 vs origin/next.
Then profile log2/log1p's reduction (the +18 over log), with a trace tool that keeps the whole `--list`.
Jobs on keke: `lfqp-count-after2`, `lfqp-gate2`, `lfqp-plants2` (final head); `lfqp-prof-base2`, `lfqp-count-after`,
`lfqp-plants` (head 21a894295, superseded).
