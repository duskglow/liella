# COMPARE-HEAD-H2: head-first compare entry (memcmp / bcmp / __memcmpeq), checkpoint 1

State: branch `task/compare-head-h2` (base origin/next 5872ad47e), code commit "Compare entries: compare the head
block before any dispatch (H2)" + plants file commit. Role keke:~/w/r5. Decision row D-4.66 (s04) written.

## What changed (csys/src/machine/fold.S, dispatch.rs, ffi.rs, tests/small_compare.rs)
* Both entries: `cmpq liella_machine_cmp_head(%rip), %rdx; jb .Lcmp_lo|.Leq_lo`, then the 16-byte SSE2 block at
  offset 0 (difference: memcmp bsf + re-read + -1/1 with the D-4.21 rescan kept; equality returns 1), then 16-32 the
  block at n - 16; above 32 `n <= cmp_long` (2048 with AVX2) -> existing AVX2 classes (`.Lcmp_wide`/`.Leq_wide` now
  start with `cmp $64; ja .L*_big_load`, the 8-byte probe dropped); else 33-64 SSE2 blocks 16/n-32/n-16 (the word-at-8
  probe dropped), 65+ -> `cmp_sse` test then `jmp .Lcmp_sse_65`/`.Leq_sse_65` (new labels past the classes' own head
  probe; the phase-pinned SSE2 code is not moved), or `.Lcmp_sse_over`/`.Leq_huge` as before.
* `.L*_lo`: n < 8 -> the explicit-NULL 0-7 classes (their now-dead `n >= 8` test removed); 8 <= n < `cmp_short`
  -> the 8-byte words at 0 and n - 8 (memcmp: no bswap unless they differ); else the checked Rust entry.
* Gate words: `cmp_wide`/`cmp_low` removed; `liella_machine_cmp_head` (16 folding on, else SIZE_MAX; .data start
  value SIZE_MAX) and `liella_machine_cmp_short` (16 on, else 8; start 8). Each alone keeps unfolded sizes off the
  folded paths, so store order does not matter; both go to the off state before the fold limit drops.
* Folding OFF / undecided: every size >= 8 now takes the checked Rust entry (`liella_hot_compare_rest` /
  `liella_hot_equal_rest`); before, >64 used assembly classes with explicit NULL tests. Performance only (rare
  state: page zero mappable, or before the lazy fold decision); correctness unchanged.
* NULL tests: folded from 8 at every size (2049, 4096, 1 MiB added) per ruling #3839; folding off aborts.
* fold.S header: a D-4.66 paragraph added; the older class-selection prose below it is marked as predating D-4.66
  (rewrite it in a follow-up; class bodies unchanged).

## Static path counts (from the listing, in-function incl. ret, NOT VERIFIED; jobs below measure them)
| row | before | after |
|---|---|---|
| memcmp head (diff in bytes 0-15), any n >= 16, both ISAs | 19-21 / 3-4 taken | 15 / 1 |
| __memcmpeq/bcmp head | 14-15 | 10 / 1 |
| memcmp/__memcmpeq 8-15 equal | ~19 / ~13 | ~16 / ~12 |
| SSE2 eq 16-32 | ~26 | ~17 |
| SSE2 eq 33-64 | ~35 | ~30 |
| SSE2 eq 65-128 (to the class's `cmp $128`) | 16 | 17 (+1) |
| **AVX2 eq 33-64** (memcmp / eq) | 19 / 17 | **26 / 25 (+7 / +8)** |
| **AVX2 eq 65-128** | 28 | **34 (+6)** |

**Finding (RULING NEEDED, against #3840's premise):** H2 puts head block (6) + `n > 32` test (2) + ISA test (2) +
class split (2) in front of the AVX2 bodies, against the ladder's 3-8. The rethink's "about +1" estimate left out
the `n > 32` and class tests and the head work the AVX2 body repeats. So AVX2 equal rows above 32 bytes are about
+6..+8 instructions (static), not +1, and the brief's "+1 at most" proof condition cannot be met by H2 as specified.
Options: (a) accept H2 if booked N100 timing shows AVX2 rows within noise (the ruling's own condition);
(b) ISA-split head: AVX2 builds use a 32-byte ymm head for n >= 32 (needs an ISA test before the head for
n >= 32: +2 on SSE2 head rows) ; (c) keep H2 for SSE2 only (Ivy Bridge, the target) and route AVX2 n > 32 via the
old ladder position (costs the +2 gate on AVX2 head rows). Recommendation: measure (a) first (counts are queued);
if N100 time is worse on eq_64/eq_128, try (c).

## Jobs (keke)
* `compare-head-h2-gate` (clippy, static, `affected --run --base origin/next`): clippy, static, `xtask so`,
  test -p liella (both modes), liella-abi, liella-boundary (both modes, small_compare included), liella-ld,
  liella-legacy OK; the rest was still running at hand-back.
* `compare-head-h2-percall-avx2-c`, `compare-head-h2-percall-sse2-c` (memcmp_bands.c, `--base none`: Liella vs
  glibc only; compare with the base numbers in handoff/compare-head-rethink.md or prime base 5872ad47e on keke).
  SUPERSEDED: `compare-head-h2-percall-avx2`, `-sse2` (rc 2: short base sha); `-avx2-b`, `-sse2-b` (base not
  primed on keke).
* Plants: NOT RUN. handoff/COMPARE-HEAD-H2-plants.txt (9 plants) is committed; after the gate:
  sync, then `cargo xtask plants handoff/COMPARE-HEAD-H2-plants.txt --dir /dev/shm/claude/COMPARE-HEAD-H2/plants-1`.
* Timing: NOT BOOKED (Ivy Bridge, N100, Skylake-family). Timing hosts drain 20:00 UTC 11-07.

## Next step
Read the gate (`~/bin/jobwait compare-head-h2-gate` on keke) and fix any failure; run the plants; read both
percall logs (`~/bin/bench-summary`); with the AVX2 numbers decide (a)/(b)/(c) above; then book timing.
Not verified: harness correctness sweep (handoff/compare-head-rethink/harness.c) against the new entries.
