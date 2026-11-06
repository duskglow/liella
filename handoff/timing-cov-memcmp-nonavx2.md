# TIMING-COV-MEMCMP-NONAVX2 (handoff, after run -2)

State: branch `task/timing-cov-memcmp-nonavx2-2` = -1's branch (025c6c53) + merge of origin/next 18a2b58e (4f5eeeac)
+ 394fd4b0 (the SSE2 NULL fold) + this handoff. Role mei:~/w/s3. Decision row D-4.62 (s04 file; -1's classes + the fold).

## -1 (unchanged, see D-4.62): SSE2 head probe first in memcmp 65-128/long; SSE2 asm equality classes 33-64, 65-128, long.

## -2: NULL fold of the SSE2 65-2048 classes (ruling #3592, owner's standing NULL-folding rule)
* fold.S: `liella_machine_cmp_sse` (2048 with folding on and no AVX2, else 0) replaces the compare's `n <= 128`
  test at `.Lcmp_sse_mid` (the 65-128 split moves after the head probe) and gates `.Leq_sse`; no NULL test below
  it (first access: offset 0 of a, then b; read-only). Above it `.Lcmp_sse_over` / `.Leq_sse_over`: AVX2 test (compare),
  then `cmpq liella_machine_cmp_sse_null, %rdi; jbe checked` (0 while folded: a NULL test; else SIZE_MAX: folding off
  sends every size > 64 to `liella_hot_compare_big` / `liella_hot_equal_big`), b NULL test, probe, long class.
  Why two words: the only way to keep AVX2 rows and SSE2 >2048 rows at their old instruction count (the a-bound word
  stands in for the a NULL test, the size word for the 128 test). Store order in `dispatch::store_cmp_gates`:
  up size word first, down a-bound first (the bound is never 0 while the size word is 0, else 65-128 would reach the
  long class). AVX2 with folding off now sends 65-128 to `.Lcmp_avx_long` (AVX2 class, explicit NULL tests).
* Tests (small_compare.rs): new `null_faults_at_every_sse2_folded_size_and_aborts_with_folding_off` (every n 64-2049,
  6 calls, SSE2, fold on SIGSEGV to 2048 / abort at 2049, off abort; ~25 s); the old NULL test expects SIGSEGV 65-2048
  in both builds.

## Counts (mei, job timing-cov-memcmp-nonavx2-2-percall-sse2-b; glibc / base next 18a2b58e / new; -1's new on kanon)
memcmp head_128 26/45/34 (-1: 38), head_1k 26/47/34 (40), diff8_96 & _127 26/45/34 (38), diff8_64 26/36/36,
eq_128 67/63/60 (62), tail_128 75/94/89 (91), 256 107/118/106 (110), 1k 323/334/310 (314), 4k 1187/1198/1130 (1130);
bcmp head 26/42/28 (30), eq_128 67/74/62 (64), eq_1k 323/392/310 (312), eq_4k 1187/1448/1128 (1128);
meq head 22/42/28 (30), diff8_100 22/42/31 (33), eq_1k 322/392/310. No row above base or above -1. AVX2 (job
...-percall-avx2-b): base = new on every row.

## Target (1.05x) NOT reached: memcmp head/diff8 1.31-1.38, meq head 1.27, meq diff8 1.41, bcmp head 1.08,
memcmp tail_128 1.19. Profile (memcmp head_128, 34 measured, ~23 in function): entry ladder 5 + jns 1 + cmp_long 2
+ cmp_sse 2 + probe 6 + bsf/re-read/order 7. glibc's whole call ~15 in function. The shared entry ladder (8,
AVX2 carry gates) is the remaining surplus: a separately dispatched SSE2 entry (no AVX2 gates; est. -8 on every
SSE2 row) is the next lever, not built (context budget; the brief's condition "head rows above 1.10x" holds).

## Not done / next step (in order)
1. BOOKED TIMING (required by the coordinator before landing; NOT run in -1 or -2): memcmp/bcmp/__memcmpeq head,
   diff8, tail, eq rows, next vs head, Ivy Bridge (less busy of kotori/nico) and the N100 forced SSE2
   (GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2); `cargo xtask bench --stage` on the role, unpacked whole; CBENCH_ROWS.
2. Then the separate SSE2 entry (profile above).
Plants: handoff/TIMING-COV-MEMCMP-NONAVX2-2-plants.txt (8: 4 CAUGHT, 4 WARN-NO-MSG: the fold-gate-off and
a-null-dropped plants failed the correctness sweep (65-128 into the long class), not the NULL test named).
Jobs (mei): timing-cov-memcmp-nonavx2-2-check, -gate, -plants, -percall-sse2-b, -percall-avx2-b.
