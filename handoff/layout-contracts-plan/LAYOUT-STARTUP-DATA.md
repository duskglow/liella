# LAYOUT-STARTUP-DATA: start-up read-only and written objects in a budgeted number of windows and pages

Id: LAYOUT-STARTUP-DATA
Kind: code (tooling)
Size: M
Model: opus
Effort: high
Plan: handoff/layout-contracts-plan.md (inventory L5, L6, L14; proposed D-X.NEW-5). After LAYOUT-STARTUP-WINDOWS.

## The change

Two earlier bound raises were data moves: TIMER-SIGEV-THREAD-NOSPAWN-4 (two start-up `.rodata` names straddled a
64 KiB window; fixed by the startup-names object) and UTMP-Y2038-UNSIGNED-2 (NSCACHE moved from the page `.bss`
already dirties to a page of its own in `.data`). Nothing checks either kind of move at build time.

1. Start-up object set: at regen time, record the objects the start-up probe touches from
   `perf record -e page-faults -d` addresses plus a gdb watch-free method of your choice (e.g. the relocations and
   RIP-relative operands of the traced start-up functions resolved to data symbols); write
   `crates/libc-so/startup-data.keys` (`@generated`, build-stable keys, section kind ro/rw).
2. Check (manifest kind `startup-data-windows`): read-only start-up objects in at most N 64 KiB windows of the probe;
   written start-up objects in at most M 4 KiB pages; fail naming the object that sits alone in a window or page
   beyond the budget. Budgets from origin/next per profile.
3. Evaluate a fix in the same spirit as `.lbss`/`.rodata1`: rename the sections of the written start-up objects to a
   section the default scripts place together (measure; keep only with no row slower), and record which.
4. Plants: budget minus one; NSCACHE's section moved back to `.data` order; a start-up rodata name moved out of the
   startup-names object.
5. D-X.NEW-5; manifest rows.

Products and counters only; no timing.
