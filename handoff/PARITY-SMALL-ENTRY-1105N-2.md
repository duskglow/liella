# PARITY-SMALL-ENTRY-1105N-2: nl_langinfo answer table built on demand (continues PARITY-SMALL-ENTRY-1105N)

State: branch `task/parity-small-entry-1105n-2` (stacks on `task/parity-small-entry-1105n`, from origin/next ff125cce),
role shiki:~/w/sh2 (Haswell-EP, E5-2697 v3). Code head aa5f6b93; handoff commits follow it. Read
handoff/PARITY-SMALL-ENTRY-1105N.md for the first run's profile (strnlen, isalpha, ctype_b_loc rows: not redone).

## Why the last run was red, and the fix
CI INSTR-REGRESS (cbench_startup setloc/printf/wctype about +4.4k): `LocaleView::new` built the per-view answer table, so
every `setlocale` paid for it. Now (view.rs):
* the view's `answers: AtomicPtr<Answers>` starts at the static `NO_ANSWERS` (all slices empty, so a lookup misses with
  no extra test on the fast path); `decoded: AtomicU32` counts calls that decoded an entry (racy count, paces only);
* `note_decoded` (cold): call number `ANSWERS_AFTER + 1` (257) builds the table and publishes it with
  compare_exchange (Release; readers Acquire). A racing second builder's table stays unused (leaked once). Allocation
  failure restarts the count; it no longer fails the view (the eager version made `LocaleView::new` fail on ENOMEM).
* the const "C" view gets a table the same way. boundary `tb::answers_at` is the one new `UNSAFE(slice)` deref (static or
  leaked, never freed or changed).
* boundary `langinfo`: the table lookup, then a tail call to the out-of-line `langinfo_decode` (decode + note_decoded,
  else `langinfo_slow`). Inline, the note_decoded call made nl_langinfo push/pop three registers on every call
  (percall 40.8 against 36 eager; found by disassembling Liella's nl_langinfo); commit aa5f6b93 restores a leaf.
* Threshold 256: about build cost / per-call saving (a few hundred items vs ~6 instructions a call), so a program that
  asks CODESET once never builds it. Local, testable: no D-row needed beyond this note (D-NEW not used).

## Results (instructions:u)
| row | limit / glibc | eager (d8ae777, CI) | f63e01b7 (lazy, non-leaf) | aa5f6b93 (final) |
|---|---|---|---|---|
| startup_instr_setloc | 69800 | 74198 | 67728 | job `-startup-2` |
| startup_instr_printf | 73600 | 77992 | 71518 | job `-startup-2` |
| startup_instr_wctype | 70900 | 75280 | 68807 | job `-startup-2` |
| percall nl_langinfo_mon | glibc 38 | 36 (origin/next 42) | 40.8 | job `-percall-3` |
| percall nl_langinfo_l_mon | glibc 35 | 43 (origin/next 49) | 47.8 | job `-percall-3` |
| percall nl_langinfo_l_codeset | glibc 28 | 36 (origin/next 42) | 40.8 | job `-percall-3` |
Startup rows on f63e01b7 are under their limits; aa5f6b93 changes only the nl_langinfo call path (not start-up), so they
should be the same. The aa5f6b93 percall is expected near the eager numbers (the push/pop pair gone) - NOT VERIFIED at
hand-back: read `~/logs/parity-small-entry-1105n-2-percall-3.log` on shiki. If nl_langinfo_mon is not below 40.8, the
model is wrong: profile before changing anything else.

Jobs on f63e01b7 (superseded by aa5f6b93): gate rc 0 (9492 passed, 0 failed; conformance 24/0; peak 6.2G), plants 7/7
CAUGHT, startup counts rc 0, percall rc 2 (base ff125cce not primed on shiki; rerun `--base none` as `-percall-2`, rc 0).
Final-head jobs: -plants-2 rc 0 (7/7 CAUGHT, 0 WARN); parity-small-entry-1105n-2-gate-2, -startup-2, -percall-3 (shiki).

## Next step
1. Read the four final jobs (`~/bin/jobwait parity-small-entry-1105n-2-gate-2 parity-small-entry-1105n-2-startup-2
   parity-small-entry-1105n-2-percall-3` on shiki).
2. Then the first run's remaining list: strnlen asm entry (-3/call), isalpha null test (-2/call), nl_langinfo_l's view_of
   null/alignment test (fold to one test), ctype_b_loc to the saturation register.
