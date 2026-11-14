# DL-DLCLOSE (DL-17): dlclose, unloading and safe reclamation in Liella's ld.so

## State (run 2, DL-DLCLOSE-2, 2026-11-14): CHECKPOINT (context), INCOMPLETE: do not land as is

Branch `task/dl-dlclose-2` (run 1's `task/dl-dlclose` commits plus run 2's, base origin/next b60c73ac6), role keke:~/w/r5.
Code head `c5f79f668`; the hand-back commit adds only this note and handoff/reports/DL-DLCLOSE-2.json.

**Open defect (the next run's first step): table exhaustion under reader load.** Gate `dl-dlclose-2-gate1` on
a73b54e63 had 6557 passed and 1 failed: `dlclose_concurrent_with_readers`, SIGSEGV. gdb on the corpus that run left
(`repro.sh`, 1 run in 6) showed a reader in `readers::iterate` reading the program headers of an unmapped object. Cause:
`close_end` flipped the parity on every dlclose, so a reader that entered before one flip was counted with the readers of
the next. Fixed in c5f79f668: `reclaim` flips only when the non-current parity has drained; check 33 now closes two
objects in one callback (old products: SIGSEGV every run, job `dl-dlclose-2-oldprod`; new: `dl-dlclose-2-tests6`, 27
passed). **But** stress `dl-dlclose-2-stress2` (6 x 40 runs of the concurrent test, only my job in the role, host
load about 25) had 15 of 240 runs fail at check 51 with `cannot load more objects` at open 314..762. A reader thread
preempted inside `read_section` stalls every reclaim (correctly). Meanwhile the main thread opens 3 objects per iteration,
the slots are not freed, and the 512-object table fills. The old scheme hid this by unmapping too early. A bounded table
with a writer that never waits cannot avoid it. **Recommended fix (needs a maintainer's yes: dlopen/dlclose would
block):** at the entry of dlopen/dlclose, before taking the loader locks, when the retired count is above a threshold
(e.g. CAP/4), loop: take locks, `reclaim`, release; if still above, wait (nanosleep poll) for `drained()`. Do NOT wait
when the caller's own stripe counts a reader in the non-current parity (it may be that reader: a callback that calls
dlopen; then fail as today). No lock is held while waiting, so a callback's dlopen/dlsym in another thread proceeds. A
plain hole reuse (any freed slot, not only the top) does not suffice: a stalled reader frees nothing. Alternative:
DL-OBJ-TABLE-GROW. After the fix: rerun stress (`~/work/DL-DLCLOSE-2/stress.sh 6 40` via jobrun, 0 fails expected),
plants (handoff/DL-DLCLOSE-2-plants.txt, plant 1 updated for the new `drained()`), gate.
**Done:** the implementation (run 1, design below); run 2 fixed the cause of the red job and verified it:
- `dl-dlclose-tests1` (run 1) failed at the product check: `libc.so.6 does not export dlclose@GLIBC_2.2.5`, because
  csys/src/machine/compat_aliases.S was stale. Regenerated with `cargo xtask compat-gen` (commit 6f2568863).
- The dlclose tests then failed on their own corpus, not on the loader (commit ddb8f7b3e): libA/libC had no search
  path to libB (now `-rpath`); the concurrent mode overflowed the 4096-byte event log with 3000 iterations of `strcat`
  (the fortified `strcat` stopped it; the log is now reset per iteration); the C++ plugin was compiled as C, the program
  exported no C++ exception runtime (`-u` list) and libgcc_eh's hidden `_Unwind_Resume` is now linked into the plugin.
