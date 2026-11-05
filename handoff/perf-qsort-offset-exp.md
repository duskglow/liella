# PERF-QSORT-OFFSET-EXP (experiment, DO NOT LAND as it stands)

State: branch `task/perf-qsort-offset-exp` (from origin/next f2157504), role keke:~/w/r3. Commits:
`validation/percall/qsort.c` (new wl_percall cases), the docs-only NEGATIVE malloc row (04-allocator.md), and the
experiment commit (core `search.rs` + boundary `s14_search.rs`).

## Finding 1: the brief's premise is out of date

Comparing by offset is already on next (QSORT-COMPARE-TRAIT): `liella::search::SortCompare::gt(base, a, b)` takes byte
offsets, and the boundary's `ByOffset`/`ByOffsetCtx` form the C pointers with `base.as_ptr().add(o)` (UNSAFE(slice),
no slicing, no bounds check). The "8 check instructions plus slice building" in register section 19 do not describe
the current code. Whether section 19's counts (2026-11-04) were taken before or after that change is not known here
(I did not read others' history); the pc1 job's `base` column gives the current figure.

## Finding 2: where the gap is (disassembly of next's build, not a wl_trace)

The wl_trace and baseline per-call jobs were queued behind about 6 jobs and kept the role busy (sync_role refused),
so I cancelled them. The split below comes from `objdump -d` of Liella's own `libc.a` (a cached products build of
another branch that does not touch qsort, `~/wl/products/38c60827...`; `merge_sort_index::<..ByOffset..>`).
Saved as keke:~/work/perf-qsort-offset-exp/msi.s.

* Merge loop (offset 0x520-0x597): **31 instructions per comparison** + the comparator (about 7 with the ret).
  11 values stay live across the C call (base, fn pointer, left buffer, idx, i, j, k, mid, n, a, b) and there are
  6 callee-saved registers, so **7 of the 31 are spill stores or reloads** (0x541, 0x55e, 0x563, 0x56c, 0x576,
  0x587, 0x592), plus the call goes through a stack slot. Two leas add the base to each offset. Bounds checks
  are 2 to 4 instructions, not 8.
* Binary insertion (runs of 8, 0x428-0x45a): about 13 instructions per comparison + comparator, the base reloaded
  from the stack each time and one bounds check.
* Per sort, outside comparisons: zeroing the 1.5n-word heap buffer, the permute pass, malloc/free: under 2
  instructions per comparison at n = 1,024 (estimate from the code, not counted).

So the surplus over glibc's ~25 library instructions per comparison is mostly register pressure across the
indirect call, not safety checks.

## The experiment (commit "Sort qsort positions by element address ...")

* The index sort's position table holds element ADDRESSES while merging (`base.as_ptr().expose_provenance()` plus the
  offset, safe Rust), turned back into offsets before `permute`. New trait method `SortCompare::gt_at` (default:
  subtract the base, call `gt`; so `BySlices` callers are unchanged). The boundary overrides it for `ByOffset` /
  `ByOffsetCtx` with `core::ptr::with_exposed_provenance` (a safe function; **no new unsafe**).
* The merge loop's output index is `i + j - mid`, not a third counter.
* Expected: 2 leas and 2 live values fewer per merge comparison, so about 3 to 5 instructions (**estimate, NOT
  VERIFIED**; job pc1 measures it).
* Safety argument: the same as QSORT-COMPARE-TRAIT's contract. Every value in the table is `start + m * size` with
  `m < n`, the merge and insertion only permute the table, so whatever the comparator answers the C side only
  sees whole elements of the caller's array, and the array stays untouched until the last comparison (permute
  after). The address carries the provenance `base` exposed; the debug build asserts the range (`at_addr`).

## Jobs (keke, all on head 6a451fc4, queued 18:02Z, results NOT READ at hand-back)

perf-qsort-offset-exp-lint (clippy -D warnings + xtask static), -gate (direct tier), -plants (6 plants,
keke:~/work/perf-qsort-offset-exp/plants.txt), -pc1 (wl_percall qsort.c, base f2157504 vs this head, glibc column),
-wlc (wl_count sort-utf8, base vs head). Re-attach: `ssh claude@10.0.100.53 '~/bin/jobwait perf-qsort-offset-exp-lint
perf-qsort-offset-exp-gate perf-qsort-offset-exp-plants perf-qsort-offset-exp-pc1 perf-qsort-offset-exp-wlc'`.

## Not done

* TIMING (the brief's item 3, owner: the final arbiter): no booking made; budget. Next run: `ops timing-book` an Ivy
  Bridge and an AVX2 host, time qsort_c_cmp_1024 / _e4 / _e8 / _e64 interleaved against glibc.
* wl_trace per-phase split of the e8/e64 rows (only e4's merge loop was read from the disassembly; the loop is
  the same code for every size, only the comparator's load differs).
* Per-row per-comparison numbers: pc1 prints per sort; divide by the comparison count its check mode prints to stderr.

## Next step

Read pc1 and wlc. If merge comparisons drop by 3 or more instructions and no row is slower, book timing. If that is
also no worse, bring "address-based positions in the index sort" to the owner (RULING NEEDED below). Otherwise
revert the experiment commit and add a NEGATIVE row to docs/saturation/02 and 19.
Further idea, not tried: a merge loop over two disjoint sources (ping-pong buffers, 2n words) removes the output
bounds check but adds a copy per in-order merge (sorted input gets slower); judge it only with counts.

RULING NEEDED: adopt address-based positions (`SortCompare::gt_at`) in core's index sort. Recommendation: only
with pc1 showing a gain on every qsort row and timing on Ivy Bridge no worse; it needs no new unsafe.
