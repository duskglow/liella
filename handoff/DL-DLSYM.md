# DL-DLSYM (DL-16b): handoff (run 2, DL-DLSYM-2)

**State.** Branch `task/dl-dlsym-2` (continues `task/dl-dlsym`), base DL-DLOPEN-LOAD-3 plus `origin/next` merged
(ruling #5331, merge commit `59b3d5117`). Role shiki:~/w/sh1. Full report: handoff/DL-DLSYM-2-report.md (this page
is the summary).

**Done.**
- Run 1 (DL-DLSYM): `dlsym`/`dlvsym` in `ld.so` (crates/boundary/src/ld/dlopen/dlsym.rs): scopes, RTLD_NEXT,
  RTLD_DEFAULT (+ caller's local scope, #5127), DEEPBIND, unique table with pinning, IFUNC and start-time STT_TLS
  answers, handle validation, texts; exports in libc.so.6 and libc.a (#5264).
- Run 2: the `dl-dlsym-t1` red was the test, not the loader: it named its start-time helpers by `DT_SONAME`, which
  the start-time loader cannot search for yet (it opens `DT_NEEDED` by path only); they are now linked by path.
  Then check 10 found a real defect: `dlvsym` answered a definition from an object without version tables.
  dynlink.md row "dlvsym missing version ... also for an object without versions" says it fails; `Want::Version`
  now matches only an exact version (relocations keep D-27.11's permissive `Want::Reference`). Pending records:
  `records/pending/DL-DLSYM-2-1.toml` (DL-16b design, series D-27) and `-2.toml` (amends D-27.11).
- `s27_dlsym_handles` (handles.c) and `s27_dlopen_failures` flipped from PENDING-LIELLA to passing (both pass on
  Liella after the merge; run.sh failed the stale pending mark of the second).
- Check 45 added (RTLD_DEFAULT from the program finds the program first). Plants: handoff/DL-DLSYM-2-plants.txt (12).
- Probe P-14 (#5321): the reference opens 2000 distinct objects, no limit; Liella refuses the 513th
  (`<name>: cannot load more objects`). dynlink.md row P-14 and a differences row; capacity ruling open.

**Decisions in force.** D-27.11 (amended by pending DL-DLSYM-2-2), D-27.32, D-27.33, D-27.35; rulings #5127, #5264,
#5331; pending DL-DLSYM-2-1.

**Open / RULING NEEDED.** #5321 capacity: recommend a growable object table (restores the reference's behaviour; no
switch needed) as its own story. `pin_unique` is not observable until `dlclose` (DL-17): no test or plant reaches it.

**Do not redo:** the scope design (P-5); the version rule change (dynlink.md row is probe-settled).

**Gate: RED on one start-up layout test** (`hot_text_products::init_data_clusters_hold_at_every_text_phase`: tool
row 53 faults, 54 at pad `cold 0x2000`; origin/next 1700e1798 passes). Plants 12/12 CAUGHT. Details in the report.

**Exact next step.** 1. Run that test on DL-DLOPEN-LOAD-3 merged with next (fresh tree) to see whether the base
already moves it; 2. per the ruling, fix the layout (ALWAYS FORWARD) or treat as LAYOUT-JITTER; 3. land. Then: DL-16c (lock-free readers; dlsym takes the writer
lock now), DL-17 (`dlclose`, which makes the unique pin testable), the capacity story (#5321).

**Plan rows to read:** plan/ph14/dl-16-dlopen.md §2, §4, §8; D-27.11, D-27.32; dynlink.md P-3..P-6, P-14.
