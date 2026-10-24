<!-- Phase 14 open questions (PH14-DESIGN, 2026-10-09). RULING NEEDED items carry a recommendation;
     INFORMATIONAL items need no answer. Local decisions taken without asking: design.md §21. -->

# Phase 14: rulings needed

Ordered by how early they block (milestone in brackets).

| ID | Question | Options | Recommendation | Blocks |
|---|---|---|---|---|
| R14-1 | **How does `ld.so` relocate itself?** Rust cannot run safely before its own `RELATIVE` relocations are applied; K2 forbids loops in assembly. | (a) a Rust relocator in `boundary/src/bootstrap.rs` (category *slice*: `&mut` view of the loader's own writable segment), callable before relocation because `ld.so` has only RELATIVE/RELR relocations and the relocator touches no relocated data; (b) an assembly loop in `machine/` (K2 exception); (c) link `ld.so` at a fixed address (ET_EXEC interpreter; no ASLR for the loader) | **(a)**, with the xtask link checks of L14-1 and the randomized-base test; the same relocator serves static-PIE `libc.a` (closes a phase-4 deviation). One new boundary file, no new unsafe category. | M14-3 |
| R14-2 | **Separate `ld.so` or `libc.so.6` as its own interpreter?** | (a) separate small `ld.so` (no imports, provable bootstrap, a private interface to libc, a second small heap for link maps); (b) one object for both names: one runtime, no private interface, one heap, but a ~4 MB libc with interposable symbols must bootstrap itself, and the loader path must never call an interposable export | **(a)**: the bootstrap invariant ("only RELATIVE relocations, no imports") is checkable for a small object and not for libc; the costs of (a) are a private interface of ~12 functions and a few pages per process. | M14-3 |
| R14-3 | **Install and search model.** The host `/etc/ld.so.cache` maps `libc.so.6` to the host's glibc. | (a) compile-time prefix; Liella's sonames pinned to `<prefix>/lib` before any search; cache at `<prefix>/etc/ld.so.cache` (= `/etc/ld.so.cache` when prefix is `/`); (b) honour the system cache for everything (breaks when prefix ≠ `/`); (c) environment-configured prefix (unsafe in secure mode) | **(a)**. Also: `LD_LIBRARY_PATH` cannot override the pinned names even outside secure mode? Recommend *yes, it can* outside secure mode (documented behaviour, useful for testing a libc build), never in secure mode. | M14-4 |
| R14-4 | **Static TLS surplus** (observable: how large an initial-exec-TLS library `dlopen` accepts; memory per thread). | (a) the reference's measured value; (b) larger, e.g. 64 KiB of *virtual* space per thread (untouched pages cost nothing until a module is placed there); (c) also place every dlopened module's TLS in the surplus while it fits (static TLSDESC fast path, but dlopen initialises the block in every live thread: O(threads)) | **(b)** with (c) as a measured experiment after M14-6: larger only makes programs that fail on the reference succeed. Cost per thread: 64 KiB virtual, 0 resident until used; per process: none. | M14-6 |
| R14-5 | **Where dynamic TLS blocks come from.** Spec §27.3 says "through malloc"; an interposed `malloc` would then see (and could recurse into) TLS allocation. | (a) the loader's own mmap-backed arena; (b) the interposable `malloc` | **(a)**: no recursion through a malloc that itself uses TLS; observable only to a malloc interposer counting calls. Needs a differences row. | M14-6 |
| R14-8 | **New machine primitives for the loader** (OD-C1 extension; each one fixed sequence, no logic): program entry transfer (set `%rsp`, `%rdx`, jump), TLSDESC static return (2 instructions), TLSDESC fallback (save all, call Rust, restore), lazy-binding trampoline in XSAVE and FXSAVE variants (selected once, CPU-variant ruling), audit PLT enter/exit trampolines | approve all / approve per milestone | **Approve as a set**, each with a forced-variant test and a planted-bug test; they are the only way to meet the psABI calling conventions. | M14-3 (entry), M14-6, M14-8, M14-11 |
| R14-10 | **`la_pltexit`** needs the caller's stack arguments copied into a new frame (`framesize` from `la_pltenter`): a copy loop. | (a) Rust prepares the copied frame, the trampoline only switches to it (no logic in asm); (b) asm loop (K2 exception); (c) do not support `la_pltexit` (documented limitation) | **(a)**; fall back to (c) with a ruling if (a) proves unsound. Low priority (audit users are rare). | M14-11 |
| R14-11 | **Number of link-map namespaces.** | (a) no fixed cap (P-lift, C6: no correct program depends on the limit); (b) the reference's limit | **(a)**: costs nothing (namespaces are allocated on demand). Observable: `dlmopen(LM_ID_NEWLM)` succeeds where the reference fails. | M14-9 |
| R14-12 | **Export placement and private names** (public interface). Today `libc.so.6` exports `__libc_stack_end` and the rseq objects; versions.tsv places them in `ld.so`. | (a) move them to `ld.so` as versions.tsv says, `libc.a` keeps them; GLIBC_PRIVATE names (`_rtld_global*`, `_dl_*`, libc's `_thread_db_*`) are not exported; our interface is `LIELLA_PRIVATE`; (b) export GLIBC_PRIVATE stand-ins | **(a)**; revisit (b) only if the binary-compat sweep (M14-13) finds a third-party binary that needs one. | M14-1, M14-5b |
| R14-13 | **Executable-stack objects** (`PT_GNU_STACK` with X). | (a) refuse with dlerror (and at startup for the program); (b) follow the reference [PROBE M14-0] and make thread stacks executable | **(a)**, with a differences row if the reference accepts: making every stack executable for one library is a security regression. | M14-5a |
| R14-14 | **How the gate runs unmodified binaries** (phase exit: "unmodified dynamically linked test binaries run"). Their `PT_INTERP` is the host's loader. | (a) `ld.so PROG` explicit invocation for the functional sweep, plus a user+mount namespace (`unshare -rm`, bind-mount our prefix over `/lib64` and the lib dirs) for fidelity runs; (b) chroot (needs root; refused for agents) | **(a)**; secure-mode tests use corpus programs built with our `--dynamic-linker` (explicit invocation cannot set `AT_SECURE`). | M14-8 |
| R14-15 | **Threat-model loader/environment text** (`threat-model-draft.md`, DRAFT FOR OWNER) | owner writes/approves | Approve the draft's B/C rules as the test checklist; probe placeholders filled by M14-0 before M14-4 starts. | M14-4 |
| R14-16 | **Benchmark basis for dynamic programs.** Today benches compare static Liella with the reference. | (a) add a dynamic column (both sides call through the PLT) and keep static rows; (b) replace | **(a)**; parity for HOT rows is then judged on both columns. | M14-13 |

## INFORMATIONAL (no answer needed)

* **R14-6, D-27.1 vs clause s27_1_c_8.** The spec says the loader lock is held during the
  `dl_iterate_phdr` callback; D-27.1 decided a lock-free snapshot. Behaviour difference: callbacks
  run concurrently with `dlopen` in other threads and may call `dlopen` themselves without
  deadlock; an object is never unmapped under a running callback (grace period). A
  `docs/differences-from-glibc.md` row is added in M14-7.
* **R14-7, lazy binding.** Implemented for real (design §12) rather than "eager plus late-bound
  stubs", because a function defined by a later `RTLD_GLOBAL` dlopen must be found at first call
  and the audit PLT callbacks need the same trampoline. Only the primitive itself is in R14-8.
* **R14-9, CET.** Shadow-stack/IBT properties are parsed and ignored (L14-12): enabling them
  needs shadow-stack support in setjmp, contexts and thread start. A later ledger item.
* **Hot-path dispatch** (D-4.25, D-4.14, FN-ALIGN) survives unchanged under our loader
  (design §14); in an x86-64-v3 hwcaps build the flags could fold to constants: an experiment
  row for the optimization register, not a phase-14 item.
* **Experiment rows to file** (out-of-the-box, not phase-14 work): persistent per-machine symbol
  resolution cache (trust/invalidation problem); 2 MiB-aligned `libc.so.6` text with
  `MADV_HUGEPAGE` (iTLB on hot rows); R14-4(c) "all dlopened TLS in the surplus".
* **Spec gap noted, not changed:** §27.3 names "malloc" as the source of lazy TLS blocks; if
  R14-5 (a) is ruled, the spec text is a three-way-rule candidate (spec vs probe vs docs) for
  M14-6.
