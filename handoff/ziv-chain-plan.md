# ZIV-CHAIN-PLAN: the Ziv chain's bases after the cutover

State: branch task/ziv-chain-plan, base origin/next f3b3584a. Planning only: no code changed, no jobs run (JOBS: none).
Rulings in force: owner #1719 / #1758 (f128 complex lands only with the full Ziv step, ZIV-C), #1759 / #1904 (A/B/C
split, B into B1/B2), #1905 (cpow with huge |c|,|d| on the ZIV-C path). Decision row: D-10.17 (on the chain branches).

## Facts the plan rests on (checked 2026-11-04 with git ls-tree/grep/diff --stat on origin)

1. After the cutover every chain branch (math-m5-cplx-f128*, ziv-a*, ziv-b*, b1*, b2*) is ONE squashed commit on the
   public root 7374bf3f. They share no history with each other. origin/next = root + 2 commits (licence wording,
   2 files, no math). So `git merge origin/task/<older chain branch>` (the briefs' "Earlier runs' work" lines) adds
   nothing and can only produce add/add conflicts: do not merge older chain branches; start from the newest one.
2. B2-3 (fed89fc5) contains B1-2's work (a0b92f66): every file the B1-2 squash changes is also changed by B2-3's;
   mp.rs, mpw.rs, special.rs and wide.rs are byte-identical between the two; B1-2 -> B2-3 differs in 5 files
   (complex, tests, D-row). ln_atanh lives in crates/math/src/mp.rs and mpw.rs (briefs say "mp_kernels": that is the
   `mp_kernels` include-twice macro in mp.rs). origin/next has neither ln_atanh nor any cexpf128 name.
3. The B2-3 squash touches 14 files vs root: complex.rs, complex_body.rs (new), tests/complex_f128_products.rs,
   tests/oracle/complex_f128_diff.c, s10_math.tsv/_gen.rs, csys adapter math_libm.c, validation/percall/cplx_f128.c,
   plan/07-decisions/s10-mathematics.md (complex), and five non-complex files: math/src/mp.rs (+Ziv `Bounded`,
   `decided`, `ex`, `ln_atanh`, ln1p above 2^-6 and ln10 switched to ln_atanh), math/src/mpw.rs (768-bit kernels +
   tests), math/src/special.rs (-24/+1), math/src/coeffs/tables.rs, mathtools/src/tables.rs. Every pub fn of root's
   mp.rs/mpw.rs still exists on B2-3 (so root's complex.rs should still find its callees; NOT VERIFIED by a build).
4. Why B2-3's `math-m5-cplx-f128-ziv-b2-3-counts2` (keke) is rc 101: the script (keke
   ~/work/MATH-M5-CPLX-F128-ZIV-B2-3/counts2.sh) asserts that only cpow reaches the counted TODO(ZIV-C) path. On the
   accuracy suite (4844 calls per function) cexp 346, csinh 335, ccosh 344, csin 336, ccos 327, ctan 262, ctanh 269
   parts are still undecided at 768 bits (retries 281-360 each); every other family, cpow included, 0. About 7% of
   inputs against an expected ~2^-250 per part: almost certainly exact or structural parts not detected, or a run-time
   R3/R4 bound (cexp, mulf, tan_core, quot, proven in B2) far too loose for huge/tiny arguments - not genuine hard
   cases. The bigger ladder must not be used to absorb it.

## The plan

| # | Story | Starts from | Continuation? | Stacks-on (new) | Lands |
|---|-------|-------------|---------------|-----------------|-------|
| 1 | MATH-M5-CPLX-F128-ZIV-C | origin/task/math-m5-cplx-f128-ziv-b2-3 (new branch task/math-m5-cplx-f128-ziv-c cut from it) | yes, of B2-3 (and so of the whole chain) | MATH-M5-CPLX-F128-ZIV-B2-3 (was ZIV-B) | the whole complex chain |
| 1' | MATH-F128-WIDE-LN-PROVEN (parallel with 1) | origin/next + the three kernel files from B2-3 (below) | no | none (was ZIV-B1-2) | alone, before or after 1 |
| 2 | MATH-M5-CPOW-EXACT | origin/next after ZIV-C has landed | no | MATH-M5-CPLX-F128-ZIV-C (was CPLX-F128-2) | alone |
| 3 | MATH-M5-CLOSE | origin/next after 1 and 2 have landed | no | MATH-M5-CPOW-EXACT (was CPLX-F128) | alone |
| 4 | MATH-M6-CPLX-DIRECTED | origin/next after 3 has landed | no | MATH-M5-CLOSE (unchanged) | alone |

### 1. ZIV-C lands the chain
- Brief edits: replace "merge origin/task/math-m5-cplx-f128-ziv-b2, not ziv-b" and every "Earlier runs' work: git
  merge origin/task/..." line with: "Your branch is cut from origin/task/math-m5-cplx-f128-ziv-b2-3, which contains
  B1-2, B1-3, B2, B2-2 and B2-3 (and A-4, CPLX-F128-2). Merge no other chain branch. Merge origin/next once before the
  final gate."
- Step 0 (new, before the ladder): reproduce B2-3's counts, print the inputs whose parts are undecided at 768 bits for
  cexp/csinh/ccosh/csin/ccos/ctan/ctanh, classify them (exact part missed, bound too loose, genuine hard case) and fix
  the first two in the code. A regime that is genuinely undecidable within the ladder follows the M5-W rule (stop that
  regime, report it).
- Proof that replaces counts2: ZIV-C's own counts job (`math-m5-cplx-f128-ziv-c-counts`) on the same suites and a
  random sample, per function and per ladder step, asserting: no part reaches the 3072-bit top on the suites except
  through the forced hook, and the 768-undecided rate of the seven families is at the expected scale (0 on the suite)
  or each remaining input is listed and explained. Name it in JOBS. B2-3's counts2 row stays rc 101 in keke's
  jobs.tsv; the coordinator records it as superseded by the ZIV-C counts job rather than re-running B2-3.
- When ZIV-C lands, close as landed-through-ZIV-C: CPLX-F128-2, ZIV, ZIV-A-4, ZIV-B, B1-3, B2-3 (and older).

### 1'. WIDE-LN-PROVEN before the chain, without complex code
- A cherry-pick of "the ln_atanh commit" is not possible: after the cutover there is no such commit (B1-2 is one
  squash of the whole chain, complex included). Instead: branch task/math-f128-wide-ln-proven from origin/next, then
  `git checkout origin/task/math-m5-cplx-f128-ziv-b2-3 -- crates/math/src/mp.rs crates/math/src/mpw.rs crates/math/src/special.rs`
  as its first commit ("Bring in the proven ln_atanh kernels from the Ziv chain"); add coeffs/tables.rs and
  mathtools/src/tables.rs from the same branch only if the build needs them. Never take complex.rs, complex_body.rs,
  s10_math*, the csys adapter, complex tests or D-10.17.
- First check (cheap): `cargo xtask clippy` and the direct tier on that commit. If root's complex.rs does not build
  or its tests fail against the new kernels, stop and hand back: WIDE-LN then waits for ZIV-C and starts from
  origin/next after it lands (no complex code to carry then).
- Conflict rules while 1 is open: identical hunks in mp.rs/mpw.rs/special.rs merge cleanly when ZIV-C merges next.
  WIDE-LN edits wide.rs and special.rs freely; it must NOT remove `mp::ln` from mp.rs if any complex site on B2-3
  still calls it (check with git grep on that branch), and adds its D-rows as new rows, not edits to D-10.17. ZIV-C's
  1536/3072 steps should go in new cold functions (its brief already asks), which keeps mp.rs conflicts small.

### 2-4. After the chain
- CPOW-EXACT changes the integer-power path of cpow at all four widths; the f128 one lives in complex_body.rs, which
  ZIV-C is changing (#1905). Running it in parallel would conflict, so it starts from next after ZIV-C lands. Drop its
  "merge origin/task/math-m5-cplx-f128" line (that is in next by then).
- CLOSE needs M5-1..M5-7 on next and regenerates docs/spec-symbol-gaps.* once: run it after ZIV-C and CPOW-EXACT land.
  It does not need WIDE-LN (not an M5 story).
- CPLX-DIRECTED keeps Stacks-on MATH-M5-CLOSE and starts from next after CLOSE lands.

## Open questions / not verified
- That root's complex.rs builds against B2-3's mp.rs/mpw.rs (fact 3) is inferred from the pub fn lists, not built.
- The cause of the seven families' 768-undecided parts is inferred from the rate, not diagnosed.
- Whether a second jobs.tsv row with B2-3's counts2 label would satisfy the dispatcher: not checked; superseding
  is recommended instead.

## Exact next step
Coordinator: set the Stacks-on and brief edits above, release ZIV-C and WIDE-LN-PROVEN together, hold CPOW-EXACT, CLOSE
and CPLX-DIRECTED behind them in that order.
