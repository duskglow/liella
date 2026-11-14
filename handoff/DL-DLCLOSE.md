# DL-DLCLOSE (DL-17): dlclose, unloading and safe reclamation in Liella's ld.so

## State (run 3, DL-DLCLOSE-3, 2026-11-14): COMPLETE pending its named jobs

Branch `task/dl-dlclose-3` (runs 1-2's commits plus run 3's; base origin/next b60c73ac6, not merged forward), role
kanon:~/w/kn1. Code head `b4ec9ff46`; the hand-back commit adds only this note and handoff/reports/DL-DLCLOSE-3.json.

**Run 3 fixed the table exhaustion (ruling #5651) in two steps.** (1) `dlopen::room_wait` at the entry of `dlopen` and
`dlclose`, holding no lock: while more than 128 objects (CAP/4) are retired it reclaims and poll-waits
(`clock_nanosleep` 50 µs) for `readers::drained()`. It never waits when the caller holds `CALL` (a constructor or
destructor: another thread's callback may be blocked on that lock) or is inside a read section. "Inside" is exact: the
thread block's new `dl_reading` words (0x288-0x297, two of the spare words; the thread's own read sections per parity),
not the stripe (another thread can share it). A `fork` child clears every stripe and puts its own counts back
(`readers::after_fork_child`). (2) The stress at 12 workers still failed 4/240: readers that are always inside some
walk leave holes below the newest retired objects (slots are reused from the top only), so the top climbs with few
retired. `room_short()` therefore also waits when anything is retired and the top (`Loader::n`) is past 3/4 of the
table. Recorded as item (9) of the pending dlclose record `records/pending/DL-DLCLOSE-1.toml` (D-NEW, series D-27,
amends D-27.44; CI assigns the id; ruling #5628: records/pending/ is the place). #5654: the differences row and the
comments now cite D-27.32 (the assigned DL-16-PLAN-3 record) instead of the pending path.
**Tests (new, `s27_1_c_4_dlopen_waits_for_readers_only_outside_them`, mode `room`):** checks 60-61 a callback's
open/close loop fails cleanly with `cannot load more objects` (no hang) and the next call outside reclaims; 62-64 a
reader stalled inside a walk makes the main thread wait (seen asleep after 100..200 pairs, via /proc state S) instead
of failing; 65-68 a constructor passing the threshold behind a reader blocked on `CALL` does not deadlock; 69 a child
forked from inside a walk, while another thread is stalled in its own, runs 600 pairs; 70-72 a reader re-entering a
walk every 10 pairs never fills the table. `alarm` is only the hang guard.
**Results (kanon):** on the final code: `dl-dlclose-3-tests3` (6 ld targets, 28 passed), `dl-dlclose-3-lint2` (records,
static, clippy clean), stress `dl-dlclose-3-stress4` (12 x 20 runs: 0 of 240 failed, load up to 16) and
`dl-dlclose-3-stress5` (6 x 40: 0 of 240, load 11-13); started at hand-back: plants `dl-dlclose-3-plants1`
(handoff/DL-DLCLOSE-3-plants.txt, 12 plants, three of them end at the 300 s alarm) and gate `dl-dlclose-3-gate1`
(direct tier). Superseded: `stress1`/`stress2` (6 x 40, 0 of 240 each, before the top trigger), `stress3` (12 x 20,
4 of 240 failed: the holes case, fixed in b4ec9ff46), `tests1`/`tests2`, `lint0` (inherited head clean), `lint1`.
**Rulings applied:** #5651 (wait), #5652 (exit order is the follow-up story **DL-EXIT-ORDER-PROBE**: `fini_loaded`
left as it is), #5654 (cite D-27.32), #5628 (records/pending/).
**Known edges (in the record):** a callback that leaves by `longjmp`, or whose thread exits inside it, stays counted:
objects closed meanwhile stay mapped, and past the threshold every later `dlopen`/`dlclose` outside a read section
waits for ever. An object a destructor re-opens keeps its destructors run (they run again at exit); `dlsym` creates no
binding dependencies; thread_local destructors a plugin registered do not pin it; atfork nodes of dropped handlers are
kept (40 bytes each). Live objects alone past the table (512) still fail: DL-OBJ-TABLE-GROW.
**Next step:** land; then DL-EXIT-ORDER-PROBE (the reference ran a unique-pinned object loaded first before a later
`RTLD_NODELETE` one at exit; Liella's `fini_loaded` runs the last initialised first: probe two `RTLD_NODELETE` objects
and a unique-pinned pair); DL-ATEXIT-OWNER; the sorted address index for `_dl_find_object` with DL-OBJ-TABLE-GROW.
**Do not redo:** the probe record (spec/behaviour/dynlink.glibc.txt `== close (DL-17)`); the compat alias
(generated); the corpus fixes; the stress (`~/work/DL-DLCLOSE-3/stress.sh W N` on the role: keeps each failure's output).

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
at each later dlopen/dlclose). No writer waits under the locks; past the room threshold dlopen/dlclose wait at
entry (`room_wait`, above). **Slots:** 512-object table; a freed slot is reused once
every slot above it is free (arenas rewind). **TLS:** surplus place freed (`Surplus::free`), module id returned to
`ltls::ModuleIds` (lowest free first, generation bumped per release), each thread's `THREAD_MODS` entry cleared under
the thread-list lock. **Errors:** -1 and `dlclose: invalid handle` (D-27.32, ruling #5006).

Files: crates/boundary/src/ld/dlopen.rs (`close_entry`, `close_begin`, `live_set`, `close_end`, `unload`, `reclaim`,
`record_binds`, `Finis`, `room_short`, `room_wait`), ld/readers.rs (reader counters, `unpublish`, `flip`, `drained`,
`inside`, `after_fork_child`), crates/rt/src/tls.rs (`dl_reading`), ld.rs (`module_room`,
`release_tls`, id-indexed `THREAD_MODS`), crates/ld (`ModuleIds`, `Surplus::free`, `UniqueRegistry::bound`), libc
`dlclose` export, fork.rs `forget_dso`. Tests: crates/ld/tests/dlclose.rs (basic, concurrent readers x3000, room,
C++ plugin x300), crates/ld/tests/tls.rs `tls_surplus_free_and_module_id_reuse_with_generation`.
