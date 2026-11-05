# TIMING-IVB-MALLOC-CHAIN: the latest-allocation fast path in free, timed (experiment). DO NOT LAND

State: branch task/timing-ivb-malloc-chain, code commit 03d7c5f7 on next ff125cce (the code of
task/perf-malloc-latest-free-exp dd873a8e, applied unchanged: heap.rs has not changed on next since that branch's
base). Role shiki:~/w/sh1. Coordinator ruling #3181: an experiment, hand back DO NOT LAND with data; the owner rules.

## Decisions in force / not to redo

- The owner did not adopt this path on instructions (#2826; register row "malloc latest-allocation fast path in free",
  docs/saturation/04-allocator.md). This run re-judges it by time, as asked. No D-12 row written (the owner rules).
- Do not redo: storing K_HINT only on change (negative twice); the instruction counts below (same as the first run).
- The "variant that costs nothing on the other frees" was not tried: no description of it was found in the branch's
  handoff, report or register row, and the budget went on the timing. See RULING NEEDED.

## Timing (interleaved base / new / glibc per round; base = next ff125cce, new = 03d7c5f7; both timing hosts booked)

Probe: the cbench_malloc `pair()` loop and the `realloc_usable()` loop alone (same helpers and volatile pointers),
50M pairs / 20M realloc cycles, pinned to one CPU, `perf stat`, 5 rounds, medians per call. Liella static, glibc 2.39
dynamic. Workloads: `~/wl/rc_time.sh` (perf stat -r 3 per round, interleaved), kits relinked by wl_count on the Zen 1
host for both shas, inputs of `~/wl/work` (same md5 on every host).

| row | Ivy Bridge base / new / glibc cycles | new/base | N100 base / new / glibc cycles | new/base |
|---|---|---|---|---|
| malloc_free_16 | 52.48 / 44.81 / 46.91 | 0.854 | 27.28 / 24.05 / 37.09 | 0.882 |
| malloc_free_32 | 52.88 / 44.92 / 44.84 | 0.850 | 27.28 / 24.06 / 37.09 | 0.882 |
| malloc_free_64 | 52.72 / 44.84 / 44.49 | 0.850 | 27.29 / 24.06 / 37.09 | 0.882 |
| malloc_free_128 | 52.54 / 44.67 / 44.20 | 0.850 | 27.28 / 24.05 / 37.09 | 0.882 |
| malloc_free_256 | 52.60 / 45.32 / 44.36 | 0.862 | 27.29 / 24.05 / 37.09 | 0.882 |
| malloc_free_1024 | 52.75 / 44.83 / 46.51 | 0.850 | 27.29 / 24.05 / 37.09 | 0.882 |
| realloc_within_usable | 70.98 / 65.25 / 57.84 | 0.919 | 43.10 / 40.10 / 51.15 | 0.930 |

Instructions per call (both hosts): pair 122.2 -> 110.1 (glibc 148.1); realloc cycle 191.2 -> 179.2 (glibc 196.2).
Ivy Bridge resource_stalls.rs per pair 20.2-20.6 -> 16.5-17.0 (glibc 5.0-7.3): rs-full is still the gap; the path
cuts about a fifth of it. Time per call moves with cycles on both hosts (new/base ns = cycles ratio within 0.012).

| workload | Ivy Bridge new/base cycles:u (10 rounds) | IVB cyc_u ranges base ; new (M) | N100 new/base wall (5 rounds) | N100 wall ranges base ; new (ms) | instr:u new/base |
|---|---|---|---|---|---|
| du | **1.045** | 76-77 ; 80-81 | 0.995 | 277.7-278.3 ; 276.1-277.0 | 1.0111 |
| gawk-regex | 1.008 | 1350-1382 ; 1363-1397 | **1.018** | 409.7-413.5 ; 417.2-419.3 | 1.0141 |
| tar-c | 1.012 (noisy) | 61-65 ; 61-72 | 0.990 | 127.0-127.8 ; 125.6-126.4 | 1.0047 |
| make-n | 1.000 | 464-468 ; 464-470 | 1.000 | 175.5-179.7 ; 175.8-178.9 | 1.0038 |

Ivy Bridge wall and task-clock are not usable for workloads on this host: rounds spread +-20% for the SAME binary
(du 452-716 ms) while cycles:u stays within 2%, so the IVB column uses cycles:u (the 5-round run agreed: du 1.0445,
gawk 1.002, tar 0.999, make 1.003). N100 wall spreads under 1%.

## Reading

- The pair rows gain 14-15% (Ivy Bridge) and 12% (N100) in time, realloc_within_usable 8% and 7%; on Ivy Bridge
  the pair goes from 1.12-1.19x glibc to 0.96-1.02x (realloc stays 1.13x glibc there).
- Real programs get SLOWER in time: du +4.5% user cycles on Ivy Bridge (ranges do not overlap), gawk-regex +1.8% wall
  on N100 (ranges do not overlap). Under "no real workload may get slower overall" this does not pass as it stands.
- Why du loses 4.5% cycles for +1.1% instructions is NOT VERIFIED (a likely cause by the model: in programs whose
  frees are not of the latest block, the added compare-and-branch in every free mispredicts and the extra two stores
  per malloc compete; not profiled).

## RULING NEEDED

- D-12, latest-allocation fast path in free: recommendation **do not adopt**; record the timing as a second negative
  (pair -15%/-12% but du +4.5% IVB cycles, gawk-regex +1.8% N100 wall). If the owner wants the pair gain, the next
  experiment is a variant with no cost on non-latest frees (to be specified; first profile du on Ivy Bridge for
  branch misses in free to confirm where its 4.5% goes).

## Gate, plants, counts (final code head 03d7c5f7)

- shiki:timing-ivb-malloc-chain-gate: clippy, static rc 0 (its `affected` only listed: a role dir needs `--run`).
- shiki:timing-ivb-malloc-chain-gate2: `cargo xtask affected --base origin/next --run` rc 0: 7 suites ok, 918 passed,
  0 failed; conformance 9 passed. Peak not recorded.
- shiki:timing-ivb-malloc-chain-plants (`cargo xtask plants ~/work/TIMING-IVB-MALLOC-CHAIN/plants.txt`): 5 CAUGHT,
  0 SURVIVED, 0 WARN (wrong word, wrong bit, no clear in refill = stale latest reuse, no liveness test = double free
  skipped, no remote test).
- keke:timing-ivb-malloc-chain-wlc (wl_count, Zen 1, base ff125cce): instructions:u new/base gawk-regex 1.0141,
  du 1.0112, tar-c 1.0050, make-n 1.0039; faults 0.985-0.999; syscalls equal; output SAME everywhere.
- nico:timing-ivb-malloc-chain-nico, nico:timing-ivb-malloc-chain-nico-wl10, nozomi:timing-ivb-malloc-chain-nozomi
  (bookings 35 and 36, cancelled after use). wl_percall malloc rows not run (the probe gives per-call instructions).

## Next step

Owner ruling. If not adopted: add the timing to the register row above (docs/saturation/04-allocator.md) and close
the branch. Scratch (probe, run.sh, summ.py, raw tsv): /home/claude/w/.scratch/TIMING-IVB-MALLOC-CHAIN/; on the
timing hosts ~/work/TIMING-IVB-MALLOC-CHAIN/ship/out/.
