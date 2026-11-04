# PERF-RC1024-HEAP-FREE handoff

State: branch `task/perf-rc1024-heap-free`, code commit d5f37b33 on origin/next ce0a9ce6. Role keke:~/w/n1.

## What was done
- Only two `crates/rt/src/heap.rs` landings lie between fa678f74 and the release: df879f4a (K_HINTV, a
  copy of the hint word's value in class word 7) and fda78913 (its compare without black_box). The
  release's heap.rs is byte-identical to fda78913's.
- Count on keke, release head ce0a9ce6 vs fa678f74 (instructions:u, new/base): gawk-regex 1.0110,
  gawk-utf8 1.0073, make-n 1.0053, sort-C 1.0000, sort-utf8 0.9960 (log
  keke:~/logs/perf-rc1024-heap-free-bisect.log). This matches RC-1024's A/B.
- d5f37b33 removes K_HINTV: heap.rs is again byte-identical to fa678f74's (blob 2cba2def). The copy
  costs every fast free one store and every fast malloc a load, two compares and a branch, and
  a stale copy (after alloc_group or the slow path) runs the malloc through both paths.

## Results on d5f37b33 (keke)
- Gate `perf-rc1024-heap-free-gate` rc 0: clippy -D warnings, static, affected --base origin/next: 7 suites,
  766 passed / 0 failed, conformance 9/0, peak 9.8G.
- Count `perf-rc1024-heap-free-count` rc 0, instructions:u vs fa678f74 (new/base): gawk-regex 1.0000
  (3324864994 vs 3324860493; release 3361296508), gawk-utf8 1.0000 (3449607270; release 3474613587),
  make-n 0.9989, sort-C 1.0000, sort-utf8 0.9960, sort-n 1.0000. Page faults and gating syscalls equal
  within 1. All outputs SAME. So the two heap.rs landings explain the whole gawk regression.

## Not verified
- The per-step bisect (df879f4a, fda78913 separately) failed: wl_count refuses a ~/wl/src/<sha> dir
  ("neither a git tree nor a role directory"). The count of d5f37b33 vs fa678f74
  (`perf-rc1024-heap-free-count`) tells us whether heap.rs explains the whole regression. The rest of the
  773 changed files may also contribute.
- No plants: the change only removes code, which leaves heap.rs exactly as at fa678f74, already
  covered by that tree's tests. No per-call heap pair bench (`cargo xtask bench --counters`) was run: out of budget.

## RULING NEEDED
- Removing K_HINTV gives back the shorter load chain the PERF-HEAP-PAIR-IVB-2/3 stories measured in the
  Ivy Bridge malloc+free pair (cycles), in exchange for the instructions:u gawk lost. Recommend landing the
  removal if the count shows gawk-regex/gawk-utf8 at or below 1.0000. If an IVB pair cycle gain is
  wanted back, it needs a design that does not add work to every free (for example, refreshing the copy only
  in the malloc slow path).

## Next step
Read the `perf-rc1024-heap-free-count` table. If gawk is still above fa678f74, bisect the non-heap
landings: make a RAM copy of ~/wl/src/<sha> with a `.synced` holding `head=<sha>` so wl_count accepts it.
Then run the heap pair per-call rows: `cargo xtask bench --compare --counters` (malloc/free pair).
