<!-- DRAFT FOR OWNER (PH14-DESIGN, 2026-10-09). The owner writes and approves the loader and
     environment sections of spec/security/threat-model.md; nothing here is in force until then.
     Sources: ld.so(8), ld.so.conf/ldconfig(8), dlopen(3), rtld-audit(7), elf(5), the current
     threat-model.md text. Items marked [PROBE M14-0] are filled from black-box set-group-ID probes. -->

# DRAFT FOR OWNER: loader and environment sections

The current `threat-model.md` already has the attackers list and rules 1-8 for the loader
(owner, 2026-10-04). This draft proposes (A) additions to the environment section, (B) a more
precise version of rules 1-3, 5-7 that the implementation and tests can check one by one, and (C)
new rules 9-12 the design surfaced. Rules 4 and 8 are unchanged.

## A. Environment section: additions

1. **Who clears, and when (dynamic programs).** In a dynamically linked secure process the loader
   applies the `rt::start::UNSECURE` list during its bootstrap, after deciding secure mode from
   `AT_SECURE` and before reading any `LD_*` variable or loading any object. libc's own pass at
   `__libc_start_main` stays and is then a no-op. One list, two call sites, same code.
2. **Secure mode is `AT_SECURE` only** in the products (static and dynamic). The hosted-build
   rule (differing IDs when auxv is unreadable) stays tests-only.
3. **`GLIBC_TUNABLES`** in a secure process: no tunable is honoured by the loader or libc
   (D-1.8 filter). [Probe (DL-0a, `spec/behaviour/dynlink-secure.md`): the reference also
   *removes* the variable from the environment it passes on; the table row says so.]
4. **`TZ`** (currently "to be reviewed with the loader"): proposed treatment in a secure process —
   a zone *name* or POSIX rule is honoured, a `:path` or absolute path is honoured only inside the
   default zone directory. [Probe (DL-0a): the reference does exactly this. A name, a rule and an
   absolute path inside the default zone directory are honoured; a path outside it, or a name
   through `..`, gives offset 0; `TZDIR` is removed. No differences row is needed.]
5. **New variables the loader reads** get a row each in the table, in the same change:
   `LD_LIBRARY_PATH`, `LD_PRELOAD`, `LD_AUDIT`, `LD_BIND_NOW`, `LD_BIND_NOT`, `LD_DEBUG`,
   `LD_DEBUG_OUTPUT`, `LD_PROFILE`, `LD_PROFILE_OUTPUT`, `LD_ORIGIN_PATH`, `LD_HWCAP_MASK`,
   `LD_TRACE_LOADED_OBJECTS`, `LD_WARN`, `LD_VERBOSE`, `LD_SHOW_AUXV`, `LD_DYNAMIC_WEAK`,
   `LD_ASSUME_KERNEL` (ld.so(8) lists them). `LD_PROFILE_OUTPUT` and `LD_TRACE_LOADED_OBJECTS`
   are not on the current list. [Probe (DL-0a): the reference keeps both in the environment
   (also `LD_ASSUME_KERNEL`, `LD_USE_LOAD_BIAS`, `MALLOC_CHECK_`) but does not act on them; with
   `LD_TRACE_LOADED_OBJECTS` set, even empty, the secure program runs normally.] So the loader
   ignores them in secure mode (rule 1) and leaves them in the environment, as the reference does.

## B. Loader rules 1-3, 5-7, made checkable

1. **Environment must not choose code or data (secure mode).** The loader ignores
   `LD_LIBRARY_PATH`, `LD_ORIGIN_PATH`, `LD_DEBUG_OUTPUT`, `LD_PROFILE`, `LD_HWCAP_MASK`,
   `GLIBC_TUNABLES`, and every command-line option of `ld.so` run as a program (a secure process
   never reaches that mode: the kernel only sets `AT_SECURE` for the executed file, which is then
   the program, not `ld.so`). `LD_PRELOAD` and `LD_AUDIT`: an entry is used only if it contains
   no `/`, is found in a *trusted directory* (B.5) and the file has the set-user-ID bit; every
   other entry is skipped. [Probe (DL-0a): every entry with a `/` is skipped silently, even a
   trusted-directory file. A bare name of a trusted-directory library without the bit is not
   loaded: `LD_PRELOAD` prints `object '<name>' from LD_PRELOAD cannot be preloaded (cannot open
   shared object file): ignored.` and the program runs; `LD_AUDIT` skips it silently. Whether a
   set-user-ID or set-group-ID bit admits an entry needs root to observe: not settled.]
   `/etc/ld.so.preload` is root-owned configuration and is honoured [probe: also in secure mode,
   an untrusted absolute path included].
