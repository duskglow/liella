# DL-LIBC-START-BIAS: libc's own start takes the load bias without relying on PT_PHDR

State: INCOMPLETE, blocked on a prerequisite. Branch `task/dl-libc-start-bias`, cut from origin/next f1f5358d5.
The only commit on it is this note and its report. No code changed.

## Why it stopped

The brief says to share `liella_ld::start::bias`, which DL-IMAGE-READER wrote ("one implementation, not a copy").
That function is **not on origin/next at f1f5358d5**. `git grep "fn bias" origin/next -- crates/` finds only a test
helper (`crates/ld/tests/reloc.rs`) and `liella_rt::unwind::bias_of`. None of origin/task/dl-image-reader,
-2 or -3 is an ancestor of origin/next. `ops log DL-IMAGE-READER-3` says: finished, verified, queued_ci
2026-11-10T23:21:57Z, not landed. This story is not stacked on it, and the brief allows no merge.

Writing the computation again in `liella_rt::start` would make a second implementation that conflicts with
DL-IMAGE-READER-3 when it lands. That breaks the brief's "share the function" rule, so I did not do it.

## What is in force on next (read, not changed)

* `liella_rt::start::program(words, phnum, phdr)` (crates/rt/src/start.rs:47) takes the bias from PT_PHDR only.
  Without PT_PHDR the bias is 0.
* libc's start uses it in two places. One is `initial_thread` (crates/boundary/src/runtime.rs, `start::program`
  about line 87). The static-PIE path there fixes the bias from `relocate_self`. The non-PIE path dies when bias != 0.
  The other is `process::exe_bias` (crates/boundary/src/process.rs:53). It uses `EXE_BIAS` (D-27.12, static PIE
  only), else `start::program(..).bias`.
* ld.so's mapper, `liella_ld::map::plan` (crates/ld/src/map.rs:247-262), already has the fallback for an object it
  maps from a file: the first PT_LOAD that holds `e_phoff..e_phoff+phnum*56`. This needs `e_phoff` from the file
  header.

## Open question for the next run

From memory alone, libc's start knows AT_PHDR, AT_PHNUM and AT_ENTRY, but not `e_phoff`. Read DL-IMAGE-READER-3's
`liella_ld::start::bias` once it lands to see what it needs as input. If it needs the file header, the libc side
must find the header in memory (for example, the ELF header that the first PT_LOAD with offset 0 maps). That would
be a new design point for D-27.21.

## Exact next step

When DL-IMAGE-READER-3 is on origin/next, re-cut or re-sync this branch. Then:
1. Have `liella_rt::start::program` (or both libc call sites) call the shared bias function.
2. Add the test programs linked without PT_PHDR (a PIE and an ET_EXEC; ld `-z noseparate-code` plus a linker
   script with PHDRS lacking PHDR). Run them under Liella's ld.so and as a static program.
3. Plant: the PT_PHDR-only form, with `msg:`.
4. Run cbench_startup counters against the baseline below.

## Baseline (origin/next f1f5358d5, keke:~/w/n1)

Job `DL-LIBC-START-BIAS-startup-base`: `cargo xtask bench --compare --counters cbench_startup`. The result is in
handoff/reports/DL-LIBC-START-BIAS.json (`informational`) and in the hand-back.

## Plan rows to read

D-27.10 (static-PIE start), D-27.12 (EXE_BIAS, the one bias source), D-27.15. D-27.21 is reserved for this story
and still unused.