- Probe record `== close (DL-17)` appended to spec/behaviour/dynlink.glibc.txt and summarised in dynlink.md (C-1..C-9).
- Decision record `records/pending/DL-DLCLOSE-1.toml` (D-NEW, series D-27, amends D-27.44; CI assigns the id).
- Plants file handoff/DL-DLCLOSE-2-plants.txt (5 plants).
**Results (keke):** on c5f79f668: `dl-dlclose-2-tests6` (6 ld targets, 27 passed), `dl-dlclose-2-lint2` (records,
static, clippy clean), `dl-dlclose-2-stress2` (15 of 240 exhaustion fails, above). On a73b54e63 (superseded):
plants `dl-dlclose-2-plants1` 5 of 5 CAUGHT (one WARN-DEV-SHM, no effect); gate `dl-dlclose-2-gate1` 6557 passed, 1 failed
(the SIGSEGV above).
**Differences:** `dlclose` of NULL / a non-handle / a closed handle is covered by the existing row of
records/pending/DL-16-PLAN-3.toml (docs/differences-from-glibc.md); no new row.
**Not covered here:** DL-ATEXIT-OWNER (the opt-in interface; it now has the module map with unloading it waited for).
DL-ATFORK-FINALIZE is covered (fork.rs `forget_dso` from `__cxa_finalize`).
**Known edges (in the decision record):** an object a destructor re-opens stays with its destructors already run (they
run again at exit); a `dl_iterate_phdr` callback that leaves by longjmp stays counted (closed objects stay mapped,
never fault); `dlsym` does not create binding dependencies; thread_local destructors a plugin registered
(`__cxa_thread_atexit_impl`) do not pin it (reference behaviour not probed); atfork nodes of dropped handlers are kept
(40 bytes each).
**SPEC GAP (RULING NEEDED in the hand-back):** at exit the reference ran C-6's object (`STB_GNU_UNIQUE`-pinned, loaded
first) before C-7's (`RTLD_NODELETE`); Liella's `dlopen::fini_loaded` runs the last initialised first (DL-16a), which
would give C-7's first. Not compared on Liella and not changed here: a probe of two `RTLD_NODELETE` objects and of a
unique-pinned pair would show whether this is exit order in general or unique pinning.
**Next step:** that probe (then fix `fini_loaded` if needed); DL-ATEXIT-OWNER; the sorted address index for
`_dl_find_object` with DL-OBJ-TABLE-GROW.
**Do not redo:** the probe (record is in the tree); the compat alias (generated); the corpus fixes.

## Design (plan/ph14/design.md §8-§10, dl-16-dlopen.md §4 "Room for DL-17"; full text in the decision record)

**Liveness:** an object stays while it is initial, `nodelete`, has `opens > 0`, or is reachable from such an object
through `deps` (`DT_NEEDED`) or `binds` (objects outside its dependency closure its relocations bound to, reported by
the relocator through `UniqueRegistry::bound`). dlclose decrements `opens`; at 0 it marks from the roots and unloads
every unmarked object. **Finalisation:** reverse constructor order; `DT_FINI_ARRAY` in reverse, then `DT_FINI`; run
holding `CALL` only, objects still published (a destructor may `dlopen`/`dlclose`; exceptions unwind);
`__cxa_finalize(dso)` drops that object's atfork handlers. **Unpublication** under `STATE`: global scope, constructor
log, `l_next`/`l_prev` chain, reader entries (header count 0); `dlpi_subs` moves before dlclose returns.
**read_section:** two-parity reader counters striped by thread pointer over 32 cache lines; a writer flips the parity
and retires unpublished objects with the old parity; unmapped once that parity's counters sum to 0 (checked at once and
at each later dlopen/dlclose). Nothing waits for a reader. **Slots:** 512-object table; a freed slot is reused once
every slot above it is free (arenas rewind). **TLS:** surplus place freed (`Surplus::free`), module id returned to
`ltls::ModuleIds` (lowest free first, generation bumped per release), each thread's `THREAD_MODS` entry cleared under
the thread-list lock. **Errors:** -1 and `dlclose: invalid handle` (pending DL-16-PLAN-3, ruling #5006).

Files: crates/boundary/src/ld/dlopen.rs (`close_entry`, `close_begin`, `live_set`, `close_end`, `unload`, `reclaim`,
`record_binds`, `Finis`), ld/readers.rs (reader counters, `unpublish`, `flip`, `drained`), ld.rs (`module_room`,
`release_tls`, id-indexed `THREAD_MODS`), crates/ld (`ModuleIds`, `Surplus::free`, `UniqueRegistry::bound`), libc
`dlclose` export, fork.rs `forget_dso`. Tests: crates/ld/tests/dlclose.rs (basic, concurrent readers x3000, C++
plugin x300), crates/ld/tests/tls.rs `tls_surplus_free_and_module_id_reuse_with_generation`.
