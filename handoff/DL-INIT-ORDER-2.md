# DL-INIT-ORDER-2: next merged into DL-INIT-ORDER (ld.rs conflict resolved), re-verified

State: branch `task/dl-init-order-2` (DL-INIT-ORDER's commits + merge 35f3e3238 of origin/next 7d31b0621, then this
note and the report json). Decisions in force: D-27.19 (DL-INIT-ORDER), D-27.18 (DL-IMAGE-READER-3's image reader).
Story handoff with the design, probe results and next steps: handoff/DL-INIT-ORDER.md (unchanged).

## The resolution (crates/boundary/src/ld.rs only; nothing else changed)
Three hunks, both sides kept:
* `run`: DL-IMAGE-READER-3's program name (its path for `ld.so PROGRAM`, else argv[0]) goes to `link`; DL-INIT-ORDER's
  initial stack pointer goes too, and the fini pass `link` returns is passed to `arch::enter(sp, entry, fini)`.
* `link` signature: `(prog, bias, page, name: &'static [u8], sp) -> usize` (next's `name` replaces the old `path`;
  `let _ = prog;` dropped as next did, `prog` is used by `image_view`).
* End of `link`: the init/fini hooks are read from every object's dynamic array BEFORE next's unmap loop
  (`maps[1..n]` file mappings and the program's `own` image copies); the hooks hold addresses only, so nothing
  points into the unmapped copies when `run_init` runs. `liella_ld::start::bias` (in `main`) is untouched.

## Jobs (keke, role n3; head 35f3e3238 for code)
* dl-init-order-2-gate: ~/work/DL-INIT-ORDER-2/gate.sh under ~/bin/gate: `cargo xtask clippy`, `cargo xtask static`,
  `cargo xtask affected --base origin/next --tier direct --files <every changed file but entry.S> --run`, `cargo xtask so`,
  `cargo test -p liella-ld --test init_order --test reloc --test ldso_link --test ldso_run`. entry.S is left out of
  `--files` because it defines `_start` and escalates to FULL; its change is in the LIELLA_LD branch only, which the
  four ld.so suites run. Result: rc 0; affected: suites 15 (15 ok, 0 failed), tests 12161 passed, 0 failed,
  1353 ignored; conformance 127 passed, 0 failed; ld.so suites init_order 7, reloc 9, ldso_link 7, ldso_run 13 passed.
* dl-init-order-2-plants: `cargo xtask plants handoff/DL-INIT-ORDER-plants.txt` (DL-INIT-ORDER's file, unchanged; all
  six patterns still apply after the merge): rc 0,
  6 plants: 6 CAUGHT, 0 SURVIVED, 0 EQUIVALENT, 0 WARN-RC-ONLY, 0 WARN-NO-MSG (one WARN-DEV-SHM note: the copy is
  under /dev/shm; none of these tests mounts its own /dev/shm).
* dl-init-order-2-counts (branch, role) / dl-init-order-2-counts-next (git archive of origin/next 7d31b0621 in
  ~/work/DL-INIT-ORDER-2/next): `cargo xtask bench --compare --counters cbench_startup`. Both rc 0. Liella instr/op, branch vs next:
  instr_c 2146 vs 2146, setloc 68079 vs 68074, printf 71875 vs 71875, wctype 69099 vs 69103 (glibc's rows moved by up to
  8 between the same two runs); kinstr_c 160626 vs 181801 (kernel, noisy: glibc 714236 vs 716222). These programs are
  static and never enter ld.so; no row moved beyond run-to-run noise.

## Open / next step
None for this story. DL-INIT-ORDER's next steps stand (handoff/DL-INIT-ORDER.md "Not done / next step").
