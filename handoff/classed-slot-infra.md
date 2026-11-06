# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra-2` (continues `task/classed-slot-infra`; cut from
`task/loadtime-entry-design-2`, not on next yet). Code head `51cb820c` plus the plants-anchor and
hand-off commits. Role mei:~/w/s4 (Skylake-SP, AVX2 class).

**Decisions in force:** D-4.53 (classed entries, M6; section name `.text.liella_machine_memset` kept until
the next hot-text regen, ruling #3274), COMPAT-20 (`liella.compat.no_classed_swap=1`), rulings #3138-#3141.

**Run -2 (this run):**
* The -1 dev job (keke `classed-slot-infra-dev`) had failed at clippy (two `as` conversions). Fixed, plus:
  io::Error comparison in the guard test, unchecked arithmetic in the unit fake, a `Tunables` literal in
  `s01_startup_clauses.rs`, `liella_classed_id` missing from `liella_csys.h` (K5), `eprintln!` in raw-fork
  children (fork-child-alloc; now one raw `write`), and `csys_symbols` (xtask/src/affected.rs), which called
  fold.S unmappable because of the macro and the slot `#define`s. It now reads past a `.macro` body with no
  global declaration and only `.L` labels, and a `#define UPPER <number>`. Unit cases added.
* New `conformance/c/s04_classed_memset.c` (the design's export test, guard test, MDWE test): re-executes
  itself five times and reads the live page from /proc/self/maps (a one-page mapping one page further into
  the file, with a hole after it). On the AVX2 class: default AVX2, `-AVX2` SSE2, COMPAT-20 SSE2,
  PTRACE_TRACEME with one word written back SSE2, PR_SET_MDWE before execve AVX2. All pass, and memset is
  correct in each start.
* New bench row `cbench_startup/startup_kinstr_c`: exec to exit, user and kernel instructions together, so
  the swap's system calls count. strace of one start on the AVX2 class: `openat /proc/self/pagemap`,
  `pread64` 16 bytes, `close`, `mremap(0x411000 -> 0x410000)`, which is the design's 4 calls.
* The dev check (dev5) passed clippy, static, the classed unit tests, classed_slots (5 run, none skipped:
  swap, forced SSE2, COMPAT-20, guard via /proc/self/mem, MDWE), small_copy (9) and config.
* Plants (`classed-slot-infra-2-plants3`): 7 of 7 CAUGHT. The first xref-bias plant was a biasless read of the fold
  limit, and it SURVIVED because the limit only picks between paths with the same result. It is now a
  biasless jump to the AVX2 fill entry.
* Gate `classed-slot-infra-2-gate`: escalated to FULL by itself (xtask/ and csys header changed). It was still
  running at hand-back.
* `docs/tunables.md` now has a row for the COMPAT-20 tunable. No differences row was added: the compat id
  static check passed without one, and the design says this is not an ABI difference.

**Counts, base = origin/next (mei, `classed-slot-infra-2-count-base`), Liella instructions per call (glibc):**
AVX2 class memset_128 27.3 (24.3), _144 30.3 (32.4), _256 30.4 (32.4), _16 20.3, _64 20.4, _4k 26.8;
SSE2 class (-AVX2 both sides) _144 39.4 (45.4), _256 39.4 (52.4). startup_instr_c 2154 (121383);
startup_kinstr_c 166535 AVX2 / 161295 SSE2 (the kernel count varies by a few percent between runs).
Branch counts: job `classed-slot-infra-2-count-branch` (it runs after the gate in the same shell);
results are in the report, if it finished before hand-back.

**Open / next step:**
1. Read `~/logs/classed-slot-infra-2-{gate,plants2,count-branch}.log` on mei. All 7 plants must be CAUGHT.
   Compare count-branch with the base above. Expected: -2 at 129-256 on the AVX2 class, everything
   else the same, and startup_instr_c up by about 100 user instructions on AVX2 only.
2. The start-up bound (ruling #3139): startup_kinstr_c on the AVX2 class, branch against base. The
   extra cost should be just the 4 calls. If not, DO NOT LAND with the numbers.
3. The booked timing of memset and the start-up on Ivy Bridge and the N100 (has not run).

**Not to redo:** the export, guard and MDWE tests are already in the conformance program. Don't add a
selftest symbol: libc.a localizes hidden symbols, so a C program cannot call `liella_classed_id`.

**Read:** docs/design/loadtime-entry.md 4 and 5.3-5.7; D-4.53; handoff/classed-slot-infra-plants.txt.
