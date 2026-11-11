# DL-NONSHARED-OPTOUT: opting out of libc_nonshared.a (investigation, 2026-11-11)

**State.** Branch `task/dl-nonshared-optout` (from `origin/next`). The head is in the hand-back and in
handoff/reports/DL-NONSHARED-OPTOUT.json. Deliverables: this note, the test-only prototype
`crates/legacy/src/owner_proto.rs` (compiled under `selftest` only: no export, no product change), its
tests `crates/core/tests/nonshared_optout_proto.rs` (8 tests) and plants `handoff/DL-NONSHARED-OPTOUT-plants.txt`.
Nothing is deprecated and libc.so.6's exports are unchanged. Plan rows: phase 4 (`plan/06-phases/06-phase-4-process-runtime.md`,
the static library's `atexit`/`at_quick_exit`), D-1.2 (exit-handler registry), ruling #4807 (DL-LIBC-NONSHARED ships the archive).
**Do not redo:** the probes below. **Next step:** the owner's rulings at the end. Then a product story (phase 14, with
dlopen/dlclose) builds the recommended mechanism. The prototype's map and unload step are its starting point.

## Evidence: black-box probes (installed glibc 2.39, x86-64 build host kanon; jobs `dl-nonshared-optout-probe`, `-probe3`)

Probe sources are in the scratch directory, not in the tree. They cover only exports, archive symbol tables and behaviour.
* **P1 exports.** libc.so.6 exports `__cxa_atexit@@GLIBC_2.2.5`, `__cxa_at_quick_exit@@GLIBC_2.10`,
  `__cxa_finalize@@GLIBC_2.2.5`, `__register_atfork@@GLIBC_2.3.2` and `pthread_atfork@GLIBC_2.2.5`. That last one is a
  NON-default version, so a new link never binds to it. It exports no `atexit` and no `at_quick_exit`.
  libc_nonshared.a defines `atexit`, `at_quick_exit`, `__pthread_atfork`, `pthread_atfork` (weak) and `__stack_chk_fail_local`.
  It references `__cxa_atexit`, `__cxa_at_quick_exit`, `__register_atfork`, `__dso_handle` and `__stack_chk_fail`. A default link copies
  `atexit` into the program as a local symbol (`t atexit`), with `__dso_handle` in its data.
* **P2 dlclose behaviour with the stub** (`main` + `liba.so` + `libb.so`):
  * Case 1: B's own `atexit` handler ran at `dlclose(B)`.
  * Case 2: A's helper registered B's handler. The handler ran at `dlclose(A)`, while B was still loaded. The stub
    attributes a registration to the module whose code called `atexit`.
  * Case 3: B registered `at_quick_exit` and `pthread_atfork` handlers, then `dlclose(B)`. A later `fork` and
    `quick_exit` ran neither: they are dropped at unload.
* **P3 opt-in marker under glibc's loader.** A program whose `atexit` reference carries a version node only Liella would
  define (`atexit@LIELLA_OWNER_1`, linked against an interface stub with soname libc.so.6) fails AT LOAD. This holds with
  lazy binding and with `LD_BIND_NOW=1`, and `ldd` reports it too: `./prog_optin: /lib/x86_64-linux-gnu/libc.so.6: version
  `LIELLA_OWNER_1' not found (required by ./prog_optin)`, exit status 1. Nothing runs and nothing is misattributed.

## 1. Mechanisms

A registration path with no stub calls libc's `atexit(f)` (or `at_quick_exit`, `pthread_atfork`) directly. libc must then
find the owner: the module whose unload must run (exit list) or forget (quick-exit, atfork lists) the entry.

