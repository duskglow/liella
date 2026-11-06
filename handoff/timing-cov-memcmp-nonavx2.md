# TIMING-COV-MEMCMP-NONAVX2 (handoff, after run -4)

State (-4): branch `task/timing-cov-memcmp-nonavx2-4` = -3's head 88ee584c + merge of origin/next 71ffff12 (1ff83649)
+ this handoff. Role shiki:~/w/sh2 (Haswell-EP, AVX2).

## -4: CI conflict with TIMING-N100-MEMCMP-3 (fold.S) and the s04 decisions file
* fold.S, one hunk before `.Lcmp_sse_mid`: next's comment and its `.p2align 6, 0xcc` + `.fill 20` (the SSE2 classes
  start 20 bytes into a line) kept as they are; this story's -2 fold comment added after next's comment; the
  `.Lcmp_sse_mid` body (the `cmp_sse` word test, the 128 split after the probe) unchanged. Everything else merged
  on its own. Checked: AVX2 with folding off sends 65-128 to `.Lcmp_avx_long` -> `.Lcmp_big_load` -> `.Lcmp_big_blocks`
  (n <= 128), which next's 256-byte long loop does not reach, so -2's routing still holds.
* s04 decisions: next already carried -1's D-4.62 text; took next's rows as they are and replaced its D-4.62 with
  this branch's row (the same text plus the -2 fold paragraph; one D-4.62 row).
* hot-text.list: next has not changed it, tools/hottext or the test since the merge base 025c6c53, but it added the
  8 KiB classed memset section to the hot group. The test on the merged head (AVX2 host) is the check, see Jobs.
* No memcmp instruction changed by the merge: no re-count, no re-time (timing still never run, see "Not done" 1).
  -2's plants re-run on the merged head: 4 CAUGHT, 0 SURVIVED, the same 4 WARN-NO-MSG as -2 (see Plants below).
* Jobs (shiki): tcm-nonavx2-4-check0 (clippy+static on 88ee584c, rc 0), tcm-nonavx2-4-check (clippy+static on
  the merge, rc 0), tcm-nonavx2-4-plants (rc 0), tcm-nonavx2-4-gate (affected escalated to FULL by hot-text.list),
  tcm-nonavx2-4-htp (`cargo xtask so`, hot_text_products --nocapture, small_compare). Results: gate rc 0
  (FULL, 61 min: 32 suites, 11122 passed, 0 failed, 1200 ignored; conformance 122/0; peak 16G, oom_kill 0); htp rc 0:
  hot_text_products 8 passed, rebind warm-up 0, runs [0,0,0,0,0], startup 17 (bound 23), setlocale + printf 57 (68),
  tool 60 (71), fork 7 (9); small_compare 15 passed. The CI test passes on the merged head on an AVX2 host.
* Next step: unchanged, "Not done" 1 (booked timing on Ivy Bridge and the N100 forced SSE2), then 2.

State (-3): branch `task/timing-cov-memcmp-nonavx2-3` = -2's head 47828fec + 91b851e1 (regenerated hot-text list)
+ this handoff. Role kanon:~/w/kn1. Not merged with origin/next (the -3 brief allows no merge); the list was generated on
a scratch merge (origin/next 31ecf574) and merges cleanly (next has not changed the list, tools/hottext or the test).

## -3: CI red (hot_text_products init_second_bindtextdomain_gettext_takes_no_text_fault, rebind 1 fault)
* Cause: composition only. Branch alone passes (kanon, rebind 0); branch + origin/next fails on kanon too (Broadwell
  AVX2, rebind runs [1,1,1,1,1]), so it is the layout, not the CPU. perf `minor-faults -d` on the rebind program: the
  one fault in the window is the instruction fetch of `Resolution::same_texts` at 0x4802c0. The committed list had
  same_texts / Mo::hinted / Mo::form at h533-535, after the fork and _Exit functions, though p_cold first-touches them
  right after set_plan (h521): the list was older than its traces. This story's SSE2 compare code makes the hot group
  a few hundred bytes larger, which pushed same_texts across 0x480000 into a window nothing else touches first.
* Fix: `tools/hottext/regen.sh ~/w/kn1` (fast profile) on the scratch merge, output committed as is (no hand edit):
  the three now follow set_plan (0x47e080-0x47e900, ~6 KiB inside the first lookup's window). hot_text_products on
  the merge: 8 passed, rebind 0, true 17, locale 57, tool 60, fork 7 (before 1, 18, 59, 62, 8). Release-profile
  regen not run (merge.py keeps release-only keys anchored). No memcmp code changed: no re-count, no re-time, no plants.
* Jobs (kanon): tcm-nonavx2-3-check (clippy/static/so on 47828fec), -htp (branch alone), -htp-merged (reproduces),
  -regen, -htp-regen (fix on the merge), tcm-nonavx2-3-gate (clippy, static, direct gate on 91b851e1).

State (-2): branch `task/timing-cov-memcmp-nonavx2-2` = -1's branch (025c6c53) + merge of origin/next 18a2b58e (4f5eeeac)
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
