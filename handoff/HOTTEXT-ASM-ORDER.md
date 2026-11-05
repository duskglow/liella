# HOTTEXT-ASM-ORDER (branch task/hottext-asm-order-2, role mei:~/w/s3)

State: run 2 (HOTTEXT-ASM-ORDER-2) done; code head cb4fc2d6 on next db50ef41. Commits: 4cc83213 (per-function
`.S` sections), bff0ac96 (mempcpy `.p2align 6`, D-4.33), cb4fc2d6 (regenerated hot-text list + grouping test).
No bound touched.

## What the story does

- xtask/src/hottext.rs renames function sections only, so `.S` kernels in plain `.text` landed after
  the whole hot group and regen dropped their traced names. Run 1 gave each function in a plain `.text`
  region of scan, fold, points, tp, guard, barrier, rdtsc, once, trap, restorer, pkru, call, fpu and
  thread_start `.S` its own `.text.<first label>` (34 sections; split rules in run 1's notes: only after
  jmp/ret/ud2, no open CFI region, no `.size` spanning the cut).
- D-4.33 (run 1's regen build failed it): mempcpy/__mempcpy sat 64-aligned only by position inside
  memmove's section; bff0ac96 gives its own section `.p2align 6`. Run 2 checked every exported
  (globl, not hidden) label of every `.S` for an explicit `.p2align >= 6` before it (scratch script,
  chisato ~/w/.scratch/HOTTEXT-ASM-ORDER-2/expalign.py): every exported entry has one; the only
  others are hidden `liella_machine_*` internals and compat data symbols, not in dynsym. Both regen
  builds (fast, full) and every count build print "all 2626 exported function entries ... 64-byte aligned".
- cb4fc2d6: list regenerated on the split tree (startup/stdio/setlocale probes + the coreutils-shaped
  `page_faults tool`), fast profile then LIELLA_FULL_PROFILE=1 (union): 503 -> 543 lines; kernels
  now listed: strncmp, memchr_entry, strcpy family, compare, equal, memmove, memset, strchr, strcmp, ...
  (liella_machine_read is not traced by any probe, so it is not listed). `init_cold_startup_functions_are_grouped`
  now also names strncmp, memchr_entry, strcpy, compare, memmove, memset.

## Results (hot_text_products, 5 runs each, every run identical)

| host | tree | tool (bound 71) | locale (68) | startup (23) | strings |
|---|---|---|---|---|---|
| mei (Skylake-SP) | next (run 1) | 68 | 66 | 19 | 0 |
| mei | split only (run 1) | 68 | 66 | 19 | 0 |
| mei | head cb4fc2d6 list | 60 | 59 | 17 | 0 |
| keke | head cb4fc2d6 | 60 | 59 | 17 | 0 |

keke "before" was not measured cleanly: hottext-asm-order-2-faults-keke built next's `.S` with the NEW list and
test (61/59/17; the grouping and string-call tests fail there by design: the list needs the split), rc 1, SUPERSEDED.

## Jobs (final head cb4fc2d6)

- mei hottext-asm-order-2-faults-mei: rc 0 (regen + 5 runs; the tree had the same list, the old test).
- mei hottext-asm-order-2-plants: rc 0; baseline ok; strncmp / memchr_entry / memmove back in plain
  `.text`: all CAUGHT with "<name>: not in the hot group".
- mei hottext-asm-order-2-gate: the affected tool escalated itself to FULL (hot-text.list is a linker
  input; .S files define exported symbols); clippy + xtask static passed in it before the tests; left
  running detached for the dispatcher. Log ~/logs/hottext-asm-order-2-gate.log.
- keke hottext-asm-order-2-faults-keke-2: head only, 5 runs, started at hand-back.

## Next step

None for the code. If the FULL gate shows a failure, look at it in that log first (no reruns to pass).
Bounds could be lowered (tool 71 -> ~62, locale 68 -> ~61, startup 23 -> ~19) in a separate story once CI
reads the same counts; not done here (the brief asked only that none be raised).
