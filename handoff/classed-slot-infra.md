# CLASSED-SLOT-INFRA handoff (one page)

**State:** branch `task/classed-slot-infra` (cut from `task/loadtime-entry-design-2`, which is not on next yet), code
commit `bb24a548` (WIP), plus the hand-off commit that adds this file. Status: **INCOMPLETE. Not built and not tested.**
The dev check was queued on keke role r1. It had not started after about 25 minutes, because CI `ci-instr-*` and the
libm tier-3 gates held the cores.

**Done (code, unverified):**
* `csys/src/machine/fold.S`: memset is now a two-page classed section, built by the GAS macro
  `LIELLA_MEMSET_BODY bias, avx2`. Page 0 holds the SSE2 body at `memset`/`liella_machine_memset`. Page 1 holds the
  AVX2 body at the local `liella_machine_memset_avx2`. Every out-of-page reference is written XREF = `+\bias`. The
  129-256 flag test is gone from both pages (page 0 is SSE2 there, page 1 AVX2). The `classed_id` slot is at offset
  4032. The `.org` lines check the page size. The section keeps the name `.text.liella_machine_memset`, so the
  generated hot-text list still groups it. This is a local detail recorded in D-4.53; the design said
  `.text.liella_classed`.
* `crates/boundary/src/classed.rs` holds the class rule (`Features::avx2`, which includes the XCR0 check, and the
  `-AVX2` tunable), the pagemap guard, the `MREMAP_FIXED` move, and a `mincore` check after a failed move (one retry,
  then a fatal message). It records the outcome. `runtime.rs` calls it at step 4, right after `cpu::init_with`, in
  static programs only. Unit tests use a fake `SwapOps` to inject failures.
* The tunable `liella.compat.no_classed_swap=1` (COMPAT-20, reserved with `ops reserve`), in `crates/core/src/config.rs`,
  with a unit test.
* `PrctlInt::SetMdwe`/`GetMdwe` (65/66), `selftest::classed_install` and `selftest::set_mdwe_refuse_exec_gain`.
* `crates/boundary/tests/classed_slots.rs` runs each case in a forked child (one thread):
  * memset sweeps on the AVX2 page and on the forced SSE2 page;
  * COMPAT-20 keeps the SSE2 page;
  * the guard keeps SSE2 after a byte is written back through `/proc/self/mem`;
  * the swap under `PR_SET_MDWE`;
  * after the swap, the live page is still a file page.
  Each skip prints its reason.
* Docs: the D-4.53 row (`plan/07-decisions/s04-byte-arrays-and-strings.md`), the COMPAT-20 row
  (`docs/compat-flags.md`), and the debugging note as section 5.7 of the design doc. The plants are in
  `handoff/classed-slot-infra-plants.txt` (7 plants, not run): xref-bias, no-guard, guard-inverted, skip-mremap,
  no-mapped-check, no-xcr0, slot-write.

**Not done:**
* No build, clippy, static check or test has run.
* Jobs not run: the gate, the plants, the counts (`cbench_memref` on the AVX2 class and on Ivy Bridge), and the booked
  timing on Ivy Bridge and the N100.
* The start-up row with system calls counted, which the design names, has not been added.
* The design's static-export conformance test (default and `-AVX2`) and its `PTRACE_TRACEME` guard test are not
  written. The forked-child tests stand in for them, but no test yet covers the real start-up call.
* The `docs/tunables.md` row has not been checked.
* No differences-from-glibc row was added. The design says this is not an ABI difference, but the static check may
  want a row for COMPAT-20.

**Check these first:**
* `jz sym+4096` and `cmpq sym+4096(%rip)` assemble as intended (PLT32/PC32 with an addend; every target is hidden).
* `safe static liella_classed_page0: u8` declared on a text label is accepted.
* `hot_text_products` still finds `liella_machine_memset` in the hot group.
* The forked child does not allocate (stack buffers only).

**Exact next step:**
1. Read the dev log: `ssh claude@10.0.100.53 'grep -E "^error|test result|FAILED|panicked" ~/w/r1-classed-dev.log'`.
   The job is `classed-slot-infra-dev`; cancel it with `~/bin/kill-label classed-slot-infra-dev --kill`.
2. Fix any compile errors.
3. Run the plants: `cargo xtask plants handoff/classed-slot-infra-plants.txt --dir /dev/shm/claude/CLASSED-SLOT-INFRA/plants-1`.
4. Run the gate, the counts and the start-up row.

**Read:** `docs/design/loadtime-entry.md` section 4 (story 1) and sections 5.3-5.5; D-4.53; D-4.25.
