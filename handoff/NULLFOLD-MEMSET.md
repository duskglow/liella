# NULLFOLD-MEMSET handoff (one page; NULLFOLD-MEMSET-3 run, final)

**State:** branch `task/nullfold-memset-3` (= task/nullfold-memset-2 + `db090059` + this handoff). Role shiki:~/w/sh1
(Haswell-EP). Report: handoff/reports/NULLFOLD-MEMSET-3.json. Plants: handoff/NULLFOLD-MEMSET-3-plants.txt.
**Decisions in force:** D-4.60 is now **REJECTED** (memset/__memset_chk keep the explicit NULL test on 65-256; D-4.22
option B unchanged for fills, its fill amendment removed). D-4.55 (copies 129-256), D-4.52, D-4.53 unchanged.
Ruling #3621 applied (branch 3: tests only, no memset code change, negative result recorded).

**Change now (vs next):** no memset code change (the `LIELLA_MEMSET_BODY` macro is byte-identical to origin/next's;
dispatch.rs/ffi.rs as next). Tests kept, expecting the abort above 64: small_copy `null_faults_in_the_folded_sizes...`
gains sizes 130, 255, 65_537, 1 MiB; `fills_store_their_head_block_first` (17-256, memset and __memset_chk, SSE2 page);
`null_fills_above_64_kib_abort_and_leave_a_page_at_64_kib_untouched`; classed_slots
`the_avx2_page_folds_null_up_to_64_and_stores_head_first` (AVX2 page: NULL 1-65537 both fold states; head-first 65-256).

**Profile (ruling #3621, ONE pass, N100 (nozomi), Liella cbench_memref, one row per run, perf stat :u, 4 event
groups of 4 + fixed, no multiplexing; 4 rounds x 15 runs, medians per run; job nozomi `nullfold-memset-3-prof-n100`):**

| event (per run) | m128 pre | m128 final(65-128 fold) | m128 d(one load) | m256 pre | m256 final | m256 d |
|---|---:|---:|---:|---:|---:|---:|
| cycles | 1.369M | 1.262M | 1.345M | 1.455M | 1.518M | 1.453M |
| topdown_fe_bound.decode | 1.346M | 0.983M | 1.318M | 0.316M | **1.143M** | 0.202M |
| topdown_fe_bound.frontend_bandwidth | 1.846M | 1.500M | 1.835M | 0.837M | 1.664M | 0.719M |
| fe_latency / icache / resteer / branch_detect | equal | equal | equal | equal | equal | equal |
| br_misp_retired / baclears / icache.misses / ld_blocks.address_alias | equal | equal | equal | equal | equal | equal |

Cause: decode-bound front-end slots, layout dependent (the 256 path runs pre's exact instructions in `final`); not
mispredicts, resteers, icache or aliasing. Fixing it means more layout variants: excluded by the ruling.
**Timing (nozomi `nullfold-memset-3-time-n100`, run.sh 6 x 25, bench ns / cycles):** m128 pre 3.35 / d 3.35 / final
2.94 ns; m144 4.19 all; m256 pre 3.78 / d 3.78 / final 4.19 ns. d's 128 gain (-1.8% cycles) is not visible in ns ->
ruling branch (2) fails -> branch (3).

**Jobs on `db090059`:** shiki `nullfold-memset-3-gate` (clippy --all-features, xtask clippy, static, affected --run
--base origin/next), `nullfold-memset-3-plants` (6 plants). Evidence (old staged trees, not the final head): nozomi
`nullfold-memset-3-prof-n100`, `nullfold-memset-3-time-n100`; trials `-proftry` (rc 3: an invalid event, dropped),
`-proftry-2`. Scripts: nozomi ~/work/NULLFOLD-MEMSET-3/prof.sh, ~/work/NULLFOLD-MEMSET-2/run.sh + stage-*.tar.

**Open / next:** none for this story. Optional own story: a decode-layout study of the memset entry on Alder Lake-N
(where the 65-128 and 129-256 bodies fall in the decoders' fetch blocks); only then could the fold come back.
**Not to redo:** padding/aliasing variants (a, b, c), the one-load variant d, this profile.
