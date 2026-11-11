# DL-LIBC-START-BIAS: libc's own start takes the load bias without relying on PT_PHDR

State: INCOMPLETE (run 2, 2026-11-11). Blocked on a ruling: the branch cannot reach the code it must share without
a merge, and the brief does not allow one. Branch `task/dl-libc-start-bias`, cut from origin/next f1f5358d5. No code
changed. The branch holds only this note and its report, all from this story.

## Why it stopped (run 2)

* The prerequisite has now landed. `origin/task/dl-image-reader-3` is an ancestor of origin/next 7d31b0621, and
  `git grep "fn bias" origin/next -- crates/` finds
  `crates/ld/src/start.rs:190: pub fn bias(words, phnum, phdr, page, header: impl FnOnce(usize) -> [u8; 64]) -> Option<usize>`.
* This branch is still based on f1f5358d5. It is 38 commits behind next and does not contain that function. It has
  diverged from next by one commit (run 1's note), so it cannot fast-forward.
* thread_common.txt says "No merge/rebase unless your brief allows exactly which". This brief allows neither. A
  re-cut would need a force-push, and the pre-push hook refuses those. I did not merge, rebase or re-cut. I also did
  not write code against a function my tree does not have, because it could not be built or tested.

## RULING NEEDED

Allow `git merge --no-edit origin/next` into `task/dl-libc-start-bias`. The push is then a fast-forward, and the only
commits being merged on top of next are this story's own handoff notes. The other option is to re-create the branch
from origin/next before the next run. Recommendation: allow the merge. It is one command and needs no force-push.

## Design notes for the next run (from the signature only; the body was not read)

* The shared function lives in `liella_ld` (ld.so's crate). libc's start is in `liella_boundary`, which uses
  `liella_rt::start::program`. To keep one implementation, put `bias` where both crates can use it, most likely in
  `liella_rt::start`, with `liella_ld::start::bias` re-exporting it or calling it. Do not copy it. First check
  whether `liella_boundary` may depend on `liella_ld` at all.
* Its `header` closure reads the 64-byte ELF header at an address. In libc's start this is a read of mapped memory
  through `conv::words_at` / `bytes_at` (a `// UNSAFE(slice)` site in boundary, ⚑ KERN: the first PT_LOAD with
  offset 0 maps the header). `page` comes from AT_PAGESZ.
* Call sites: `runtime.rs` `initial_thread` (the non-PIE path dies when bias != 0, so an ET_EXEC must keep giving
  0, and a dynamic PIE goes through ld.so), and `process::exe_bias` (process.rs:49-54; EXE_BIAS for a static PIE,
  else `start::program(..).bias`). Keep the PT_PHDR path as the fast case, so that `cbench_startup` does not move.
  Run the fallback only when PT_PHDR is absent.
* Write a one-line D-27.21 row only if a rule changes, with `cargo xtask records new decision`.

## Exact next step

After the ruling: merge origin/next, then do brief steps 1 to 4. Those are: share `bias`; add the no-PT_PHDR PIE
and ET_EXEC test programs, run under Liella's ld.so and as a static program; a plant that restores the PT_PHDR-only
form, with `msg:`; and `cargo xtask bench --compare --counters cbench_startup` against the baseline below.

## Baseline (run 1, origin/next f1f5358d5, keke)

Job `DL-LIBC-START-BIAS-startup-base` (rc 0): startup_instr_c 2146, startup_instr_setloc 68079,
startup_instr_printf 71876, startup_instr_wctype 69106 (Liella instructions per run). Next has moved 38 commits
since then, so take a fresh baseline on the merged base.

## Plan rows to read

D-27.10 (the static-PIE start), D-27.12 (EXE_BIAS, the one source of the bias), D-27.15. D-27.21 is reserved for
this story and still unused.
