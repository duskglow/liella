# DL-LIBC-START-BIAS-3: libc's executable bias without PT_PHDR

**State:** branch `task/dl-libc-start-bias-3` from origin/next 7d31b0621; code head 6ac4b9a8f; role mei:~/w/s1.
INCOMPLETE at the call budget (70): the code and test are done, and the jobs on the final head were still running at
hand-back. The start-up counters have not been measured.

**Decisions in force:** D-27.21 (new, amends D-27.12): `process::exe_bias` uses `liella_rt::start::bias`
(D-27.18's method). D-27.18 and D-27.12 still hold.

## What was done
* `bias` moved from `liella_ld::start` to `liella_rt::start` (one implementation); `liella_ld::start` re-exports it
  (`pub use`), so ld.rs's call is unchanged. New guard: no header read when `phnum == 0 || phdr == 0`.
* `process::exe_bias` (used by dl_iterate_phdr, _dl_find_object, the %n check and the backtrace): without the static-PIE
  bias it now calls `liella_rt::start::bias(..., PAGE, exe_header)`, falling back to 0. `exe_header` lives in runtime.rs
  (process.rs may not use slice-category unsafe) and exists only with the `runtime` feature. Without that feature
  (fuzz and test builds) a stub returns zeros. Start-up doesn't call any of this: `initial_thread` still uses
  `start::program`'s one pass.
* Test `reloc_libc_bias_without_pt_phdr` (crates/ld/tests/reloc.rs, s27_1_c_8): a PIE and an ET_EXEC linked against
  libc.so.6 with PT_PHDR set to PT_NULL, run kernel-started and as `ld.so PROGRAM`, plus a static ET_EXEC (which has
  no PT_PHDR). Each checks dl_iterate_phdr's dlpi_addr against AT_ENTRY - e_entry, and a read-only `%n` under
  `__printf_chk`. libc.so.6 is named by path through a copy whose DT_SONAME is retagged DT_CHECKSUM, because ld.so
  has no library search yet.

## Results
* Test: passed on 5f053afca's tree (`dl-libc-start-bias-3-test2`).
* Plants (`dl-libc-start-bias-3-plants`, on 5f053afca, superseded): phdr-only CAUGHT. That run was stopped before
  header-off ran. `plants3` runs both on the final head.
* Probe on next's products: the static ET_EXEC derives bias 0 with or without PT_PHDR (correct). Programs linked
  dynamically against libc.so.6 by soname fail in ld.so with "library search is not available".
* Superseded (older heads, or stopped): clippy, static (unsafe-audit: slice in process.rs), static2 (fuzz-check: no
  `runtime` module without the feature), startup, startup2, gate, gate2, plants, plants2, test.
* startup-base failed: `cargo xtask bench --counters cbench_startup` is the wrong syntax ("unknown argument").

## Next step
1. Check `~/bin/jobwait dl-libc-start-bias-3-clippy3 dl-libc-start-bias-3-static3 dl-libc-start-bias-3-gate3
   dl-libc-start-bias-3-plants3` on mei.
2. Start-up counters: find the right `cargo xtask bench` form for cbench_startup (docs/benchmarks.md
   "### cbench_startup"). Run it on the head and on the base. ~/work/DL-LIBC-START-BIAS-3/base.sh rebuilds the base
   in RAM from the role with this story's diff reversed; fix its last line. No start-up code changed, so the counts
   should not move.

Plants file: handoff/DL-LIBC-START-BIAS-3-plants.txt. Read: D-27.18, D-27.12, D-27.21.
