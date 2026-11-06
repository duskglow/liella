# PARITY-MEMCMP-SMALL-EQ (handoff)

State (PARITY-MEMCMP-SMALL-EQ-2, the continuation): branch `task/parity-memcmp-small-eq-2` = the first run's two
commits + a merge of origin/next bf079683 (carries SEARCH-NOMEM-TLS) at a56dffe7, handoff commit on top. Role
mei:~/w/s4 (Skylake-SP). No code change in the continuation: the merge, then gate and plants again (see "Jobs (mei)").
First run: branch `task/parity-memcmp-small-eq` (from origin/next 4b765f96), code commit e47db4ab, role keke:~/w/n4.

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
* parity-memcmp-small-eq-after (rc 0, read by the continuation): `__memcmpeq_eq_32` / `_tail_32` and `bcmp_eq_32` /
  `_tail_32` 30.4 (was 31.4; glibc 26.4 / 20.4 / 27.4 / 24.4); every other memcmp/bcmp/__memcmpeq row equal to
  before, none higher. parity-memcmp-small-eq-nomem: search_nomem 0 of 10 failed (intermittent; its cause and fix
  were SEARCH-NOMEM-TLS, now on next).

## Jobs (mei, continuation, on a56dffe7 = the change merged with origin/next bf079683)
* cargo xtask clippy rc 0, cargo xtask static rc 0 (foreground on s4 before the jobs).
* parity-memcmp-small-eq-2-gate (rc 0): `cargo xtask affected --base origin/next --run` under `~/bin/gate 16G`;
  11 suites ok, 10310 passed, 0 failed, 1232 ignored; conformance 120 passed; liella-boundary (exports interposed)
  ok (the first run's red). Peak 15G (page cache included), oom_kill 0. liella-math took 2075 s at load ~30.
* parity-memcmp-small-eq-2-plants (rc 0): handoff/PARITY-MEMCMP-SMALL-EQ-plants.txt, 3 plants, 3 CAUGHT, 0 SURVIVED,
  0 WARN.

## Next step
None for this change: it should land. Brief targets (1.05x on the 17-32 eq/tail rows) are NOT reached; the
saturation row in docs/saturation/03-memory-and-string-kernels.md records what would be needed and what was tried.
