# TIMING-N100-SCAN-64K (handoff, after run -3)

State: branch task/timing-n100-scan-64k-3 (stacks on -2), code head e2c744c6 (scan.S layout pin), later commits plan/
and handoff/ only. Role mei:~/w/s1. Verdict of run -3: COMPLETE for the layout fix + 256-byte loops (no prefetch);
prefetch (pf512) NOT adopted yet: its Skylake-SP timing was still queued at hand-back.
Decisions: D-NEW (plan/07-decisions/s04, placeholder: the coordinator reserves the id), rulings #3322, #3371.
Scratch: mei:~/work/TIMING-N100-SCAN-64K-3 (stage3.sh, run3.sh, scan-{base,lay,laypf}.S, {base,lay,laypf}.tar,
measure-*.tsv, syms-*.txt); nozomi, nico: ~/work/TIMING-N100-SCAN-64K-3/out (results below).

## Change (csys/src/machine/scan.S, AVX2 level)
256-byte aligned groups in the AVX2 memchr/rawmemchr loops (runs -1/-2). Run -3: the AVX2 long paths are assembled
in `.subsection 1` (after the SSE2 code) and `.org <entry> + 0x231` (memchr) / `+ 0x1ed` (rawmemchr), int3 fill,
pins each SSE2 path at the offset it had before the 256-byte loops. Assembled base vs head: the SSE2 path bytes
and every SSE2-executed head byte are identical, same relocations (only AVX2-only jump displacements differ).
Sections: memchr 0x57d -> 0x73e, rawmemchr 0x493 -> 0x5f6 (code after them moves by a multiple of 64).
Linked bench (cbench_memref): memchr entry 0x425200 in base and head; rawmemchr entry 0x495740 -> 0x495940.

## Timing (run3.sh 5 rounds x 3 alternations, base/lay/laypf interleaved; L new/old = median per-round ratio)
| row | IVB L/g base | IVB L/g head | IVB head/base | N100 L/g base | N100 L/g head | N100 head/base | N100 pf512/head |
|---|---|---|---|---|---|---|---|
| memchr_16_mix | 1.162 | 1.159 | 1.000 | 0.617 | 0.617 | 1.000 | 1.000 |
| memchr_64_mix | 0.928 | 0.940 | 1.000 | 0.467 | 0.465 | 1.000 | 1.000 |
| memchr_4k | 0.872 | 0.874 | 1.000 | 1.036 | 0.988 | 0.953 | 0.998 |
| memchr_64k | 1.005 | 1.004 | 1.000 | 1.151 | 1.056 | 0.918 | 0.940 |
| rawmemchr_16_mix | 0.800 | 0.794 | 1.000 | 0.641 | 0.641 | 1.000 | 1.000 |
| rawmemchr_64_mix | 0.857 | 0.849 | 1.000 | 0.758 | 0.758 | 1.000 | 1.000 |
| rawmemchr_4k | 0.953 | 0.953 | 1.001 | 1.047 | 1.035 | 0.988 | 1.003 |
| rawmemchr_64k | 1.010 | 1.011 | 1.000 | 1.059 | 1.060 | 1.002 | 0.870 |
glibc drift 0.994-1.006 everywhere. IVB pf512/head: all rows 0.999-1.001. Jobs: nico:timing-n100-scan-64k-3-ivb,
nozomi:timing-n100-scan-64k-3-n100 (both rc 0). The Ivy Bridge memchr_16_mix regression of run -2 (+14%) is gone.
Instructions: unchanged from run -2 (layout only); pf512 rows 4k/64k have no bench/instr_limits.tsv rows (limits
exist only for <= 128 B rows, all unchanged), so pf512 stays within the file as it is.

## Jobs (mei, final code e2c744c6)
gate timing-n100-scan-64k-3-gate (clippy, static, affected --base origin/next); plants timing-n100-scan-64k-3-plants
(9 plants: 6 CAUGHT, 0 SURVIVED, 3 WARN-NO-MSG, all three SIGSEGV on the guard page, log checked); rerun with
`|SIGSEGV` in memchr-256-end, raw-256-hi, raw-256-fold, raw-256-lo: timing-n100-scan-64k-3-plants2 (from a copy of
the plants file in ~/work, so the gate's role tree was not re-synced mid-run). Skylake-SP timing
timing-n100-scan-64k-3-skx (base/lay/laypf, booking 71) was queued behind the gate at hand-back.

## Staging recipe (timing hosts have no compiler)
stage3.sh: write the variant's scan.S (fresh mtime), `cargo xtask bench --build-only --only cbench_memref`, copy
target/debug/xtask to xtask-bin, then `tar -cf <v>.tar Cargo.toml crates csys target/liella bench xtask/Cargo.toml
-C <work> xtask-bin`. The top-level Cargo.toml is needed by xtask's root(); crates/ and csys/ (with their mtimes,
older than the build) by the --no-build freshness check. run3.sh extracts and runs `./xtask-bin bench --no-build`.
Start with `~/bin/timing <label> bash run3.sh 5 3 <variants>` called directly.

## Exact next step
1. Read mei:~/logs/timing-n100-scan-64k-3-skx.log (out in mei:~/work/TIMING-N100-SCAN-64K-3/out); analyze with
   the analyze3.py approach (handoff/TIMING-IVB-STRSCAN-DISPATCH-2-analyze.py with the side names as arguments:
   `base lay`, `lay laypf`). If Skylake-SP times no row slower with laypf: adopt pf512 (scan-laypf.S = head + 4
   prefetcht0 at the top of each 256-byte loop; ruling #3371), recount, gate, plants, and re-time N100 is done
   (pf512 column above). Else record the numbers in docs/saturation and keep prefetch out.
2. Cancel booking 71 when the skx job ends.
Not to redo: Ivy Bridge and N100 timing of base/lay/laypf (above), the layout analysis.
