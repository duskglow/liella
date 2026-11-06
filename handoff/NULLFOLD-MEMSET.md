# NULLFOLD-MEMSET handoff (one page)

**State:** branch `task/nullfold-memset` (stacked on CLASSED-SLOT-INFRA-7 `a8f38570`); code `21d14c18` + `f04d9084`,
plants-file fix `209f520e`. Role shiki:~/w/sh1 (Haswell-EP, AVX2 class).
**Decisions in force:** D-4.60 (new, reserved by `ops reserve D-4`), D-4.22 option B amended for fills (quoted ruling),
D-4.52 (model), D-4.53 (slot pages, swap OFF by default, ruling #3434).

**Change (D-4.60).** With folding on, `memset`/`__memset_chk` 65-128 and 129-256 have no NULL test:
* 65-128: reached only when `liella_machine_set_mid` = 128 (folding on), so its `testq/jz` went.
* 129-256: a new word `liella_machine_set_hi` (256 with folding on, else 0; raised after set_mid, dropped before it,
  same cache line) replaces `n - 129 <= 127` + the NULL test with one compare, on both slot pages.
* Folding off/undecided: both words 0, every n > fold limit reaches `.Lset_out`; `.Lset_rest` keeps 129-256 in assembly
  after the explicit test (as before) and sends 1-128 and NULL to Rust (S6 abort).
* Head-first: every store of both classes is in `[d, d+256)`, head block at `d` first, inside the guard's 64 KiB.
* `__memset_chk` is Rust that calls the asm entry after its fortify check: folded with it. The internal fill (D-X.15)
  has no NULL test (contract n > 0), nothing to fold.

**Measure-first (which classes carried NULL tests; instr/call, `bench --compare --counters cbench_memref`, page 0,
the default on both CPU classes; base = this branch with the D-4.60 code reverse-patched; shiki jobs
`nullfold-memset-counts` (base) and `nullfold-memset-counts-2` (branch)):**

| row | class (NULL test before) | base | branch | glibc |
|---|---|---:|---:|---:|
| memset_16, _64, _mix | 1-64 (already folded) | 20.3 / 19.4 | 20.3 / 19.4 | 19.4-20.3 |
| memset_128 | 65-128 (testq/jz) | 26.3 | **24.3** | 27.3 |
| memset_144 | 129-256 (lea/cmp/ja + testq/jz) | 37.4 | **34.4** | 34.3 |
| memset_256 | 129-256 | 37.3 | **34.4** | 34.3 |
| memset_4k | >256 rep stosb (test KEPT) | 26.8 | 25.6 | 25.7 |
| memset_64k / 4k_off | >256 (KEPT) | 40.2 / 13.0 | 40.1 / 12.9 | 39.9 / 12.9 |

Kept, with reason: every size above 256 (aligned-`d` `rep stosb`, the AVX2 fill, the Rust large fill): those stores
can reach past 64 KiB (n unbounded) and the test is 2 instructions before a >=257-byte fill (4k: 25.6 of which the test
is 2; the path got 1 cheaper anyway from the shorter class chain). No memset row went up by instructions.
The AVX2 page (swap on) was NOT counted: `GLIBC_TUNABLES=liella.compat.no_classed_swap=0` on the bench gave page 0's
numbers (37.4 base), so the bench did not swap; its correctness is covered by the classed_slots test below.

**Timing** (staged `--stage` tarballs of base and branch, 4 alternating rounds, `~/bin/timing`):
nozomi (N100) `nullfold-memset-time-nozomi`, nico (Ivy Bridge) `nullfold-memset-time-nico`, both rc 0. Liella ns/call
medians, 4 rounds each (all rounds identical unless shown):

| row | N100 base | N100 branch | Ivy Bridge base | Ivy Bridge branch |
|---|---:|---:|---:|---:|
| memset_128 | 3.35 (one 3.56) | **2.94** | 4.81 | **4.38** |
| memset_144 | 4.19 | 4.19 | 7.43 | 7.43 |
| memset_256 | 3.78 | **4.19 (slower, 4/4 rounds)** | 7.43 | 7.43 |
| memset_16/64/_mix, 4k, 4k_off | equal | equal | equal | equal |
| memset_64k | 2053-2103 | 2101-2116 | 2698-2709 | 2699-2702 |

**Verdict: NOT landable as is.** memset_256 on the N100 is one timer step slower in every round with 3 fewer
instructions; 144 (same class) is equal. Unexplained: the row is layout-sensitive (CLASSED-SLOT-INFRA-7 saw it move
4.19 -> 3.78 on a path change), so suspect the 129-256 body's placement (the class chain before it got 5-10 bytes
shorter), not the removed test. 64k on the N100 overlaps (noise; that path only lost one compare).

**Tests:** small_copy.rs `null_faults_in_the_folded_sizes_and_aborts_in_every_other` (SIGSEGV for memset/__memset_chk
1-256 folded; abort off and above 256, n = 65 537 and 1 MiB added), `folded_fills_store_their_head_block_first`
(d 16 B below a shared writable page over PROT_NONE, n 17-256: page untouched), `null_fills_above_64_kib_abort_and_
leave_a_page_at_64_kib_untouched` (page mapped at 0x10000 in a child; plain, rep stosb and AVX2 fill), classed_slots.rs
`the_avx2_page_folds_null_up_to_256_and_stores_head_first` (after the swap; folding on and off; head-first 65-256).
**Plants** (handoff/NULLFOLD-MEMSET-plants.txt, `cargo xtask plants`): run 1 `nullfold-memset-plants` 5 CAUGHT +
2 WARN-NO-MSG (the two unguarded plants abort the test binary through the allocator's heap check, "free(): invalid
pointer": the planted class stores before d with folding off); msg regexes fixed; run 2 `nullfold-memset-plants-2`:
7 CAUGHT, 0 WARN, rc 0. Gate: shiki `nullfold-memset-gate` (clippy + static + `affected --base origin/next --run`).
Plant-tool note: in a `perl:` pattern write `\x40` for `@` (perl interpolates `@:` as an array; `.Lset_big\\@:` matched
nothing until changed).

**Next step (continuation):** find why memset_256 is slower on the N100: profile (cycles, not instructions) base vs
branch for memset_256; try the 129-256 body at the base's alignment (e.g. `.p2align 4` at `.Lset_129_body`, or pad
the 65-128 class back) and re-time 128/144/256 on the N100; if no layout explains it, keep only the 65-128 fold (128
faster on both classes) and time that. Then CLASSED-SLOT-SWAP-COST (unchanged).
**Not to redo:** the counts above; plants run 2. Scripts: shiki ~/work/NULLFOLD-MEMSET/{gate,counts,counts2}.sh;
time.sh + stages on nozomi and nico ~/work/NULLFOLD-MEMSET/.
