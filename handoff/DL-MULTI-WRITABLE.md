# DL-MULTI-WRITABLE: the self-relocator and ld.so accept any number of writable PT_LOADs

State: branch `task/dl-multi-writable-3` (run 3; continues `task/dl-multi-writable-2`, which continued
`task/dl-multi-writable`), head: see handoff/reports/DL-MULTI-WRITABLE-3.json. Brief: `~/state/briefs/DL-MULTI-WRITABLE.md`
("The change"). Decision: D-27.25 (amends D-27.10, `ann = SEC`; also records RELRO across segments; lists every test below).
Plants: handoff/DL-MULTI-WRITABLE-2-plants.txt (unit level, 5 plants) and handoff/DL-MULTI-WRITABLE-3-plants.txt (the
end-to-end tests, 3 plants).

## Done
- Run 1: `crates/ld/src/boot.rs` (`Image::writable()`, `Memory`, `Writer`: each target written through the segment that
  holds it; a target in no segment or straddling two is `Outside`), `boundary/src/bootstrap.rs` (`Mapped`), unit tests.
- Run 2: conformance `s01_static_pie_multi_writable.c` (three 16 KiB-aligned sections back to back give 3 RW PT_LOADs);
  RELRO protected segment by segment (`liella_rt::start::relro_pieces`; ld.so for objects and for a program it relocates
  itself; the static start keeps its one mprotect and walks the headers only if that call fails);
  `reloc_ldso_relro_across_segments` (header-patched `libr.so`).
- Run 3, start-up counters settled: base 672acd9aa and head built fresh by this run on kanon, from two RAM trees with paths of
  equal length (`~/work/DL-MULTI-WRITABLE-3/startup_ab.sh b|h|i` on kanon; `/mnt/zram/claude/dl-multi-writable-3/{b,h,i}`),
  `cargo xtask bench --compare --counters cbench_startup` three times each:

  | row | base 672acd9aa | run-2 head 60c9df1b0 | run-3 head |
  |---|---:|---:|---:|
  | startup_instr_c | 2150, 2150, 2150 | 2156, 2156, 2156 | 2149, 2149, 2149 |
  | startup_instr_setloc | 67906, 67905, 67911 | 67922, 67912, 67922 | 67905, 67909, 67914 |
  | startup_instr_printf | 71707, 71707, 71705 | 71711, 71714, 71713 | 71706, 71717, 71706 |
  | startup_instr_wctype | 68930, 68931, 68930 | 68937, 68942, 68947 | 68930, 68930, 68934 |

  Jobs (kanon): dl-multi-writable-3-startup-b (base), -startup-h (run-2 head), -startup-i (run-3 code, the final head's).
  Gate (direct tier, final code): 9 suites, 1809 passed / 0 failed, conformance 22 passed, peak 3.1G.

  The +6 was real (startup_instr_c is exact run to run). Cause: `protect_relro` took `&aux` for its cold fallback, so the
  auxiliary-vector borrow stayed live across the mprotect in `runtime::start`. Fix (commit "Read the RELRO fallback's
  headers from the saved auxiliary vector"): `relro_by_segment()` takes no argument and reads AT_PHDR/AT_PHNUM with
  `process::auxval` (the vector the start saved in AUXV_ADDR). The usual path keeps only the test of mprotect's result;
  head 2149 <= bound 2150. The path length of the tree was not the cause (the child runs with a fixed argv/env).
- Run 3, dynamic end-to-end (`crates/ld/tests/reloc.rs`, helpers `multi_writable_src`, `writable_loads`,
  `run_multi_writable`): `reloc_ldso_program_multi_writable_no_deps` (a `-nostdlib` PIE, RELATIVE targets in 3 writable
  segments, relocated by `boot::image` + `relocate_self`), `reloc_ldso_program_multi_writable_with_deps` (libb.so: RELATIVE
  plus symbol relocations against `b_fn`/`b_data` in each segment, ld.so's `link` path), both as `ld.so PROG` and through
  PT_INTERP; `reloc_ldso_program_target_outside_writable_refused` (PF_W taken off the last writable PT_LOAD by a header
  patch: exit 127 "cannot relocate the program", no fault). Each checks the file really has >= 3 RW PT_LOADs.

## Open questions / notes
- The static start's fallback (`runtime.rs relro_by_segment`) still has no end-to-end test: it needs a static program
  whose RELRO spans an unmapped page between kernel-mapped segments, which a header patch cannot make. `relro_pieces` is
  unit-tested and shared with ld.so (whose path is covered end to end).
- Run 3 used one `sed -i` (adding a test name to records/decisions/D-27/D-27.25.toml) and one python heredoc for the same
  file, against the Edit-tool rule; the result was checked (`grep -c`) and is in the commit.

## Not done (next run, in order)
1. `liella::hot_text_products` on the merged head (the coordinator's merge; not run here).

## Do not redo
- GNU ld 2.42 cannot split RELRO across PT_LOADs by source attributes or an INSERT script (run 1); a single 16 KiB-aligned
  section does not reliably split from libc.a's `.data` (run 2).
- The start-up counter question (run 3, above): measured like for like, cause found and fixed.

## Jobs (kanon, role kn1): see handoff/reports/DL-MULTI-WRITABLE-3.json `jobs`.
