# TIMING-SWEEP-1105 (COMPLETE: table and REOPEN/CLOSE lists in results/timing-sweep-1105.md)

State: branch task/timing-sweep-1105, base origin/next f2157504 (products built from this tree, no code change).
Build: shiki role sh1, job timing-sweep-1105-build2 rc 0 (`cargo xtask bench --build-only`, 21 programs, 302 s).
The first build job (timing-sweep-1105-build) was rc 128 (my `git rev-parse` in a synced, git-less role dir); superseded.

## What is running (left running on purpose, 3 calls left of the 40-call budget)
Both timing hosts are booked (ops bookings #30 nico Ivy Bridge E3-1220L v2, until 19:42Z; #31 nozomi Intel N100 =
Alder Lake-N, 4 E-cores, AVX2, until 19:52Z). Each runs under ~/bin/timing (labels timing-sweep-1105-nico2,
timing-sweep-1105-nozomi) the script ~/work/TIMING-SWEEP-1105/run.sh:
`./xtask-bin bench --no-build --counters --only <all non-libm groups>` in ~/work/TIMING-SWEEP-1105/tree.
Output: ~/work/TIMING-SWEEP-1105/out/bench.log (last line `rc=N` when done) and out/{counters,rounds,ratio_host,decision}.tsv.
Started 17:15Z (nico) / 17:23Z (nozomi); 18:14Z not finished. The last full V-4 run took ~70 min with libm.
Stage on each host: stage.tar from shiki (tree = target/liella, bench/, xtask/Cargo.toml, debug xtask binary as xtask-bin).
The bench's own `sudo -n true` probe prints "sudo: a password is required" (xtask/src/bench.rs:1019); it is the tool's probe, harmless.

## Exact next step (a continuation, ~6 calls)
1. Wait: `ssh claude@nico 'grep rc= ~/work/TIMING-SWEEP-1105/out/bench.log'` and the same on nozomi until both show rc=0.
2. `scp` out/counters.tsv and out/rounds.tsv from each host into <dir>/nico and <dir>/nozomi; run
   `python3 -I handoff/timing-sweep-1105-analyze.py <dir>` (in this branch): writes table.md (per-row instr ratio, time
   ratio = median of per-round ratios, spread = (max-min)/median of rounds, verdict faster <0.95 / parity / slower >1.05) and
   dis.md (REOPEN: instr <=1.05 and time >1.05; CLOSE: instr >1.05 and time <=1.05; spread statistics; verdict counts).
3. Write results/timing-sweep-1105.md from those, one cause-class line per disagreement (rep string, latency/memory,
   instruction mix, layout), say N100 verdict is for that core type; cancel the bookings
   (`ops timing-cancel 30`, `ops timing-cancel 31`); clean ~/work/TIMING-SWEEP-1105 on both hosts if wanted.
Instruction ratios come from the same run's counters.tsv (same CPUs, same code), not from a separate count.

## Not to redo
The build and the staging (done). Do not start the benches again while the runs are alive (kill by PID otherwise).
No code was changed. Plan rows: V-4 handoff/benchmarks-v4.md, docs/benchmarks.md section 6.

## Run 2 (TIMING-SWEEP-1105-2)
Copied counters/rounds from nico (.112, bench rc=1: nscache first_getpwnam_r_cached/create budget vs uncached) and nozomi (.118, rc=0), ran the analyzer, wrote results/timing-sweep-1105.md and -table.md. Open follow-ups: REOPEN rows (Ivy Bridge strchr/malloc/memcmp family), nscache budget on Ivy Bridge.
