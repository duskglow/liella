# LIBM-CPLX-POW handoff: cpow/cpowf quick + middle tiers, plants caught, per-call measured (run 5)

State: branch task/libm-cplx-pow-5 (= -4's branch, merged with origin/next 51af84e06 in 9a8c60f52); head in
handoff/reports/LIBM-CPLX-POW-5.json. Role mei:~/w/s2. Decisions in force: D-10.53, D-10.74, D-10.99, D-10.25,
D-10.17; this tier's record is still pending (records/pending/LIBM-CPLX-POW-4-1.toml, D-NEW in series D-10: the
coordinator assigns the id at landing).

## Run 5 results

* The -4 blocker is gone: after merging next (HOTLIST-TRAIN-REGEN landed), `xtask clippy`, `static` and `so` pass,
  and all 12 layout contracts hold (mei `libm-cplx-pow-5-lint-so`). The only merge conflict was the
  xtask/src/longsweeps.rs rows: kept the `cpow_decline_rates` row and took next's rows.
* Plants (handoff/LIBM-CPLX-POW-5-plants.txt, the -4 set unchanged): 6 CAUGHT, 0 SURVIVED, no WARN lines
  (mei `libm-cplx-pow-5-plants`, on d93e26f0f; the code under test is the same at the final head).
* Direct-tier gate `cargo xtask affected --run --base origin/next` on the final code: mei `libm-cplx-pow-5-gate2`.
  (`libm-cplx-pow-5-gate` on d93e26f0f was stopped so the gate ran on the final code instead.)
* Per-call instructions (validation/percall/cplx_pow.c, new; release products; base = next 51af84e06; mei
  `libm-cplx-pow-5-percall`, `libm-cplx-pow-5-percall-nofma` with GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA):

  | row | reference | next | this branch | new/ref | ref, -FMA | new/ref, -FMA |
  |---|---|---|---|---|---|---|
  | cpow (z, w in [-3,3]^2) | 719.6 | 351721.5 | 1518.9 | 2.11 | 811.3 | 1.87 |
  | cpow, abs(w log z) 2^-6..1 | 692.1 | 334762.0 | 1442.5 | 2.08 | 776.1 | 1.86 |
  | cpow, abs(w log z) 1..16 | 719.5 | 352324.0 | 1679.2 | 2.33 | 811.1 | 2.07 |
  | cpow, abs(w log z) 16..512 | 730.5 | 353490.2 | 2820.5 | 3.86 | 824.1 | 3.42 |
  | cpowf (z, w in [-3,3]^2) | 470.4 | 352300.5 | 1567.8 | 3.33 | 492.4 | 3.18 |
  | cpowf, abs(w log z) 4..64 | 473.5 | 352631.7 | 1567.7 | 3.31 | 496.1 | 3.16 |

  sd 0 for every row. Liella's counts are the same with -FMA (these paths take no FMA dispatch); the reference's grow.
  The large band costs +1300 over typical: the quick tier declines 2^-2.4 of the parts there and the middle tier
  answers them: about 0.38 declined parts per call, so roughly 3.4k instructions per declined part (an estimate
  from these rows, not measured directly).
* Trace (mei `libm-cplx-pow-5-trace`, wl_trace on the release products): 1484 instructions in one typical call; the
  release library has no internal symbols, so the split is by address only (mxcsr get/set are 6 of them). A phase
  split needs `--list` plus a symbolised build (WL_PROFILE=fast) or address ranges from the map.

## Next step (a new performance story; this branch should land as it is)

1. Profile first: per-phase split of the typical cpow call (WL_PROFILE=fast products or `--list` with the link map):
   clog quick parts, exp, sincos, double-double products, strict tests, entry overhead.
2. cpowf costs more than cpow (1568 against 1519) although it needs 25-bit answers: a float-precision quick tier
   (double arithmetic, no double-double) is the likely win; prove its bound the same way.
3. In-place entry (`cplx2_fast` in crates/boundary/src/mathret.rs mirroring `cplx_fast`, rows `cpow f`/`d` in
   crates/boundary/src/posix/s10_math.tsv, `cargo xtask math-gen`): only if the profile shows entry overhead worth it;
   the fast function must raise only inexact where it answers. More boundary code: check `xtask so` again.
4. Large band: a middle tier cheaper than about 3.4k per declined part, or a quick tier with a tighter bound for
   abs(E) > 16, aimed by the profile.

Do not redo: the proofs (log_dd 29.8u², atan_dd 34.6u²), the constants, the proof test, the decline sweep and its
long-sweeps row, the record text, the plants, the percall spec.
