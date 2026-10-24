<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 14: Dynamic linking

* **Scope:**
  * §27, `ld-linux-x86-64.so.2`: `dlopen` and friends, namespaces, audit, the debugger
    rendezvous, TLS descriptors, IFUNC, RELR, symbol-version resolution, and hwcaps search;
  * §22.10, `libthread_db` for our thread layout (F30);
  * NSS plugin modules (F27); modules call `__nss_hostname_digits_dots` (§16.1, compat entry
    written 2026-10-05 by owner ruling), which Liella exports for them;
  * the placeholder libraries (`libpthread.so.0`, `libdl.so.2`, `librt.so.1`,
    `libutil.so.1`, `libanl.so.1`) with their version nodes, and an empty
    `libc_malloc_debug.so.0` (owner ruling 2026-10-04: libc itself provides `mcheck`/`mtrace`,
    D-3.5; preloading the placeholder gives no hooks, and the documentation says so; it also
    carries the compat `malloc_get_state`/`malloc_set_state`, phase-15 ledger);
  * before the loader is written, the loader and environment sections of
    `spec/security/threat-model.md` (owner, 2026-10-04).
* **Symbols:** 17 plus the `td_*` set.
* **Clauses:** 19 plus thread_db.
* **Needs:** all of P1–P13.
* **Builds:** IP-24 (the full ELF reader), the loader, and the library cache (the
  `ld.so.cache` format is inspected and recorded in Appendix D). Calling constructors and
  entry points uses `machine/call.S` (OD-C1). Mapped segments reach the safe loader as
  category-*slice* conversions.
* **Test surface:**
  * Loading-order and constructor-order corpora (test ELF objects built by the test suite
    with the system linker).
  * `RTLD_*` modes.
  * `dlsym` `RTLD_NEXT` interposition.
  * Unique symbols.
  * TLS in dlopened objects.
  * `dl_iterate_phdr` during concurrent dlopen.
  * GDB driving our `libthread_db`.
* **Exit:** unmodified dynamically linked test binaries run.
  `LD_LIBRARY_PATH`/`LD_PRELOAD`/`LD_AUDIT` work.

#### Stories (PHASE-14-PLAN, 2026-10-23)

The design is `plan/ph14/design.md` (PH14-DESIGN, 2026-10-09, PROPOSED), its rulings R14-n are
in `plan/ph14/open-questions.md` (none ruled yet), its threat-model text is
`plan/ph14/threat-model-draft.md`. This list splits the 16 milestones of `plan/ph14/milestones.md`
(sized at 60-100 calls) into stories of one 50-call run and replaces that table for dispatch.

