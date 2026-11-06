# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra-6` (continues -5/-4/-3/-2/-1), origin/next `c101c4c6` merged in (72360562;
one conflict, D-4.53 and next's D-4.54 both kept). Code commit `0e63bc1f`. Role shiki:~/w/sh1 (Haswell E5-2697 v3,
AVX2 class).
**Verdict: INCOMPLETE.** Gate RED: `test -p liella-boundary (exports interposed)` FAIL (the plain boundary run is ok;
the gate log has no failing test name: xtask affected prints none). Not diagnosed: the run hit its call budget. The
N100 timing job was still queued at hand-back. Plants, clippy, static and the kernel count are green.

**Decisions in force:** D-4.53 (amended this run: swap OFF by default), COMPAT-20, rulings #3138-#3141, #3297, #3434.

**Run -6 (this run), ruling #3434:**
* **Swap off by default.** `classed::SWAP_BY_DEFAULT = false`; `classed::swap_disabled(t.no_classed_swap)` =
  `no_classed_swap.unwrap_or(!SWAP_BY_DEFAULT)`. COMPAT-20's tunable keeps its name: unset or `=1` keeps page 0 on
  every class, `=0` turns the swap on. No new interface; the default now matches glibc's behaviour.
* **Tests both ways.** `conformance/c/s04_classed_memset.c`: default -> SSE2 page; `=0` -> AVX2 page on an AVX2 host;
  `=0` + `-AVX2` -> SSE2; `=1` -> SSE2; the ptrace guard and PR_SET_MDWE starts run with `=0`. Unit test
  `the_swap_is_off_unless_the_flag_is_zero`. `classed_slots.rs` calls `install_with` directly (unchanged).
* **Docs:** compat-flags COMPAT-20 row (also its glibc/Liella columns, which were swapped), tunables.md, config.rs
  table, design loadtime-entry.md 5.7, D-4.53 amendment (the -5 cost table, the -6 count, CLASSED-SLOT-SWAP-COST).
* **Gate** shiki `classed-slot-infra-6-gate` (affected --base origin/next). **Plants** shiki `classed-slot-infra-6-plants`
  rc 0: 12 CAUGHT, 0 WARN (3 new: swap-on-by-default (conformance), flag-ignored, swap-on-by-default-unit).
  clippy + static clean on the role before the gate.
* **Kernel instructions per start** (shiki `classed-slot-infra-6-kcount`, `perf stat -r 50` of `cbench_startup true`,
  8 rounds, order flipped, load ~26): merge-base 199,390, branch 192,560 (-6.8k, ranges disjoint: 191.2-194.4k vs
  197.3-202.0k), user 2253 -> 2261 (+8), faults 14 -> 12; swap on (`=0`) 282,894 kernel. As expected (-5: about -6k).
* **N100 timing** nozomi `classed-slot-infra-6-n100` (booking #72; `~/work/classed-slot-infra-6/run.sh`, 3 rounds,
  alternating, start-up rows + memset 16-128). Result: see the report json / hand-back.

**INFORMATIONAL:** with the swap off, the AVX2 class runs page 0's SSE2 `memset` body at 129-256 bytes (the merge-base
tested the D-4.25 AVX2 flag there and ran 32-byte blocks). No bench row covers 129-256, so neither the counts nor the
timing see it. CLASSED-SLOT-SWAP-COST (or a small follow-up) should add a `memset_192`-style row and measure it; if it is
slower, page 0 can keep a flag test at 129-256 until the swap is on by default.

**Exact next step:** first the red gate step: on shiki sh1 run the boundary tests with exports interposed (the
`affected` step `test -p liella-boundary (exports interposed)`; `cargo xtask affected --help` for the flag), read
the failing test, find whether the default flip causes it (a test that expects the swap at a real start?) and fix
the cause. Then read `nozomi:~/logs/classed-slot-infra-6-n100.log` (medians per row, mb vs branch; within noise =
done). Then CLASSED-SLOT-SWAP-COST.

**Not to redo:** no-swap class costs, the fixed user cost, the syscall/VMA bound, the swap's cost (all -3..-5).
Scripts: shiki `~/work/classed-slot-infra-6/{stage.sh,kcount.sh}`, nozomi `~/work/classed-slot-infra-6/run.sh`;
staged tars on both. Plants file: handoff/classed-slot-infra-plants.txt.

**Read:** D-4.53; docs/design/loadtime-entry.md 5.3-5.7.
