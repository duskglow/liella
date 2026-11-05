# PARITY-MEMCMP-AVX2 (handoff)

State: branch `task/parity-memcmp-avx2` (from origin/next 62826474); code 352f94c1 (fold.S + test), a57a30f9 (test sizes),
69b6620c (plants anchors); HEAD in `handoff/reports/PARITY-MEMCMP-AVX2.json`. Role keke:~/w/n3 (Zen 1 host, AVX2, no AVX-512).

## Change (part (c) of the brief only: the long loop)
* `csys/src/machine/fold.S`: with AVX2 on, sizes above `liella_machine_cmp_long` (2048; every size above 128/64 with
  folding off) no longer go to the Rust large classes (`liella_hot_compare_big` / `liella_hot_equal_big`, about 18
  instructions per 128 B plus the call): `.Lcmp_avx_long` / `.Leq_huge` do the explicit NULL tests (NULL -> the checked
  entry, S6 abort as before; no folding above 2048) and enter the same AVX2 head block and long class as 129-2048.
* The long classes (memcmp loop and last chunk, equality tail chunk and loop) test the ANDed chunk with one
  `vptest` against the head block's all-ones ymm0 (CF clear = a byte differs) instead of `vpmovmskb`+`inc`+`jne`:
  16 instructions per 128 B instead of 17. The memcmp loop's spare AND now uses ymm6 so ymm0 stays all-ones.
* Test: `mid_and_long_classes_find_the_first_difference_at_every_size_to_2049_in_each_build` also covers
  2050-4096 at chunk-boundary sizes (the fence part puts a and b in one 8 KiB window, so sizes stop at 4096: a first
  try with 4097-8192 failed on that overlap, a test geometry error, not the kernel).

## Per-phase split (static, from the listing; instructions entry to ret, bench overhead about 8.4)
| path | before | after |
|---|---|---|
| > 2048, AVX2 | entry 6 + class words 6 + Rust compare_big (NULL tests, class tests, loop ~18/128 B) | entry 6 + class words 6 + NULL 4 + jmp 1 + head 5 + size tests 6 + setup 4 + 16/128 B + last chunk 18 |
| 129-2048 | loop 17/128 B | 16/128 B |
Expected: memcmp_4k about 554 (Rust before: 669), memcmp_64k about 8240 (before 9324).

## Not done (budget; reasons)
* (a) 17-32 head and (b) mismatch-in-first-vector rows: every design measured statically trades rows. Moving 17-32 to
  VEX 16-byte blocks behind the wide carry (cmp_wide 48) saves 5-8 on 17-32 but adds a size test (+2) to every 33-64
  call (memcmp_64 / bcmp_64 rows rise, sort -C's 17.8M 33-64 calls see it on equal/late shapes). The 65-2048 head-diff
  rows pay 8 instructions of entry dispatch (two class-word adds, sign, cmp_long) before the first load; a
  dispatch that reaches them sooner adds 2 to every 8-64 call. Not changed (the "no row slower" rule); see
  handoff/perf-memcmp-tails.md and handoff/memcmp-33-64-recover.md for the earlier analysis.
* No AVX-512 path (the Skylake-SP gap at 4k/64k is glibc's own lower count there).
* Counts on the Skylake-SP and Ivy Bridge classes: no role on those hosts for this story (dispatcher). Ivy Bridge has no
  AVX2, so this change cannot reach its rows (SSE2 classes untouched).
* No timing run (no booking in this budget).

## Jobs (keke) and counts (`cargo xtask bench --compare --counters --runs 3 cbench_memref`, Liella / glibc instr)
* parity-memcmp-avx2-count-before (origin/next, rc 0) and parity-memcmp-avx2-count-after (rc 0, the code of 69b6620c
  run from a RAM copy of the role, /dev/shm/claude/parity-memcmp-avx2/count-1, because the gate held the role): bench and
  percall (`validation/wl_percall.sh --spec validation/percall/avx2_families.c`).
* memcmp_4k 669.1 -> 554.1 (576.1, 1.16 -> 0.96); memcmp_64k 9319.6 -> 8245.1 (8748.1, 1.07 -> 0.94);
  __memcmpeq_eq_1k / bcmp_eq_1k 165.6 -> 157.6 (171.7 / 172.6); memcmp_256 52.5 -> 52.4; 10 rows lower, none in
  the memcmp/bcmp/__memcmpeq set higher. Percall memcmp_eq: 1024 172 -> 164 (169), 4096 665 -> 557 (577),
  16384 2393 -> 2093 (2209), 65536 9305 -> 8237 (8737); 16-256 unchanged.
* Rows up 0.4-1.5 instr in code this change does not reach (scan.S kernels): memchr_64k 6699.9 -> 6700.3,
  rawmemchr_64k 5683.4 -> 5684.9, memmem_miss_4k / strstr_miss_4k +0.5 (at most 0.03%; read as counter noise, NOT VERIFIED).
* Unchanged and still above 1.05 here: __memcmpeq_tail_32 1.54, eq_32 1.19, bcmp_tail_32 1.28, bcmp_eq_32 1.15,
  memcmp_diff8_64/96/127 1.25/1.21/1.21, __memcmpeq_head_128/1k 1.20 (see Not done).
* parity-memcmp-avx2-gate (direct tier, --base origin/next) rc 0. parity-memcmp-avx2-plants-3: 6 plants, 6 CAUGHT, no
  WARN (plants file handoff/PARITY-MEMCMP-AVX2-plants.txt, run from ~/work/parity-memcmp-avx2/plants.txt, the same
  content: the role sync was refused while the gate ran). Superseded: -plants and -plants-2 (rc 3, refused by the
  tool: unanchored .S plants).
* clippy (`cargo xtask clippy`) and `cargo xtask static` rc 0 on the role at 352f94c1; the later commits touch only
  the test file's size list and handoff/.

## Next step
Skylake-SP and Ivy Bridge count jobs from the dispatcher (labels parity-memcmp-avx2-count-avx2 / -count-ivy), and a
booked timing run of memcmp_4k/64k and the 1k equality rows on an AVX2 timing host. Then a story for (a)/(b) only with a
ruling that allows a band trade (for example 17-32 VEX blocks at +2 on 33-64), or an entry design that sees more than n.
