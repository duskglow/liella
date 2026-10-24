<!-- Phase 14 milestones (PH14-DESIGN, 2026-10-09). Each row is at most one agent run. -->

# Phase 14 milestones

> For dispatch, this table is replaced by the story list in
> `plan/06-phases/17-phase-14-dynamic-linking.md` (PHASE-14-PLAN, 2026-10-23): the same scope cut
> into 34 stories of one 50-call run (DL-0a to DL-30). This table stays as the design-level view.

Sizes are agent runs of the usual budget (~60-100 tool calls). Risk: L/M/H. "Needs" = must be
merged first. Every code milestone ends with `vm_affected` green and its own corpus tests; the
coordinator gates at merge. Rulings each depends on are R14-n in `open-questions.md`.

| ID | Milestone | Needs | Rulings | Risk | Delivers / exit |
|---|---|---|---|---|---|
| M14-0 | **Black-box behaviour records** (no product code): set-group-ID probes for the threat-model open questions (LD_PRELOAD/LD_AUDIT exception, `$ORIGIN` in secure mode, empty path elements, `/etc/ld.so.preload`, corrupt cache, LD_BIND_* residue, TZ), dlerror texts, loader exit status, constructor/destructor order corpus run against the reference, static-TLS surplus measurement (largest IE-TLS dlopen that succeeds), `$LIB`/`$PLATFORM` values, `dl_iterate_phdr` counters, placeholder functions' behaviour; is `-z now` the norm on the reference's `/usr/bin` (readelf `BIND_NOW` count) | GATE-14 | none | L | `spec/behaviour/dynlink.md` + `.glibc.txt`; settles threat draft TBDs |
| M14-1 | **Placeholder libraries** + `libc_malloc_debug.so.0` (empty + compat state functions) generated from versions.tsv; xtask check that each export sits in the library versions.tsv names | GATE-14 | R14-12 | L | `target/liella/*.so.N`; readelf checks of sonames, version nodes |
| M14-2 | **IP-24 full ELF reader** in `liella_ld::elf` (headers, phdrs, dynamic, hash tables, versions, RELA/RELR iterators, notes), bounds-checked, typed errors; malformed corpus + fuzz target | GATE-14 | none | L | readelf-parity tests on system-linker corpus |
| M14-3 | **`ld.so` product + bootstrap**: `crates/ld-so`, boundary feature `ld`, entry, self-relocation, provisional TB, auxv/env/secure/tunables/CPU reuse, `ld.so --help/--version`, runs a `-nostdlib` PIE with no dependencies; xtask link checks (L14-1); static-PIE start in `libc.a` via the same relocator | M14-2 | R14-1, R14-2, R14-8 (entry transfer only) | H | 100 randomized-base starts green; planted RELR bug caught |
| M14-4 | **Mapper + search + cache + hwcaps**: reservation mapping, permissions, RELRO hook, search order, DSTs, pinned Liella sonames, `ld.so.cache` format inspection (Appendix D) and reader, `--list` / `LD_TRACE_LOADED_OBJECTS` | M14-3, M14-0 | R14-3 | M | ldd lists match the reference for non-Liella libs; corrupt-cache corpus |
| M14-5a | **Symbols, versions, relocation** (eager, non-TLS): GNU/SysV hash, scopes, versions, IFUNC/IRELATIVE ordering, COPY, TEXTREL, RELRO, init/fini order, entry transfer; runs `-nostdlib` test programs with several shared objects | M14-4 | R14-13 | H | relocation-type corpus, order corpus vs reference |
| M14-5b | **libc.so.6 under our ld.so**: `LIELLA_PRIVATE` early init, startup table of design §14, export placement moves (R14-12), `__libc_start_main` dynamic path; the native dynamic example and a C hello world run | M14-5a, M14-1 | R14-12 | H | `native_dynamic` example runs; conformance subset linked dynamically |
| M14-6 | **TLS**: static layout + surplus, dtv, all TLS relocations, `__tls_get_addr`, TLSDESC (static + fallback trampoline), `__liella_tls_*` for `pthread_create` (closes D-22.10), dlopen into surplus | M14-5b | R14-4, R14-5, R14-8 | H | TLS model matrix; threads before/after dlopen; `cbench_tls` rows vs reference |
| M14-7 | **dlopen family**: dlopen/dlclose/dlsym/dlvsym/dlerror/dladdr/dladdr1/dlinfo, all `RTLD_*` modes, refcounts, unique symbols, `RTLD_NEXT`, RCU snapshot + grace period, `dl_iterate_phdr`, `_dl_find_object`, differences row | M14-6 | R14-6 | H | the 13 s27_1 clause tests; concurrent dl_iterate_phdr stress |
| M14-8 | **Lazy binding + unmodified binaries**: PLT trampoline (XSAVE/FXSAVE variants), LD_BIND_NOW/NOT, RTLD_LAZY late definitions; run host `/usr/bin` programs via `ld.so PROG` and in a mount namespace | M14-7 | R14-8, R14-14 | M | phase exit item 1 (unmodified binaries run) |
| M14-9 | **Namespaces + debugger rendezvous**: dlmopen, `r_debug` v2, `_dl_debug_state`, `DT_DEBUG`; GDB batch tests | M14-7 | R14-11 | M | s27_1_c_2, s27_4_c_0 |
| M14-10 | **libthread_db** (40 td_* exports) on our layout; GDB thread listing, TLS printing, attach | M14-6, M14-9 | none | M | §22.10 thread_db clauses |
| M14-11 | **Audit + profiling**: LD_AUDIT/DT_AUDIT, all la_* callbacks, symbind redirect, PLT enter/exit trampolines, LD_PROFILE/_dl_mcount | M14-8, M14-9 | R14-8, R14-10 | H | s27_2_c_0; audit log equals the reference's |
| M14-12 | **NSS modules (F27)**: module service kind, trusted-directory loading, `__nss_hostname_digits_dots` export | M14-7 | none | M | test module + host module smoke |
| M14-13 | **Secure-mode and robustness pass + exit**: threat-model rules 1-8 tests (set-group-ID corpus with our PT_INTERP), malformed ELF/cache corpus, startup syscall count vs reference (`strace -c`), dynamic bench column, phase-14 exit checklist (LD_LIBRARY_PATH/LD_PRELOAD/LD_AUDIT work) | all above | R14-15, R14-16 | M | phase 14 exit |

## Dependency order

```
M14-0 ─┐
M14-1 ─┼──────────────────────────────┐
M14-2 ─▶ M14-3 ─▶ M14-4 ─▶ M14-5a ─▶ M14-5b ─▶ M14-6 ─▶ M14-7 ─┬▶ M14-8 ─┐
                   ▲ (M14-0)                                     ├▶ M14-9 ─┼▶ M14-10 (needs 6, 9)
                                                                 └▶ M14-12 └▶ M14-11 (needs 8, 9)
                                                         all ─▶ M14-13
```

M14-0, M14-1 and M14-2 can run in parallel at GATE-14. M14-8, M14-9 and M14-12 can run in parallel
after M14-7.

## Size and risk

15 runs (16 rows; M14-0 and M14-1 are small). Critical path: 0/2 → 3 → 4 → 5a → 5b → 6 → 7 → 8/9
→ 10/11 → 13 = 11 sequential runs. Highest risk: M14-3 (bootstrap correctness before any test
harness exists inside the process), M14-5b (startup ordering with a real libc), M14-6 (TLS with
live threads), M14-7 (RCU grace periods). Each high-risk run should get the checkpoint early and
hand back a WIP rather than overrun. The libm track must be complete before phase 14 exits
(overview).
