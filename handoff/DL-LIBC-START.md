# DL-LIBC-START: the dynamic path of `__libc_start_main` (phase 14 DL-14)

**State:** branch `task/dl-libc-start` (on `task/dl-libc-early-2`'s head 47fa75c1d; DL-LIBC-EARLY not landed,
EARLY-4 running), role mei:~/w/s3. INCOMPLETE (context checkpoint): code, tests, D-27.24 and plants file written and
committed; **`cargo xtask so` fails** (below), so no product was built and no new test has run yet.

**Decisions in force:** D-27.24 (this story, records/decisions/D-27/D-27.24.toml), D-27.22, D-27.19/.20, D-10.5.
Plan rows: plan/06-phases/17-phase-14-dynamic-linking.md DL-14; plan/ph14/design.md §14.

## What was done (commits on the branch)
* `liella_rt::start::private::StartInfo` + `init`, `init_array`, `init_array_len` (the program's hooks); ld.so's
  `run_init` fills them from `hooks[0]` (crates/boundary/src/ld.rs).
* crates/boundary/src/runtime.rs: `early_init` opens the std fds in secure mode, stores the hooks, sets `EARLY_DONE`
  last. `start` now does the `ARCH_GET_FS` test FIRST (moved up from after the auxv count: same instructions for a
  static start, reordered) and calls `start_under_liella_ld` when FS is set and `EARLY_DONE` is set: skips steps 1-4,
  keeps set_tid_address / forbid / fpu_control / perturb / nscache / init_initial(false) / cancel, registers
  `rtld_fini` and `fini`, runs `init(argc,argv,envp)` or DT_INIT + DT_INIT_ARRAY, then main.
* Tests: crates/ld/tests/reloc.rs `reloc_libc_start_hello_world` (glibc oracle output; liella-crt1.o, host start
  files via `-nodefaultlibs`, old-style `_start` with init/fini; kernel-started and `ld.so PROGRAM`).
  crates/boundary/tests/products/mod.rs `conformance_dynamic` now also links each program against a by-path libc
  copy with Liella's ld.so and runs it; the #814 skip in s10_math_symbols.rs `sym___fpu_control` is removed.
* Plants: handoff/DL-LIBC-START-plants.txt (early-skip removed, rtld_fini not registered, program init not run).

## Jobs (mei)
* `dl-libc-start-step1` rc 1: clippy (arithmetic lints in products/mod.rs), fixed.
* `dl-libc-start-step2` rc 1: clippy green; records check: D-27.24 body had newlines, fixed.
* `dl-libc-start-step3` rc 1: clippy, records check, static passed; **`cargo xtask so` failed**:
  `start-up state: want one .data/.bss section each, got: 15liella_boundary3sys12SYSINFO_EHDR (0 sections)`.
  `readelf -SW target/liella/libc.a` shows no section naming SYSINFO_EHDR. Not diagnosed. Suspects: the moved
  `ARCH_GET_FS` test or the new path changing how LTO keeps `sys::SYSINFO_EHDR` (inlined into one user and
  internalised/merged?). Check first: does origin/task/dl-libc-early-2's head (47fa75c1d) pass `xtask so` in this
  role (it gated green on kanon)? If yes, bisect my three runtime.rs pieces (FS move; `start_under_liella_ld`;
  early_init additions). Do NOT edit coldbss.rs's key list to make it pass (ALWAYS FORWARD).

## Next step
1. Fix the `xtask so` failure (above). 2. Run ~/work/DL-LIBC-START/step1.sh on mei (clippy, records, static, so, the
two tests). 3. Plants via `cargo xtask plants handoff/DL-LIBC-START-plants.txt --dir /mnt/zram/claude/DL-LIBC-START/plants-1`.
4. `affected --run --base origin/next`, hot_text_products, cbench_startup counters head vs next (static unchanged).
Left for DL-LIBC-START-2: running `native_dynamic` in xtask/src/native_examples.rs (link a by-path variant with
Liella's interpreter, compare with EXPECTED) and a wider dynamic conformance subset (only fpu_control_* now).
Records: D-10.5's body still says the dynamic run is a skip; correct it (or `amends`) once the test is green.
