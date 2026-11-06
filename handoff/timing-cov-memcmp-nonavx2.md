# TIMING-COV-MEMCMP-NONAVX2 (handoff)

State: branch `task/timing-cov-memcmp-nonavx2` from origin/next e119b8bc (PARITY-MEMCMP-SMALL-EQ already merged there).
Code commits ac5ef91e (classes), ab5ff78d (diff search by block group), 092dc543 (plant escape); handoff on top.
Role kanon:~/w/kn1. Decision row D-4.57 (plan/07-decisions/s04-byte-arrays-and-strings.md).

## Change (csys/src/machine/fold.S, non-AVX2 paths only)
* Compare 65-128 (`.Lcmp_sse_mid`): explicit NULL tests, then the block at 0 **alone** (was 0 and 16 ANDed), a head
  difference goes straight to `.Lcmp_first`; then blocks 16, 32, 48, n-64, n-48, n-32, n-16 ANDed into the last
  block's register (no `movdqa`); the first difference: the three blocks at 16-63 ANDed pick the group, then that
  group's masks in order (the last block's from the AND).
* Compare >128 (`.Lcmp_sse_long`): probe the block at 0 alone; loop from (a+16)&-16 (in [a+1, a+16]); chunks AND
  into the fourth block (no copy); `.Lcmp_sse_ldiff` takes that mask from the AND (it decides only when the three
  blocks before are equal).
* Equality (`bcmp`, `__memcmpeq`): 33-64 `.Leq_33` (word at 8, blocks 16, n-32, n-16) instead of the Rust
  `liella_hot_equal_tail`; above 64 without AVX2 (`.Leq_huge`, cmp_mid 0) `.Leq_sse` instead of the Rust
  `liella_hot_equal_big`: NULL tests (NULL -> the checked entry, S6 unchanged), words at 0 and 8, then 7 blocks
  (65-128) or 64-byte chunks + a last chunk at n-64. AVX2 code paths untouched (no instruction changed on them).
* Tests: crates/boundary/tests/small_compare.rs already sweeps 0-300 (+129-2049, NULL cases) with both builds
  forced and folding on/off; the five sweep shards now name clauses s4_3_c_1/2/3 (were "no spec clause").
* Percall spec handoff/timing-cov-memcmp-nonavx2/tcm.c (tc1106.c's shapes, bcmp/__memcmpeq checked as `r != 0`).

## Profile (in-function instructions; the bench loop adds ~10-11)
memcmp head/diff8 65-127 after: entry ladder 5 + jns 1 + cmp_long 2 + `n > 128` 2 + NULL tests 4 + probe 6 +
bsf 1 + re-read/order 6 = 27 (38 measured; glibc 26 total). Ladder + NULL tests (14) alone exceed glibc's whole
call minus overhead (~15). bcmp/meq head: 5+1+2+2+4+3+2 = 19 (30 measured; glibc 26 / 22).

## Counts (kanon, SSE2 build = GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2; glibc / base e119b8bc / new; job tcmn-percall-sse2-2)
memcmp_diff8_96 26/45/38, diff8_127 26/45/38, diff8_64 26/36/36, head_128 26/45/38, head_1k 26/47/40, eq_128 67/63/62,
tail_128 75/94/91, 256 107/118/110, 1k 323/334/314, 4k 1187/1198/1130; bcmp head_128 26/42/30, head_1k 26/42/30,
eq_64 42/57/41, eq_128 67/74/64, eq_1k 323/392/312, eq_4k 1187/1448/1128, tail_64 50/57/41; meq head_128/1k 22/42/30,
eq_64 41/57/41, eq_128 65/74/64, eq_1k 322/392/312, tail_64 41/57/41, tail_128 65/74/64, eq_32/tail_32 29/33/33,
diff8_100 22/42/33. No row above base; 0 MISMATCH. AVX2 build: job tcmn-percall-avx2 (see report json).
Superseded: tcmn-percall-sse2 (ac5ef91e: memcmp_tail_128 94 -> 98, fixed by ab5ff78d), tcmn-gate / tcmn-plants
(stopped, old head).

## Target (1.05x) NOT reached for: memcmp diff8_64/96/127 (1.38-1.46), bcmp head (1.15), meq head (1.36),
meq/bcmp 17-32 (1.14, the AVX2-shared class of PARITY-MEMCMP-SMALL-EQ, untouched). Reached: every eq/tail row
>= 64, memcmp_256 (1.03), 1k, 4k. Why the head rows stop: the shared entry ladder (8, fixed by the AVX2 classes'
carry gates) and the S6 NULL tests (4). RULING NEEDED: NULL folding for the SSE2 65-2048 classes (as D-4.52 for
AVX2; -4 per call above 64) - recommend yes if the owner accepts SIGSEGV for those sizes without AVX2.
Bigger lever (design, not tried): a separately dispatched SSE2 entry (no AVX2 carry ladder), est. -8.
diff8_64 (33-64 class): the word at 8 sits behind `n > 16` and `n > 32` (17-32 class shared with AVX2).

## Not done / next step
Booked timing (nico or kotori: Ivy Bridge rows; nozomi: AVX2-class rows not slower) NOT run (context budget);
a continuation books it: `cargo xtask bench --stage` on kn1, rows of this file, Liella vs glibc. The Rust
`liella_hot_equal_tail` export is now unreferenced by the kernels (kept; removal is a follow-up).
Jobs to read: tcmn-gate-2, tcmn-plants-2 (9 plants), tcmn-percall-sse2-2, tcmn-percall-avx2 (kanon).
