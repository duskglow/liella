# DL-DLSYM (DL-16b): handoff (run 4, DL-DLSYM-4)

**State.** Branch `task/dl-dlsym-4` (continues `task/dl-dlsym-3`, `-2`, `task/dl-dlsym`); `origin/next` d53c0d551
merged (LAYOUT-TRACE-DEFAULT-4 landed there: the build groups from a trace of itself by default). Role shiki:~/w/sh2.
Full report of this run: handoff/DL-DLSYM-4-report.md; earlier: handoff/DL-DLSYM-3-report.md, -2-report.md.

**Done.**
- Run 1: `dlsym`/`dlvsym` in `ld.so` (crates/boundary/src/ld/dlopen/dlsym.rs): scopes, RTLD_NEXT, RTLD_DEFAULT
  (+ caller's local scope, #5127), DEEPBIND, unique table with pinning, IFUNC and start-time STT_TLS answers, handle
  validation, texts; exports in libc.so.6 and libc.a (#5264).
- Run 2: `dlvsym` refuses objects without version tables (pending DL-DLSYM-2-2 amends D-27.11); handles.c and
  s27_dlopen_failures pass; probe P-14 (capacity) with a differences row; 12 plants CAUGHT.
- Run 3: placed the start-up layout red of run 2 (ruling #5350): next's stale tracked hot-text list left
  `setlocale_locked` in cold text, not this branch's start-up code.
- Run 4: merged next (DL-16d's static-TLS placement next to this branch's unique table in `dlopen.rs`: both kept;
  next's `thread_walk` now uses this branch's `syms_of`). With trace-at-build the layout test passes in both
  profiles (fast: tool 50, release: tool 49 at every pad). The FULL gate's one red was next's
  `s27_dynlink_helper_dlsym`, still marked `PENDING-LIELLA: DL-16b` although it now passes; the mark is dropped
  (conformance 134/0). Details in the report.

**Decisions in force.** D-27.11 (amended by pending DL-DLSYM-2-2), D-27.32, D-27.33, D-27.35; rulings #5127, #5264,
#5331, #5350, #5351 (512-object refusal kept with its differences row); pending DL-DLSYM-2-1.

**Open.** The capacity story (growable object table, #5351) is briefed separately. `pin_unique` is observable only
with `dlclose` (DL-17). `dlsym` takes the writer lock (DL-16c makes readers lock-free).

**Do not redo:** the scope design (P-5); the version rule (dynlink.md row, probe-settled); the layout placement of
run 3; the plants (12/12 CAUGHT in runs 2 and 3; run 4 changed no dlsym code, only merge glue).

**Exact next step.** Land. Then DL-16c (lock-free readers), DL-17 (`dlclose`, makes the unique pin testable), the
capacity story.

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §2, §4, §8; D-27.11, D-27.32; dynlink.md P-3..P-6, P-14.
