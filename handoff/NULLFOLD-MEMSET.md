# NULLFOLD-MEMSET handoff (one page; NULLFOLD-MEMSET-2 run)

**State:** branch `task/nullfold-memset-2` (= task/nullfold-memset + merge of origin/next `18a2b58e` + the narrowing
commit `d0de5adb`). Role shiki:~/w/sh1 (Haswell-EP). Report: handoff/reports/NULLFOLD-MEMSET-2.json.
**Decisions in force:** D-4.60 (AMENDED this run: 65-128 only), D-4.55 (copies 129-256, from next), D-4.52 (model),
D-4.53 (slot pages), ruling #3603 (profile 256; if layout does not explain it, land 65-128 alone).

**Change now:** with folding on, `memset`/`__memset_chk` 65-128 has no NULL test (reached only while
`liella_machine_set_mid` = 128). 129-256 is back to `lea/cmp/ja` + explicit `testq/jz` (the pre-D-4.60 code); the
`liella_machine_set_hi` word is gone. Tests: fills expect SIGSEGV up to 128 (small_copy, classed_slots renamed
`the_avx2_page_folds_null_up_to_128_and_stores_head_first`); the head-first tests for 17-256 are kept unchanged.

**Why (N100 probe, 6 rounds x 25 runs, Liella cbench_memref one row per run under `perf stat`, fixed iteration
count, user cycles per run, medians; nozomi jobs `nullfold-memset-2-layout-n100`, `-n100-2`):**

| variant (page 0 memset) | memset_128 | memset_144 | memset_256 |
|---|---:|---:|---:|
| pre (D-4.60 reversed: both tests) | 1.361M | 1.511M | **1.450M** |
| head (65-256 folded, 2 memory compares on the 256 path) | **1.259M** | 1.513M | 1.501M |
| a: head, body padded (int3, never run) to pre's 0xf8 | 1.261M | 1.515M | 1.501M |
| b: head, body padded to 0x100 | 1.260M | 1.514M | 1.501M |
| c: head, `set_hi` moved 1 KiB (other page offset) | 1.258M | 1.517M | 1.505M |
| d: one `set_mid` load, 129-256 bound 2*rcx | 1.341M | 1.510M | 1.453M |

Layout (a, b) and 4K aliasing of the word (c) do not explain it; the second memory compare does (d). Haswell-EP
showed no difference. NEGATIVE row in docs/saturation/03-memory-and-string-kernels.md.

**Jobs on the code head `d0de5adb`:** shiki `nullfold-memset-2-gate` (clippy --all-features, xtask clippy, static,
affected --base origin/next), `nullfold-memset-2-plants` (handoff/NULLFOLD-MEMSET-2-plants.txt, 4 plants),
`nullfold-memset-2-stage-3` (stage of the final head). Earlier: `nullfold-memset-2-stage`, `-stage-2` (probe stages),
`-trycmd`, `-trycmd-2` (script trial on shiki). The old `nullfold-memset-counts` rc 5: its script called
`cargo xtask bench --stage` without the tarball path (counts were fine; counts-2 had them).

**FINAL HEAD MEASURED WORSE (DO NOT LAND as is):** nozomi `nullfold-memset-2-final-n100-2` (same probe, 6 x 25):
memset_128 final 1.263M / pre 1.359M / head 1.261M; memset_144 1.522M / 1.518M / 1.518M; **memset_256 1.516M / 1.457M /
1.502M**. The final head's 256 path executes exactly pre's instructions (only the 65-128 test/jz before it is gone, so the
129-256 block sits 9 bytes earlier, `.Lset_129` at 0xd9 as in head), yet it is slower: the "second memory compare" model
is WRONG. Facts so far: slow = head (target 0xd9, body 0xe2), a (0xef/0xf8), b (0xf7/0x100), c, final (0xd9/0xef);
fast = pre (0xe2/0xf8), d (one load, rcx bound). Suspect the branch layout of the whole entry (the 0xa2 `ja`, its target,
predictor aliasing) or the 65-128 block's length, not one offset. Next: perf on the N100 (branch-misses,
topdown/front-end events if available) for pre vs final memset_256; try pre with only the 65-128 testq/jz replaced by
an equal-length never-taken form or nop padding inside the 65-128 class (keeps the layout of pre).
**Next step (after that):** read the jobs above; time the final head against next on the N100 (`run.sh` probe, stage-final) and
Ivy Bridge (`--compare cbench_memref`, kotori or nico); expected memset_128 faster, 144/256 equal to pre.
Scripts: shiki and nozomi ~/work/NULLFOLD-MEMSET-2/{stage.sh,run.sh} (run.sh <rounds> <reps> <variant>...).
**Not to redo:** the layout/aliasing probe above. Then CLASSED-SLOT-SWAP-COST (unchanged).
