# DL-MAP-PROBE (-2): DL-MAP's open items (#4186, #4188, #4189)

**State:** branch `task/dl-map-probe-2` (continues `task/dl-map-probe`; origin/next merged at 070c02a1f, ruling #4253),
role keke:~/w/n2. Commits of this chain: b84d12d67 (bound removed, D-27.15 amended, tests), 4050f6302 (test fixes),
8b8a8d071 + edfa3414b (moved 2 MiB ET_DYN corpus object, -Bsymbolic), a07ef4b75 (ld.so RELRO bias fix). Decisions in force: D-27.15 (amended in run 1). `cargo xtask codemap`
after the merge wrote nothing (no codemap change to commit).

## 1. PT_LOAD bound (#4186): done
* Black-box probe (run 1, kanon jobs `dl-map-probe-ref`, `dl-map-probe-ref2`, rc 0): a PIE and a shared object
  rewritten to 64, 65, 66, 100, 128, 200, 300, 500, 1000, 1100, 4000, 10000 and 30000 PT_LOADs all load in the
  reference loader (glibc 2.39) through `ld.so PROGRAM` and `dlopen`. So `LOADS_MAX` / `TooManySegments` are
  removed; the only bound is the ELF reader's e_phnum check (PN_XNUM refused). D-27.15 amended, no differences row.
* Tests: 72-PT_LOAD synthetic object (`map_malformed_corpus_refused`), ld.so runs a PIE with 300 PT_LOADs
  (`map_proc_self_maps_of_loaded_programs`). Plant `loads-bound` re-adds a 64 bound.
* INFORMATIONAL (#4257, kernel): direct execve of the rewritten programs fails (64: SIGSEGV, 65+: ENOEXEC); that is
  the kernel's program-header handling, out of scope (ruling #4186: the bound follows the reference loader via ld.so).

## 2. Fuzz (#4188): done, no finding
`kanon:dl-map-probe-fuzz3` (run 1, 60 min on the final map.rs): rc 0, 227,598,719 execs, cov 145, ft 494, corpus 316,
no crash or artifact.

## 3. Movable object, large p_align, first segment not at 0 (#4189): done
GNU ld 2.42 gives ET_EXEC for `-pie -Wl,-Ttext-segment=...` and has no ELF `--image-base`; `-shared` keeps ET_DYN
(keke probe `dl-map-probe-2-ldprobe`). New corpus object `MOVED_2M` in crates/ld/tests/map.rs: `-fPIC -shared
-Wl,-e,_start -Wl,-Ttext-segment=0x600000` with 2 MiB page sizes; the test asserts it is ET_DYN with its first PT_LOAD at
0x600000, maps it with the model mapper (`map_model_objects_from_the_linker`) and runs it as `ld.so PROG` with the
2 MiB alignment and /proc/self/maps checks (`map_proc_self_maps_of_loaded_programs`). Plant `bias-ignores-first`
(bias = reservation base, ignoring the first segment's address) is what only this object catches: every other corpus
object starts at 0. 0x600000 is a multiple of 2 MiB, so base alignment and bias alignment coincide for it; the
alignment rule itself is covered by `align2m`.
* The object needs `-Wl,-Bsymbolic`: without it its GOT holds GLOB_DAT relocations, which this ld.so build refuses
  (only RELATIVE until DL-8/DL-10).
* **Bug found and fixed (a07ef4b75):** ld.so protected RELRO through `start::relro`, which takes the bias from PT_PHDR
  and assumes 0 without one (the static start-up rule). A program ld.so maps itself with no PT_PHDR (this `-shared`
  object run as `ld.so PROG`) got mprotect at its link address and was refused ("cannot protect the program's RELRO
  segment"). New `start::relro_at(words, phnum, bias, page)` takes the bias ld.so holds; `relro` calls it. Unit check
  in start.rs; plant `relro-bias-phdr` (back to `relro`) must be caught by `map_proc_self_maps_of_loaded_programs`.
* INFORMATIONAL: the kernel-started path in ld.rs (step 6, PT_INTERP) still derives the bias from PT_PHDR (0 without
  one); a PT_INTERP program without PT_PHDR would be relocated and protected at bias 0. Not exercised by any test;
  a later DL story should take the bias from AT_PHDR and the segment holding e_phoff, as map.rs's planner does.

## Jobs (keke unless named)
* `dl-map-probe-2-lint` (inherited head + merge: codemap, clippy, static): rc 0.
* Final head (code 4e453352e): `dl-map-probe-2-gate4` (`cargo xtask so`, clippy, static, `affected --base origin/next
  --run`) and `dl-map-probe-2-plants4` (handoff/DL-MAP-PROBE-2-plants.txt, 3 plants) were RUNNING at hand-back (PENDING).
* plants3 (head a07ef4b75): loads-bound CAUGHT, bias-ignores-first CAUGHT, relro-bias-phdr SURVIVED only because the
  tool reused the baseline products (ld.so not rebuilt for a crates/boundary mutant); plants4's command for it runs
  `cargo xtask so` first. If plants4 still shows it surviving, check the log's "build:" line before anything else.
* SUPERSEDED: gate (stopped, stale), plants/plants2 (baseline red: GLOB_DAT, then the RELRO bug), gate2 (the RELRO
  bug), gate3 (clippy: unused closure), plants3 (product not rebuilt for the ld.rs plant).
* SUPERSEDED from run 1: kanon gate/gate2/gate3/gate4/plants (older heads, stale ld.so, or started while BUSY).

## Exact next step
None for the story once gate and plants are green: it lands. Read: D-27.15, handoff/DL-MAP.md.