| | how it works | registration cost | per load | per unload | differs from the stub when |
|---|---|---|---|---|---|
| (a) caller's return address | A two-instruction C-layer entry shim passes `[rsp]` to the registration. libc asks ld.so's module map which object holds that address and stores that object's per-load handle (eager). No module (JIT, anonymous memory) gives the process: the entry runs at exit only. | One binary search over the loaded `PT_LOAD` ranges, at most 10 comparisons for 512 ranges (prototype test). Estimated 50-80 instructions for a 100-module process, not counted on a product. Registrations are a few per module, mostly at start-up. | none extra: the same sorted map serves `_dl_find_object`/`dladdr`, which phase 14 needs anyway | scan of the exit and quick lists (entries × the module's ranges), as `__cxa_finalize` already does | a **tail call**: `return atexit(f)` compiled as `jmp` returns into the caller's caller, so the wrong module is named. Inlining is not a difference: the inlined call site is in the module that holds it, and the stub would be that module's too. |
| (b) handler's code address | Store nothing new. When module M unloads, take every entry whose handler lies in M's text. | 0 | 0 | the same scan | Case 2: B's handler runs at B's unload rather than A's. A handler in libc or another module with module-owned data via `arg` is never M's. For `atexit`, `at_quick_exit` and atfork there is no `arg`, so the handler address is the whole story. |
| (a)+(b) **recommended** | (a) eagerly, plus (b) as a safety net at unload for opted-in entries: an entry goes when its owner OR its handler belongs to the unloading module | as (a) | 0 | the same scan | only where (a) and (b) disagree. The entry then runs at the FIRST of the two unloads, so a handler is never called after its code is unmapped. |
| (c1) version-node opt-in | the carrier for (a)+(b), see 2 | | | | |
| (c2) header path | Under a feature macro, `<stdlib.h>`/`<pthread.h>` give `atexit(f)` as a function-like macro for `__cxa_atexit((void(*)(void*))(f), 0, &__dso_handle)`, and likewise `__cxa_at_quick_exit` and `__register_atfork`. All of these are exported by glibc too (P1). | 0 | 0 | as today | Never differs: the identity is the module's own `__dso_handle`. But `(atexit)(f)` and `&atexit` still pull the stub, and the program must be recompiled. |
| (c3) loader-supplied handle per reference (a special relocation or symbol ld.so resolves per module), (c4) per-module PLT thunks | rejected: (c3) needs linker/psABI work for no gain over (a), since registrations are rare. (c4) needs writable+executable memory per module (W^X). | | | | |

Not built: the loader map has no runtime form before phase 14 (`crates/ld/src/api.rs` is a placeholder). So the costs
above are a model and an estimate. The scan per unload is the same order as today's `exit_handlers::finalize`.

## 2. The opt-in

* **Carrier: a Liella-only symbol version node** (name illustrative: `LIELLA_OWNER_1`). It goes on `atexit`,
  `at_quick_exit` and `pthread_atfork`, all exported by libc.so.6 as NON-default versions (`atexit@LIELLA_OWNER_1`, one `@`).
  A default link never binds an unversioned reference to a non-default version, so every existing link is unchanged: it
  still takes the archive's stub, and two-way compatibility stays the default. An opted-in link points at a link-time
  interface stub that exports the same symbols as default versions (`@@`). The stub sits in a separate directory
  (`-L<libdir>/liella-owner`, or a compiler spec/driver flag a distribution sets), and its `libc.so` script names the stub
  and not libc_nonshared.a. The program then carries `verneed libc.so.6: LIELLA_OWNER_1` and no archive members.
* **On glibc:** refused at load with a message naming the node (P3), lazily or not. This is the required "fail clearly".
* **On Liella:** ld.so already checks verneed. It records the node as a per-module "opted in" flag, which the deprecation
  reporting below reads. libc attributes registrations as (a)+(b).
* **Rejected markers:**
  * An ELF note or a new dynamic tag fails the fail-clearly rule. glibc's loader is not expected to reject a note or tag
    it does not know, so the program would load and misattribute silently. This is reasoned, not probed.
  * A `DT_NEEDED` on a Liella-only library fails clearly too ("cannot open shared object file"). But it adds a file and
    the message does not say why.
* C++ static destructors are unaffected: the compiler calls `__cxa_atexit(…, &__dso_handle)` directly, and `__dso_handle` comes from the
  compiler's crtbegin, not from the archive.

## 3. Edge cases (prototype test in brackets)

* **Helper in A registers B's handler** [`helper_in_one_module_registering_anothers_handler`]:
  * (a) names A, as the stub does (P2 case 2).
  * (b) names B.
  * (a)+(b) runs the handler at whichever of A or B unloads first. The stub path keeps A only, so if B unloads first the
    entry outlives B's code.
* **JIT or anonymous memory** [`jit_and_anonymous_memory_belong_to_the_process`]:
  * Caller or handler in no module gives the process: the entry runs at exit.
  * Eager attribution keeps a module later mapped over freed JIT memory from claiming the entry by owner. A lazy variant
    stores the raw return address and tests ranges at unload; it would claim the entry, which is why eager is recommended.
  * A JIT handler whose memory is freed before exit is the program's error, as it is with the stub.
* **Static programs.** One module and no unload. libc.a skips the lookup (owner = process), so the cost is 0. The opt-in
  has no effect.
* **Library dlopen'd twice** [`reload_at_the_same_address_is_a_new_owner`]:
  * A second open does not map again: the map refuses overlap.
  * Close then reopen gives a new handle, even at the same address, so an old handle never matches the new instance.
