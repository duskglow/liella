# STATIC-PIE-LOAD-OFFSET: one source for the static PIE's load bias

**State:** branch `task/static-pie-load-offset` (stacked on DL-BOOT-RELOC-5, base origin/next), code head `20ad530a`.
Role shiki:~/w/sh1. Decision row **D-27.12** (reserved id).

## What changed
* `process::EXE_BIAS` (boundary/src/process.rs): set once by the static-PIE start (`runtime::initial_thread::<true>`,
  after the relocator's pass 1) to the bias the relocator took from `__ehdr_start`. `process::exe_bias(words, phnum,
  phdr)` gives it, or the `PT_PHDR` bias when it is 0 (non-PIE static, dynamic programs: unchanged behaviour).
* Users: `is_read_only` (`%n`; `liella_rt::start::read_only` now takes the bias, not `phdr_addr`), `s27_dynlink::scan`
  (so `dl_iterate_phdr`'s `dlpi_addr` and `_dl_find_object`'s ranges / `dlfo_eh_frame`), `backtrace::Executable::find`.
* Non-PIE start: the store is on the const-generic `PIE` branch (chosen at link time), so `start::<false>` has no new
  code. Static workload counts were NOT measured (see below): this is argued from the code, NOT VERIFIED by counts.
* Tests: rt unit test `read_only_tests_the_segments_moved_by_the_given_bias`; conformance
  `s27_static_pie_load_offset.c` (C++ static PIE, relr; 100 starts at random bases; base = AT_ENTRY - e_entry read
  from /proc/self/exe; dl_iterate_phdr dlpi_addr/dlpi_phdr, function in a PF_X and object in a PF_W segment;
  _dl_find_object for both + eh_frame, -1 for an anonymous mapping; `__printf_chk(1)` `%n` accepted from .rodata,
  SIGABRT from .bss; a throw through 3 frames with destructors).
* conformance/run.sh: a LINK-CXX + LINK-STATIC-PIE program links crtbeginS.o/crtendS.o (crtbeginT.o is for
  fixed-address static programs).
* 3b: docs/toolchain.md already lists liella-rcrt1.o in §3.3 and gives the static-PIE link line in §4 (DL-BOOT-RELOC-4);
  no change needed.

## Jobs (shiki)
* `static-pie-load-offset-lint3`: clippy, static, `xtask so`, conformance s27_static_pie_load_offset, s01_static_pie,
  s01_static_pie_rela, s22_once_cxx: rc 0, 4 passed.
* `static-pie-load-offset-plants`: `cargo xtask plants handoff/STATIC-PIE-LOAD-OFFSET-plants.txt`: rc 0, 5/5 CAUGHT,
  no WARN.
* `static-pie-load-offset-gate`: `cargo xtask affected --run --base origin/next` under ~/bin/gate. It **escalated to
  FULL by itself** (the change touches conformance/run.sh); still running at hand-back.
* Superseded: lint1 (crash-audit: deliberate abort needed expect_crash), lint2 (s27 crashed in the unwinder, below),
  count/count2/count3 (rc 2: base not primed, then primed; rc 1: `wl_prep: no link command for seq` / `for printf`).

## INFORMATIONAL
* A static C++ program that throws but never uses threads crashed (jump to 0 in libgcc's `uw_init_context_1`): the
  unwinder calls `pthread_once` through a weak reference, a weak reference does not pull libc.a's member, while
  `__pthread_key_create` is linked (so the unwinder thinks threads are active). `nm`: s27 had no `pthread_once`,
  s22_once_cxx (std::call_once) had it. Not PIE-specific. The test now names `pthread_once`. Recommendation: a story
  to make that link work without help (e.g. the object that defines `__pthread_key_create` also references
  `pthread_once`), after a black-box probe of the same program against the reference static library.
* Not covered by a test or plant: the backtrace's use of `exe_bias` in a static PIE (same function as the covered users).
* wl_count cannot link printf/seq on this host (wl_prep "no link command"); the static workload counts the brief asks
  for are missing.

## Next step
1. Read the gate (`ssh shiki ~/bin/jobwait static-pie-load-offset-gate`).
2. Static start counts before/after (printf-loop is 20000 static starts) on a host where wl_prep links them; expected
   identical instructions for the non-PIE start.

Plan rows: D-27.12, D-27.10, D-27.4, D-27.5, D-12.4. Plants: handoff/STATIC-PIE-LOAD-OFFSET-plants.txt.
