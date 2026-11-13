# DL-DLSYM (DL-16b): handoff (run 3, DL-DLSYM-3)

**State.** Branch `task/dl-dlsym-3` (continues `task/dl-dlsym-2`, `task/dl-dlsym`); `origin/next` d483eefbe merged
(the base DL-DLOPEN-LOAD-4 landed there; rulings #5331, #5350). Role shiki:~/w/sh1. Full report of this run:
handoff/DL-DLSYM-3-report.md; earlier: handoff/DL-DLSYM-2-report.md.

**Done.**
- Run 1: `dlsym`/`dlvsym` in `ld.so` (crates/boundary/src/ld/dlopen/dlsym.rs): scopes, RTLD_NEXT, RTLD_DEFAULT
  (+ caller's local scope, #5127), DEEPBIND, unique table with pinning, IFUNC and start-time STT_TLS answers, handle
  validation, texts; exports in libc.so.6 and libc.a (#5264).
- Run 2: `dlvsym` refuses objects without version tables (pending DL-DLSYM-2-2 amends D-27.11); handles.c and
  s27_dlopen_failures pass; probe P-14 (capacity) with a differences row; 12 plants CAUGHT.
- Run 3: merged next; placed the start-up layout red of run 2 (ruling #5350). It is not this branch's start-up
  code: the `tool` probe's extra fault is `liella_legacy::global_locale::setlocale_locked`, a tool-path function
  that next's tracked (train-owned) `crates/libc-so/hot-text.list` does not list, so it lies in cold text and
  straddles a page at `cold 0x2000` once this branch's library code shifts it (fast profile only; release passes).
  A regenerated list (`cargo xtask hot-lists`) gives tool 51 at every pad, but breaks two zero-fault cluster tests
  on origin/next alone too, so it is no way out today. Numbers and method in the report.

**Decisions in force.** D-27.11 (amended by pending DL-DLSYM-2-2), D-27.32, D-27.33, D-27.35; rulings #5127, #5264,
#5331, #5350, #5351 (512-object refusal kept with its differences row); pending DL-DLSYM-2-1.

**Open.** RULING NEEDED (report): hold as LAYOUT-JITTER-CLASS until trace-at-build (or consistent train-owned
lists) lands, then land unchanged. The capacity story (growable object table, #5351) is briefed separately.
`pin_unique` is observable only with `dlclose` (DL-17).

**Do not redo:** the scope design (P-5); the version rule (dynlink.md row, probe-settled); the layout placement above
(no code move, bound change or hand edit of a generated list: the cause is next's stale list).

**Exact next step.** When trace-at-build has landed: merge origin/next, rerun
`init_data_clusters_hold_at_every_text_phase` (fast and release) and the direct gate, then land. Then DL-16c (lock-free readers; dlsym takes the writer lock now), DL-17 (`dlclose`, makes
the unique pin testable), the capacity story.

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §2, §4, §8; D-27.11, D-27.32; dynlink.md P-3..P-6, P-14.