**What exists today** (checked at `42e682a3`): `crates/ld` (`liella-ld`, `forbid(unsafe)`) and
`crates/thread-db` are module skeletons of 3 lines each; `s27_dynlink.rs` exports only the
static-program `dl_iterate_phdr` (D-27.4), `_dl_find_object` (D-27.5) and
`__x86_get_cpuid_feature_leaf` (D-27.6); `dlopen`, `dlsym`, `dlclose`, `dlerror`, `dladdr*`,
`dlinfo`, `dlmopen`, `dlvsym`, `_r_debug`, `__tls_get_addr` are not exported (the
`s27_dynlink_symbols.rs` rows wait). Static TLS (variant II) and the thread block are in
`rt/src/tls.rs` (glibc's `tcbhead_t` prefix kept, D-22.10); startup steps 1-6 in
`rt/src/start.rs`. `cargo xtask so` already builds `libc.so.6`, `libm.so.6` and an empty
`libpthread.so.0` (no version nodes); `libdl`, `librt`, `libutil`, `libanl` and
`libc_malloc_debug` do not exist. Dynamic programs link against `libc.so.6` but cannot run;
tests that need it report a skip (`s10_math_symbols.rs`, `__fpu_control`, #814). Static PIE is
refused (phase-4 deviation). D-27.3 says search reads the *system* `ld.so.cache`; R14-3
recommends a prefix cache: the ruling settles which, and D-27.3 is rewritten to match.

**Every story is security-relevant** (the loader runs before the program in every dynamic
process, set-user-ID ones included): each one adds malformed-input cases to its own tests,
puts any glibc weakness only in SECURITY-PRIVATE, and gets plants on its bounds checks.
Marked **HIGH** below: the ones where a mistake is a memory-safety or trust-boundary hole, or
a startup that cannot be debugged from inside. Every implementation story needs the boundary
`unsafe` audit (`cargo xtask static`) and the K2 rule (no logic in assembly).

**The gate.** No implementation story (DL-1 onwards) starts before the owner has reviewed the
saturation register (SATURATION-REGISTER) and V-6 (`PRE14-GATE-REPORT`) has landed. DL-0a to
DL-0d are docs and probes: they may run now. The rulings each story needs are named; a story
whose ruling is still open is not dispatched.

**Proof common to all implementation stories.** Host-side unit tests of the `liella-ld` logic
over objects the test suite builds with the system linker (no process loads our `ld.so` for
these); process tests that run our `ld.so` as a child (the test harness itself always runs on
the host's C library, R14-17); a black-box comparison with the reference loader wherever the
behaviour is observable (dlerror text, order, exit status), recorded in
`spec/behaviour/dynlink.md`; plants on the new code with `msg:` fields. The INVIOLABLE workload
gate is static: no phase-14 story may slow a static program (only DL-4 touches the static start).

**Arch seam (A-1 `ARCH-PH14-NEUTRAL`, RL-5 provisional).** x86_64 stays the only implementation.
Every implementation story puts what it adds per architecture behind `liella_ld::arch::Arch` /
`boundary/src/arch/` (design §0a, L14-13). It ticks the §0a review checklist in its handoff
(`arch seam: ticked`, or each exception with its reason). A review finds a missed tick
cheaply; finding a hard-coded `R_X86_64_*` later costs an A-story a rewrite. The stories
below name the seam items they add (**seam:**).

No code (may run now):

1. **DL-0a `DL-PROBE-SECURE`** (M, no deps, HIGH). Black-box set-group-ID probes for the
   threat-model open questions: the `LD_PRELOAD`/`LD_AUDIT` exception, `$ORIGIN` in secure mode,
   empty path elements, `/etc/ld.so.preload`, a corrupt cache (read-only observation of the
   reference with a private cache through `ld.so --library-path`/namespaces, never by changing
   `/etc`), `LD_BIND_*` residue, `TZ`. Proof: `spec/behaviour/dynlink-secure.md` + `.glibc.txt`;
   the threat-draft TBDs filled. Needs R14-18 (how a set-group-ID program is made).
2. **DL-0b `DL-PROBE-BEHAVIOUR`** (M, no deps). dlerror texts per failure, loader exit status
   and messages, the constructor/destructor order corpus run against the reference (the corpus
   becomes DL-11's oracle), the static-TLS surplus (largest IE-TLS `dlopen` that succeeds),
   `$LIB`/`$PLATFORM` values, `dl_iterate_phdr` counters across dlopen/dlclose, the placeholder
   libraries' functions, the `BIND_NOW` share of the reference's `/usr/bin`. Proof:
   `spec/behaviour/dynlink.md` + `.glibc.txt`, each line from a probe program in `tools/probe/`.
3. **DL-0c `DL-CACHE-FORMAT`** (S, no deps). Inspect `ld.so.cache` as data (files written by
   the host's `ldconfig -C <private file>` over a scratch directory, plus ldconfig(8)), record the
   format in spec Appendix D. Inspection of files only, no loader source.
4. **DL-0d `DL-THREAT-TEXT`** (S, after DL-0a). Fold the owner-approved `threat-model-draft.md`
   (R14-15) into `spec/security/threat-model.md` "The dynamic loader" with the probe answers;
   the rules become DL-29's test checklist. Must land before DL-8.

Implementation:

5. **DL-1 `DL-PLACEHOLDERS`** (S, no deps, R14-12). `libpthread.so.0` (now with its version
   nodes), `libdl.so.2`, `librt.so.1`, `libutil.so.1`, `libanl.so.1` and an empty
   `libc_malloc_debug.so.0` with `malloc_get_state`/`malloc_set_state`, generated from
   `versions.tsv`; an xtask check that each export sits in the library versions.tsv names.
   Proof: readelf checks of sonames and version nodes; a program linked with `-ldl -lrt` links.
   **seam:** the export list and version nodes are read from versions.tsv; no x86_64 base node is hard-coded.
6. **DL-2 `DL-ELF-READ-1`** (M, no deps). IP-24 part 1 in `liella_ld::elf`: ELF header, program
   headers, dynamic section, notes; bounds-checked, typed errors. Proof: readelf parity over the
   system-linker corpus; a truncated/overlapping-header corpus. **seam:** creates
   `liella_ld::arch` (the `Arch` trait, `X86_64`, `Native`) with the ELF identity; the reader
   checks `e_machine` only through it.
7. **DL-3 `DL-ELF-READ-2`** (M, after DL-2). Hash tables (GNU, SysV), `versym`/`verdef`/`verneed`,
   RELA and RELR iterators; the `elf` fuzz target. Proof: parity with readelf `-V`/`-r`;
   malformed corpus; a short fuzz run with zero crashes. **seam:** the RELA iterator yields
   `RelocKind` through `Arch::classify` (unknown types are `Unsupported(n)`).
8. **DL-4 `DL-BOOT-RELOC`** (M, after DL-3, HIGH, R14-1). The self-relocator (RELATIVE + RELR,
   `boundary/src/bootstrap.rs`, category *slice*), first used where a real process can test it:
   the static-PIE start of `libc.a` (closes the phase-4 deviation, IRELATIVE included). Proof:
   static-PIE conformance programs; 100 starts at random bases; plant "skip one RELR word" must
   fail; the static (non-PIE) workload count unchanged. **seam:** the relocator matches
   `RelocKind::Relative`/`Irelative` and RELR, never raw type numbers.
9. **DL-5 `DL-LDSO-PRODUCT`** (M, after DL-4, HIGH, R14-2, R14-8 entry). `crates/ld-so` product,
   boundary feature `ld`, the `ld` variant of `entry.S`, provisional thread block, the xtask link
   checks (L14-1: no undefined symbols, no `DT_NEEDED`, relocations ⊆ RELATIVE/RELR, no
   TEXTREL); `ld.so --version`/`--help` run. Proof: link checks with a plant for each; the
   program runs at random bases. **seam:** product name from `Arch::INTERP`;
   `boundary/src/arch/` with `set_thread_pointer` (provisional block) and the
   `liella_machine_ld_entry` declaration; the L14-1 check over `RelocKind`.
10. **DL-6 `DL-LDSO-RUN`** (M, after DL-5, HIGH). auxv/env, the secure decision and
    `UNSECURE` clearing before any `LD_*` read, tunables and CPU features; load and enter a
    `-nostdlib` PIE with no dependencies (by `PT_INTERP` and as `ld.so PROG`). Proof: 100
    randomized starts both ways; a secure-mode child (R14-18) sees `LD_*` cleared.
    **seam:** `Arch::features(&Auxv)` is the only source of CPU features.
11. **DL-7 `DL-MAP`** (M, after DL-6, HIGH). Reservation mapping, segment permissions exactly as
    requested, `p_align`, the RELRO hook, unmapping on failure. Proof: `/proc/self/maps` checks;
    malformed-object corpus (overlapping, huge, misaligned segments) refused without a crash.
12. **DL-8 `DL-SEARCH-CACHE`** (M, after DL-7, DL-0a, DL-0c, DL-0d; HIGH; R14-3, R14-15). Search
    order, DSTs, pinned Liella sonames, the cache reader, hwcaps subdirectories, `--list` /
    `LD_TRACE_LOADED_OBJECTS`; secure-mode rules; a `cache` fuzz target. Proof: `ldd` lists equal
    the reference's for non-Liella libraries; corrupt-cache corpus. **seam:** default
    directories, `$LIB`, `$PLATFORM`, the hwcaps subdirectory list and the cache-entry arch
    flag come from the `Arch` table.
13. **DL-9 `DL-SYMBOLS`** (M, after DL-3; parallel to DL-4 to DL-8). Lookup: GNU/SysV hash,
    scopes, versions (default, hidden, base), `STB_GNU_UNIQUE`, weak rules. Pure logic. Proof:
    host-side tests over a versioned-symbol corpus; plants on version matching.
14. **DL-10 `DL-RELOC`** (M, after DL-7 and DL-9, HIGH, R14-13). Eager relocation of every
    non-TLS x86-64 type, IFUNC/IRELATIVE ordering, COPY, TEXTREL policy, RELRO, `PT_GNU_STACK`
    refusal; `-nostdlib` programs with several shared objects run. Proof: relocation-type corpus
    (one object per type), plants on addend/bias arithmetic. **seam:** `Arch::classify` and
    `Arch::apply` for the non-TLS kinds; which TEXTREL widths exist is a table entry; the
    `PT_GNU_PROPERTY` notes are parsed per arch.
15. **DL-11 `DL-INIT-ORDER`** (M, after DL-8 and DL-10). Init/fini order (preinit, dependencies
    first, `DT_INIT` + `INIT_ARRAY`), destructors at exit, entry transfer. Proof: DL-0b's order
    corpus equal to the reference.
16. **DL-12 `DL-TLS-STATIC`** (M, after DL-10, HIGH, R14-4). Static TLS layout of the initially
    loaded objects with the surplus, final thread block (canary, pointer guard), dtv, `TPOFF`/
    `DTPMOD`/`DTPOFF` relocations, `__tls_get_addr` for static modules. Proof: TLS model matrix
    (LE/IE/GD/LD) over initially loaded objects. **seam:** the layout takes `Arch`'s TLS
    variant, TCB size and placement as parameters; the canary and pointer-guard slots and the
    TLS kinds' bias come from the table (A-4 then adds variant I as host tests).
17. **DL-13 `DL-LIBC-EARLY`** (M, after DL-11, DL-12, DL-1; R14-12). `LIELLA_PRIVATE` with the
    version-mismatch refusal (L14-10), `__liella_libc_early_init`, rseq publishing, the export
    moves (`__libc_stack_end`, rseq objects to `ld.so`). Proof: a mismatched pair is refused with
    the diagnostic; constructors see an initialised libc (`memcpy(NULL, ..)` from
    `.preinit_array`, design §14).
18. **DL-14 `DL-LIBC-START`** (M, after DL-13, HIGH). The dynamic path of `__libc_start_main`
    (old and new crt1), the `native_dynamic` example and a C hello world, a dynamic conformance
    subset; the #814 skips lifted (`fpu_control_*` dynamic on Liella). Risk of overrun: split
    point is "hello world runs" / "conformance subset".
19. **DL-15 `DL-TLS-THREADS`** (M, after DL-14, HIGH, R14-8). `__liella_tls_layout/init/release`
    for `pthread_create` (closes D-22.10's dynamic `EAGAIN`), TLSDESC static return and
    fallback trampoline. Proof: threads with TLS in libc and a library; forced-variant and
    planted-bug tests for each trampoline. **seam:** `liella_machine_ld_tlsdesc_static` /
    `_dynamic` as fixed-name primitives, with the register convention in the file header.
20. **DL-16 `DL-DLOPEN`** (M, after DL-14). `dlopen`/`dlsym`/`dlvsym`/`dlerror`, all `RTLD_*`
    modes, `dlopen(NULL)`, `RTLD_DEFAULT`/`RTLD_NEXT`, unique symbols; no unloading yet. Proof:
    s27_1_c_1*, c_3, c_5, f_0 clause tests; dlerror texts equal DL-0b's.
21. **DL-17 `DL-DLCLOSE-RCU`** (M, after DL-16, HIGH). Reference counts, `dlclose` with
    destructors and unmapping, the RCU snapshot and grace period, `dl_iterate_phdr` and
    `_dl_find_object` over the live list (replacing D-27.4/D-27.5 for dynamic programs), the
    R14-6 differences row. Proof: concurrent dlopen/dlclose/`dl_iterate_phdr` stress in RAM;
    plant "unmap before the grace period" must fail.
22. **DL-18 `DL-DLADDR-INFO`** (S, after DL-16). `dladdr`, `dladdr1` (both requests), `dlinfo`
    (every request). Proof: s27_1_c_6, c_7 tests against the reference's answers.
23. **DL-19 `DL-TLS-DLOPEN`** (M, after DL-15 and DL-17, HIGH, R14-4, R14-5). TLS of dlopened
    objects: surplus placement, dynamic blocks from the loader's arena, lazy `__tls_get_addr`,
    threads created before and after the dlopen, release at dlclose. Proof: s27_3 tests;
    `cbench_tls` rows against the reference.
24. **DL-20 `DL-LAZY`** (M, after DL-16, HIGH, R14-8). The PLT trampoline (XSAVE and FXSAVE,
    selected once), `LD_BIND_NOW`/`LD_BIND_NOT`, `RTLD_LAZY` late definitions. Proof: vector
    and x87 registers preserved across a first call (forced variants); plant drops one register.
    **seam:** `liella_machine_ld_lazy*` primitives, `Arch::trampoline_variant(&Features)`, and
    the reserved GOT slots / PLT index convention from the table.
25. **DL-21 `DL-UNMODIFIED`** (M, after DL-19 and DL-20, R14-14). Host `/usr/bin` programs via
    `ld.so PROG` and in a user + mount namespace; `LD_LIBRARY_PATH`/`LD_PRELOAD`; the dynamic
    validation items T17 (`stdbuf`), T27 (make's `load`), T33 (gawk extensions). Proof: phase
    exit item 1 on a named program list.
26. **DL-22 `DL-NAMESPACES`** (M, after DL-17, R14-11). `dlmopen`, `LM_ID_NEWLM`, per-namespace
    scopes. Proof: s27_1_c_2.
27. **DL-23 `DL-RDEBUG`** (S, after DL-17; parallel to DL-22). `_r_debug` v2, `_dl_debug_state`,
    `DT_DEBUG`. Proof: s27_4_c_0; GDB batch test lists the objects and stops at a dlopen.
28. **DL-24 `DL-THREAD-DB-1`** (M, after DL-15 and DL-23). `libthread_db.so.1`: agent, thread
    list, LWP mapping, event breakpoints. Proof: s22_10_c_0/1/2/5 with GDB batch.
29. **DL-25 `DL-THREAD-DB-2`** (M, after DL-24 and DL-19). TLS addresses, TSD values, attach to
    a running process. Proof: s22_10_c_3/4; GDB prints a `__thread` variable of a dlopened library.
    **seam:** TLS addresses use the same `Arch` layout parameters as DL-12 (no TP offset of its own).
30. **DL-26 `DL-AUDIT`** (M, after DL-20 and DL-22, HIGH, R14-8). `LD_AUDIT`/`DT_AUDIT`, all `la_*`
    callbacks except the PLT pair, `la_symbind` redirection. Proof: s27_2_c_0; the audit log equals
    the reference's.
31. **DL-27 `DL-AUDIT-PLT`** (M, after DL-26, HIGH, R14-8, R14-10). `la_pltenter`/`la_pltexit`
    trampolines, `LD_PROFILE`/`_dl_mcount`. Proof: argument and return registers seen and
    changed by the auditor; forced-variant tests. **seam:** `liella_machine_ld_audit_plt_*`
    primitives; the per-arch `la_<arch>_gnu_pltenter`/`pltexit` names and register structures
    (rtld-audit(7)) are chosen through the table.
32. **DL-28 `DL-NSS-MODULES`** (M, after DL-16). F27: module service kind, loading from the
    trusted directory only, the `__nss_hostname_digits_dots` export. Proof: a test module and a
    host module smoke test; secure mode loads no module from an untrusted path.
33. **DL-29 `DL-SECURE-PASS`** (M, after DL-21, DL-26, DL-28; HIGH; R14-18). The threat-model rules
    as tests (set-group-ID corpus with our `PT_INTERP`), a fuzz campaign of the `elf` and `cache`
    targets. Proof: every rule has a test; campaign numbers in `docs/fuzzing.md`.
34. **DL-30 `DL-EXIT`** (M, after all; R14-16). Startup syscall count against the reference
    (`strace -c`), the dynamic bench column, the exit checklist, `xtask status`/symcov §27 at 0
    pending, the differences rows, README and the Delivered entry.

**Order and parallel lanes.** Now: DL-0a, DL-0b, DL-0c in parallel, then DL-0d. After the gate:
DL-1 and DL-2 in parallel; after DL-3 two lanes, *loader* DL-4 → 5 → 6 → 7 → 8 and *symbols*
DL-9, joining at DL-10. Then DL-10 → (DL-11 ‖ DL-12) → DL-13 → DL-14 → (DL-15 ‖ DL-16). After DL-16:
DL-17, DL-18, DL-20 and DL-28 in parallel; after DL-17: DL-22 ‖ DL-23. The critical path is
DL-2 → 3 → 4 → 5 → 6 → 7 → 8 → 10 → 11 → 13 → 14 → 16 → 17 → 19 → 21 → 29 → 30: 17 sequential
runs of 34 stories. HIGH stories should checkpoint early and hand back a WIP rather than overrun.
The libm track must be complete before phase 14 exits (overview).

**RULING NEEDED** (new ones; the open R14-1 to R14-16 stand as written in `open-questions.md`):

* **R14-17 How the loader is tested before it can load itself.** Recommend a ladder, each rung
  green before the next: (1) `liella-ld` logic as host-side unit tests over system-linker objects
  (no process involved); (2) the relocator first in static-PIE `libc.a` (DL-4), where an ordinary
  test program exercises it at random bases; (3) `ld.so` run as a program on `-nostdlib` PIEs;
  (4) corpus programs linked with `--dynamic-linker=<our ld.so in target/liella>`; (5) host
  binaries by explicit invocation and in a user + mount namespace (R14-14). The test harness,
  cargo and xtask always run on the host's C library and only spawn children under our loader,
  so a broken loader fails tests, never the harness.
* **R14-18 Set-group-ID probes and tests without root.** Secure mode needs an effective group
  other than the real one. The `claude` user has only the groups `claude` and `users`; a
  set-group-ID file of group `users` run with real group `claude` gives `AT_SECURE` without root
  (to confirm in DL-0a). Recommend that; if it does not hold, the owner approves one exact root
  command (`chown`/`chmod g+s` on one probe file under `~claude`) rather than any standing sudo.
* **R14-19 Order of the gate items.** Recommend that DL-0a to DL-0d (probes and docs) run before
  the saturation-register review, and that the gate holds back only DL-1 onwards, as this list does.