2. **Dynamic string tokens.** In a secure process: an `RPATH`/`RUNPATH` element containing
   `$ORIGIN` is used only if the program's own directory is a trusted directory [probe (DL-0a):
   in an untrusted program directory the element is not used, for `DT_RUNPATH` and `DT_RPATH`
   alike; a program in a trusted directory needs root to observe], `$LIB` and `$PLATFORM` expand to fixed strings (no environment input), a
   `dlopen` *name* containing a token is refused, and any element that is relative after
   expansion, or empty, is skipped. Outside secure mode `$ORIGIN` is the directory of the
   requesting object's resolved path (`/proc/self/exe` is not consulted for the program; the
   kernel's `AT_EXECFN` is), or `LD_ORIGIN_PATH` when that cannot be determined.
3. **No implicit current directory.** In a secure process, an empty element in any search list
   and a relative element are skipped. Outside secure mode they mean the current directory, as
   documented. A `dlopen` name with a `/` that does not start with `/` is opened relative to the
   current directory in both modes (documented behaviour; the program chose it).
5. **Trusted directories** are the compiled-in default directories of the build
   (`<prefix>/lib`, and the system's `/lib64`, `/usr/lib64`, `/lib/x86_64-linux-gnu`,
   `/usr/lib/x86_64-linux-gnu` when the prefix is `/`) and nothing else; `ld.so.conf` directories
   are *searched* through the cache but are not trusted for rules 1-2. The cache is untrusted
   input (unchanged rule 5 text applies).
6. **Order guarantee:** no IFUNC resolver, constructor or audit callback runs before every object
   of its load set is relocated and its `RELRO` is read-only, and the debugger rendezvous shows
   `RT_CONSISTENT`. Audit modules: only as rule 1 allows; `DT_AUDIT` in a secure program is
   subject to the same filter as `LD_AUDIT`.
7. **NSS modules** (unchanged text) plus: the module path is built only from a trusted directory
   and `libnss_<service>.so.2`; `LD_LIBRARY_PATH` never applies to it in a secure process
   (follows from rule 1).

## C. New rules

9. **Liella's own libraries cannot be swapped by the environment in a secure process.** The
   pinned sonames (`libc.so.6`, `libm.so.6`, placeholders, the loader) resolve to the build's
   prefix before any search (R14-3). Outside secure mode `LD_PRELOAD` can still interpose
   symbols, as documented.
10. **No writable and executable mappings, and no executable stacks on request.** An object whose
    `PT_GNU_STACK` asks for an executable stack is refused (R14-13, recommended), rather than
    making every thread stack executable.
11. **`dlerror` and diagnostics never print attacker-sized data unbounded:** paths and symbol
    names in messages are truncated to a fixed cap; `LD_DEBUG` output in a secure process goes
    nowhere [probe (DL-0a): the reference ignores `LD_DEBUG`, `LD_SHOW_AUXV` and
    `LD_TRACE_LOADED_OBJECTS` in a secure process: nothing on stderr or stdout, no
    `LD_DEBUG_OUTPUT` file].
12. **A loader/libc pair from different builds is refused** (L14-10), so a secure program can
    never run with a libc whose private interface it does not match.

## How it will be tested (proposed)

A set-group-ID copy of each corpus program, built with `--dynamic-linker` pointing at the
build's `ld.so` (running `ld.so PROG` explicitly cannot test secure mode, because the kernel
sets `AT_SECURE` from the executed file); one test per rule 1-3, 7, 9-12; the malformed ELF and
cache corpora for 4-5 (fuzzed in the security pass); rule 6 by an IFUNC resolver and a
constructor that assert relocation completeness and read `_r_debug.r_state`.