* **Tail calls and inlining** [`tail_call_names_the_callers_caller_and_the_net_covers_it`]:
  * The test shows (a) alone leaving A's handler registered after A is gone.
  * The (b) net runs it at A's unload.
  * Inlining: see the table.
* **Foreign (non-Liella) modules** [`explicit_dso_from_a_stub_or_foreign_module_wins`]:
  * Modules built against glibc's or Liella's archive pass `&__dso_handle`. An explicit non-zero `dso` always wins.
  * An unload also takes entries whose `dso` lies inside the module's ranges, which is where `__dso_handle` is.
* **Quick-exit entries** [`quick_exit_entries_are_forgotten_on_unload`] are forgotten at unload (P2 case 3).
* **Lookup cost** [`lookup_is_logarithmic_and_exact`]: 256 modules (512 ranges, loaded out of order) give exact owners
  at range edges in at most 10 comparisons.
* Open for the product story:
  * Registration can run inside a constructor while dlopen holds the loader lock. The map read must therefore be lock-free
    (the `_dl_find_object` structure) or re-entrant.
  * `pthread_atfork` has three handlers, so the net checks all three addresses.

## 4. Prototype

`liella_legacy::owner_proto` (`#[cfg(feature = "selftest")]`):
* `ModuleMap`: sorted ranges, a handle per load, binary search with a comparison count.
* `attribute` (policy `Caller` or `Handler`; an explicit `dso` wins) and `opted_atexit`/`opted_quick`.
* `finalize_module`: the unload step over the real `exit_handlers::Registry`, with the `net` switch.

The tests build entries exactly as the stub and the opted-in paths would, then check what each unload runs or forgets.
9 plants (map order, range end, overlap, handle reuse, policy, explicit dso, owner-by-address, net, quick list) each
name the test that must catch them.

## 5. Recommendation (owner)

1. **Mechanism:** (a)+(b). Eager caller attribution through ld.so's module map, plus the handler-address net at unload,
   applied to opted-in registrations only.
   * Cost: one map search per registration, 0 per load, and the existing per-unload scan.
   * No behaviour differs from glibc for any program that does not opt in, so no differences row and no compat flag.
   * Extending the net to stub-registered entries (P2 case 2 with B closed first) would be a difference. It would need a
     row in docs/differences-from-glibc.md and a COMPAT flag restoring owner-only finalisation. Not recommended now.
2. **Opt-in:** the non-default version node plus the separate link-stub directory (2). It is a new public interface,
   so it needs a ruling.
3. **Migration a distribution can follow:**
   1. Ship the opt-in.
   2. Optionally adopt the (c2) header path. It is two-way compatible and removes most stub uses without opting in.
   3. Turn the opt-in on per package for packages that do not need to run on glibc.
   4. Find the remaining users of the archive:
      * Liella's archive members carry a small allocated note (for example `.note.liella.nonshared`). `readelf -n` finds
        them in a package scan, and ld.so can report them under a debug setting.
      * For foreign modules (glibc's archive, no note), heuristic: no `LIELLA_OWNER_1` verneed and a reference to
        `__register_atfork` or `__cxa_at_quick_exit`. `__cxa_atexit` alone is ambiguous with C++ destructors.
   5. Warn: archive members carry `.gnu.warning.<symbol>` sections, so GNU ld warns at link time. A distribution can
      make that fatal with `--fatal-warnings`.
4. **Deprecation end state, with its cost:**
   * The default `libc.so` script switches to the opt-in stub and the archive keeps only identity-free members.
     `__stack_chk_fail_local` (P1b) must stay available to objects that reference it.
   * Cost: every NEW build then needs Liella and is refused by glibc's loader (P3). This ends two-way compatibility by
     default. Existing binaries are unaffected: they carry their stubs.
   * The alternative keeps the archive frozen as the default forever (5 tiny members) and deprecates it only in
     documentation and warnings for opted-in distributions.
   * I recommend the alternative until the owner rules that new builds need not run on glibc.

## Rulings needed

* The opt-in's public interface: the version node name and non-default exports on `atexit`, `at_quick_exit`,
  `pthread_atfork`, plus the link-stub directory. Recommend adopting it in the phase 14 product story, with a decision
  record and user docs (the extension rule: glibc default, opt-in, documented).
* The deprecation end state. Recommend: keep the archive as the default while two-way compatibility is a default, and
  deprecate it by warnings for opted-in distributions only.
* Whether the handler-address net should also cover stub-registered entries. Recommend no, for now. If yes: a
  differences row and a COMPAT flag.
