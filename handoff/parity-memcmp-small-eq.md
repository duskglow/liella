# PARITY-MEMCMP-SMALL-EQ (handoff)

State: branch `task/parity-memcmp-small-eq` (from origin/next 4b765f96), code commit e47db4ab, handoff commit on top
(HEAD in `handoff/reports/PARITY-MEMCMP-SMALL-EQ.json`). Role keke:~/w/n4 (Zen 1 host).

## Change
csys/src/machine/fold.S `.Leq_17` (bcmp / __memcmpeq, 17-32 bytes): the head probe has already proved bytes 0-7, so
the word at 8 (`movq 8(%rdi)`, `cmpq 8(%rsi)`, unequal -> 1) and one 16-byte block at n - 16 replace the two 16-byte
blocks and their AND. [8, n) is covered for every n in 17-32. Static count: -1 instruction on the equal and tail paths,
the head path unchanged, results unchanged (0 / 1). Nothing else changed: no gate word, dispatch.rs untouched.
crates/boundary/tests/small_compare.rs: MAX 256 -> 300 and a fifth shard (257-300), so the oracle sweep (every size,
every alignment pair up to 130, every first-difference position for three pairs, SSE2 and AVX2 builds, folding on
and off) covers 0-300 as the brief asked.

## Profile (static, in-function instructions; the bench adds about 8.4)
| path | entry | probe | size tests | blocks | total |
|---|---:|---:|---:|---:|---:|
| eq 17-32 (bcmp/meq), before | 5 | 3 | 4 | 11 | 23 |
| eq 17-32, after | 5 | 3 | 4 | 3 + 7 | 22 |
| memcmp diff8 33-64 | 3 | 4 | - | 7 + 8 | 22 (glibc ~16) |
| memcmp diff8 / head above 64 | 8 | - | - | 5 + 8 | 21 (glibc ~16) |
Where the rest of the surplus sits and why it stays: the saturation row added in
docs/saturation/03-memory-and-string-kernels.md (32 on the AVX2 block, MEMCMP-MID-4 timing; the 33-64 head probe,
sort-C; a 17-64 AVX2 class costs 32-64 rows; the entry ladder above 64).

## Jobs (keke)
* parity-memcmp-small-eq-before (rc 0, unchanged tree 4b765f96): `__memcmpeq_eq_32` 31.4 / `_tail_32` 31.4,
  `bcmp_eq_32` / `_tail_32` 31.4; percall meq_eq_17/24/32 34, bcmp_eq_17/24/32 34.
* parity-memcmp-small-eq-plants: 3 plants, 3 CAUGHT, 0 WARN (rc 0).
* parity-memcmp-small-eq-gate: clippy, static ok; affected: every suite ok except `test -p liella-boundary (exports
  interposed)`: `search_nomem::tsearch_returns_null_and_leaves_the_tree_walkable`, child abort "fatal runtime
  error: Unexpected TLS failure" (rc 134) under its forced out-of-memory. No compare kernel on that path (the
  failure is in the Rust runtime's TLS setup while allocation is failing). NOT PROVEN unrelated.
* parity-memcmp-small-eq-after (counts + percall after; chained after the gate) and parity-memcmp-small-eq-nomem
  (search_nomem x10 on the final head): NOT READ at hand-back.

## Next step
Read parity-memcmp-small-eq-after: expect each 17-32 eq/tail row 1 lower, no row higher. If nomem fails 0 of 10,
the gate failure is intermittent (a defect: file it against search_nomem and the TLS-under-nomem path). If it fails
every time, compare with origin/next before blaming this change. Brief targets (1.05x) are NOT reached; see the
saturation row for what would be needed.
