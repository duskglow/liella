# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra-7` (continues -6/-5/-4/-3/-2/-1). origin/next `c48dffb4` merged in
(`4052dbe5`, it carries SEARCH-NOMEM-TLS). Plants-file commits `41343c69`, `4ee8b339`. Role mei:~/w/s3 (Skylake-SP,
AVX2 class).
**Decisions in force:** D-4.53 (swap OFF by default, run -6), COMPAT-20, rulings #3138-#3141, #3297, #3434.

**Run -7 (this run):**
* **Merge conflicts.** `csys/src/machine/fold.S`: next's PARITY-MEMSET-SMALL-AVX2 replaced the `n - 65` compare with a
  compare against the memory word `liella_machine_set_mid` (128 with folding on, 0 off), and moved the 129-256 compare
  to `n - 129 <= 127`. Taken into `LIELLA_MEMSET_BODY` as `liella_machine_set_mid+\bias` (XREF: page 1 reads the
  word through the +4096 bias) with `\@` labels. The AVX2 runtime flag test at 129-256 from next is NOT taken: page 0
  stays SSE2 there and page 1 is AVX2 there (D-4.53). Decisions file: D-4.53 and next's D-4.56 both kept.
* **Gate** mei `classed-slot-infra-7-gate` rc 0 (affected --base origin/next on `41343c69`; peak 15G; the step that
  was red in -6, `test -p liella-boundary (exports interposed)`, is ok: the cause was SEARCH-NOMEM-TLS, now merged).
  clippy (`cargo clippy --workspace --all-targets --all-features -D warnings`) and static clean on `4ee8b339`.
* **Plants** mei `classed-slot-infra-7-plants` (run 1): 12 CAUGHT, 1 SURVIVED: the new `xref-bias-set-mid` (page 1
  reads the set_mid word without the bias). Equivalent by result: the word only picks between the 65-128 assembly
  class and the Rust path (same bytes), and the misread word was at most 128 in this link (the size sweep passed).
  Declared `equivalent:` like run 1's biasless fold-limit read. Rerun `classed-slot-infra-7-plants-2` on `4ee8b339`: 12 CAUGHT, 1 EQUIVALENT, 0 WARN, rc 0.
* **N100 start-up (run -6 job, nozomi `classed-slot-infra-6-n100`):** within noise or faster. exec_true_static mb
  165.0-166.1 us, branch 160.8-161.7 (-2.7%, ranges disjoint); exec_hello_locale 353-356 vs 343-345; exec_true_echo
  234.5-237.3 vs 234.1-235.2; spawn_bin_true 437.7-440.1 vs 437.7-439.5. Done.
* **memset 129-256 on the AVX2 class (the -6 INFORMATIONAL).** The rows exist: `memset_144`, `memset_256` (and
  `memset_128` as the control) in bench/c/cbench_memref.c; no row added.
  Instructions per call (mei Skylake-SP, `bench --compare --counters cbench_memref`, job `classed-slot-infra-7-prep`):

  | row | merge-base | branch | glibc |
  |---|---:|---:|---:|
  | memset_128 | 26.3 | 26.3 | 24.3-25.1 |
  | memset_144 | 30.3 | 37.4 | 32.4 |
  | memset_256 | 30.3 | 37.4 | 32.4 |

  +7.1 instructions (+23%) at 129-256: 16 `movups` on page 0 against the flag test plus 8 `vmovdqu` and
  `vzeroupper`. Timing, Liella ns/call medians, 4 alternating rounds per tree (`time.sh`, `--compare`):

  | row | N100 mb | N100 branch | N100 glibc dyn | Skylake-SP mb | Skylake-SP branch | Skylake-SP glibc dyn |
  |---|---:|---:|---:|---:|---:|---:|
  | memset_128 | 3.35 x4 | 3.35 x3, 3.56 x1 | 4.18-4.23 | pending | pending | pending |
  | memset_144 | 4.19 x4 | 4.19 x4 | 4.60-4.64 | pending | pending | pending |
  | memset_256 | 4.19 x4 | 3.78 x4 | 4.60-4.64 | pending | pending | pending |

  N100: no row slower; memset_256 is 10% faster with the SSE2 body (3.78 against 4.19 ns, every round), 144 equal.
  By instructions the branch is +7 at 129-256 on the AVX2 class and goes from below glibc to above it (37.4 against
  32.4). Skylake-SP timing: mei `classed-slot-infra-7-skx-2` (`~/bin/timing`, whole host; the first try,
  `classed-slot-infra-7-skx`, waited 66 min for the host to drain and was cancelled before it started, so it has no
  jobs.tsv row). Medians: `awk` the `memset_(128|144|256)` rows out of `~/logs/classed-slot-infra-7-skx-2.log`
  (column 4 = glibc dynamic, column 7 = Liella), per `=== mb|branch` round.

**INFORMATIONAL:** the slot tests check results, not which data word a page reads: both biasless data-word plants
(fold limit, set_mid) survive by result. A static check that page 1's RIP-relative displacements to out-of-page
symbols equal page 0's plus 4096 (objdump of the classed section) would catch them whatever the link layout.
Done by CLASSED-SLOT-XREF-CHECK (D-X.34, `cargo xtask classed-xref`, also in `static` and `so`); both plants are real
plants again (handoff/CLASSED-SLOT-XREF-CHECK.md).

**Exact next step:** read the Skylake-SP timing (above); if memset_144/256 is slower beyond noise there, the
coordinator rules on ruling #3434 (a per-function AVX2 129-256 body on page 0 without the swap, or land as is); then CLASSED-SLOT-SWAP-COST.
Verdict this run: INCOMPLETE only because the Skylake-SP timing verdict is pending; gate, plants, clippy and static
are green on the final code.

**Not to redo:** no-swap class costs, the fixed user cost, the syscall/VMA bound, the swap's cost (all -3..-5); the
N100 start-up timing (-6); the 129-256 counts above. Scripts: mei `~/work/classed-slot-infra-7/{prep.sh,time.sh}`,
staged tars there and on nozomi. Plants file: handoff/classed-slot-infra-plants.txt.

**Read:** D-4.53; docs/design/loadtime-entry.md 5.3-5.7.
