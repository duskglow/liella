# PRE14-BENCH-AVX2-COUNTS

State: branch task/pre14-bench-avx2-counts (rebased on origin/next 87bc22cb). docs/benchmarks.md gains the AVX2-class counts (section 2a, Data used row, notes, section 5).

Measured: Skylake-SP host (mei, Xeon Platinum 8151, 48 threads, avx2 + avx512), shared, not booked (counts only). Products: a6fc34c2 (the V-4 commit, from a scratch worktree synced to role s3), not current origin/next, so rows compare with the Ivy Bridge run on identical code.
Command: cargo xtask bench --compare --counters --runs 3 <17 programs>. Default (no names) runs only cbench_opt; cbench_nscache aborts the run ("liella/native.h: No such file": the host build gets no -I include, bench.rs line 65 adds it only for the Liella build). So 17 programs, in two jobs (full1b 65 min, full2 35 min), both rc 0.
Result: 529 counted rows: 187 faster / 90 parity / 252 slower (without libm 360: 186/87/87; libm 169: 1/3/165); median ratio without libm 0.93 (Ivy 0.86). 89 rows change band vs Ivy Bridge, 22 are parity-vs-slower.
Not redone: 5cc49dc counts; AVX2 timing; AVX-512 split (the CPU has AVX-512; try --tunables glibc.cpu.hwcaps=-AVX512F).
Open: Liella is relatively slower on the AVX2 class for strlen/strnlen/memchr/rawmemchr/memset/memcmp/strcmp rows (19 rows faster on Ivy, slower here). memcpy_4k Ivy 26.33 vs 0.17 here: unexplained.
INFORMATIONAL: cbench_nscache cannot be compared (host build lacks -I include); a bench command with no names runs only cbench_opt, "full bench" needs the 18 names listed. Scratch: /home/claude/w/.scratch/PRE14-BENCH-AVX2-COUNTS/work (analyze.py, an.txt = all 89 flips).
