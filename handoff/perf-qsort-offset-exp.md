# PERF-QSORT-OFFSET-EXP(-2): address-based positions in qsort's index sort (ADOPTED, ready to land)

State: branch `task/perf-qsort-offset-exp-2` (stacks on `task/perf-qsort-offset-exp`, cut from origin/next f2157504;
origin/next has since moved only in docs/saturation/22, no overlap, so no merge was made). Role keke:~/w/n3 (the first
run used keke:~/w/r3). Code commit: 6a451fc4 (core `search.rs`, boundary `s14_search.rs`); later commits are docs and
handoff only.

## Decisions in force

* Ruling #3097 (owner, 2026-11-05): ADOPT address-based positions (`SortCompare::gt_at`). The Ivy Bridge verdict comes
  from the next TIMING-SWEEP: revert this change if it times slower there. The comparison-count gap with glibc (about
  4% more comparisons) is its own story.
* No D-row was added (no id reserved; the ruling is the record). No compat option: no behaviour change.

## What the change is

While merging, the index sort's position table holds element addresses (`base.as_ptr().expose_provenance()` plus the
offset, safe Rust), turned back into offsets before `permute`. `SortCompare::gt_at` (default: subtract the base,
call `gt`) is overridden in the boundary for `ByOffset` / `ByOffsetCtx` with `core::ptr::with_exposed_provenance`
(a safe fn; no new unsafe; debug assert of the range in `at_addr`). The merge output index is `i + j - mid`.
Why it wins: the merge loop's surplus was register pressure across the C call (7 of 31 instructions per comparison
were spills, plus 2 leas adding the base), not bounds checks; two fewer live values.

## Evidence (all on keke, Zen 1 host)

* pc1 (`wl_percall` qsort.c, base f2157504 vs 6a451fc4): every qsort row -8.1% to -9.9% per sort, about 4.5
  instructions per comparison; comparator-only control unchanged; comparison counts identical before/after.
  Table in docs/saturation/19 (new "Update" subsection). Still 1.13 to 1.35x glibc per comparison.
* wlc (sort-utf8): instructions:u new/base 1.0000, output SAME.
* gate perf-qsort-offset-exp-gate (direct tier, 6a451fc4): 2354 passed, 0 failed; peak 2.3G, oom_kill=0.
* lint perf-qsort-offset-exp-2-lint (clippy --workspace --all-targets --all-features -D warnings + xtask static,
  on fc69b97a): rc 0.
* plants perf-qsort-offset-exp-2-plants (`cargo xtask plants`, keke:~/work/perf-qsort-offset-exp-2/plants.txt,
  /dev/shm/claude/perf-qsort-offset-exp-2/plants-1): 6 CAUGHT, 0 SURVIVED; only WARN-DEV-SHM (informational).

## The failed job and its cause

perf-qsort-offset-exp-plants rc 2: the plants file named package `liella-core`, which does not exist (core's
package is `liella`). The baseline failed, so no plant ran. Cause of the mistake: docs/plants.md's own example says
`cargo test -p liella-core --test stdio_short_io`. That doc example should be fixed (not done here: one change per
branch).

## Not to redo

The disassembly split (merge loop 31 instr/comparison, binary insertion about 13) and the pc1/wlc counts above.

## Next step

Land. Then: the next TIMING-SWEEP times the qsort rows (Ivy Bridge, AVX2) against glibc; revert if slower. Separate
stories: the comparison-count gap; a ping-pong merge over two buffers (removes the output bounds check, adds a copy
on in-order merges; judge with counts, sorted input included).

Plan rows to read: section 19 of the saturation register; ruling #3097.
