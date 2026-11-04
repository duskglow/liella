# Threat model

Liella's trust boundaries: for each, the attacker, what they control, and what they must not be
able to achieve. This document is completed in the security pass after phase 15
(`implementation-plan.md`, "Security pass"); the sections the dynamic loader (phase 14) depends on,
the environment and secure-execution mode, are written first, before phase 14 starts.

Status: started 2026-10-03 with the environment-variable list; the dynamic loader's section
written 2026-10-04, before phase 14 (owner), its rules made checkable and extended to 12 on
2026-11-04 (R14-15, DL-0d), and the environment section's loader additions approved the same
day (#2661, DL-0d section A). Other boundaries (the name-service cache region,
the secure allocator, credential changes, the C layer and the boundary crate, pattern and word
expansion, parsers of system files and network data) are written in the security pass.

## Environment variables in secure-execution mode

**Attacker:** the user who runs a set-user-ID, set-group-ID or file-capability program (the
kernel sets `AT_SECURE`), or any program whose real and effective IDs differ.
**Controls:** the whole environment the program starts with, and the current directory.
**Must not achieve:** make the privileged program read files, libraries, data or configuration of
the attacker's choosing, change what it resolves or reports, or learn anything through it that the
attacker could not learn directly.

**Two mechanisms.** (1) **Clearing at start** (owner ruling 2026-10-03): in a secure process
Liella's start code removes every entry whose name is on the list below from the initial
environment array, all occurrences, before constructors, `main` or any other code can read it
(`rt::start::UNSECURE`, applied by `boundary::runtime` in step 2 of the start sequence), so
neither the program nor a child it executes sees them. (2) **Ignoring on read:** where Liella
itself reads a variable that names a file, a directory or behaviour a privileged program must not
take from its caller, it reads it through `secure_getenv` (or checks `AT_SECURE`), which also
covers variables a program sets itself after start.

**Who clears, and when, in a dynamically linked program** (owner approval #2661, 2026-11-04,
from DL-0d section A): the dynamic loader applies the same `rt::start::UNSECURE` list during its
bootstrap, after it decides secure mode and before it reads any `LD_*` variable or loads any
object. The libc pass in `__libc_start_main` stays and is then a no-op. One list, two call sites,
the same code. **Secure mode is `AT_SECURE` only** in the products, static and dynamic; the
hosted-build rule (real and effective IDs differ when the auxiliary vector cannot be read) stays
in the tests only.

**How the list was established** (2026-10-03; not from any library's source): the ld.so(8) man
page on the reference system ("Secure-execution mode" lists 22 names), and a black-box probe: a
program printing the names left in `environ`, built dynamically and statically against the
reference library, made set-group-ID with a supplementary group of the user (the kernel then
sets `AT_SECURE`, no root needed), and run with `env -i` and 90 candidate names set: the 22, every
other `LD_*`, `MALLOC_*`, `RESOLV_*` and tunable name known from documentation, the variables
Liella reads, common variables (`PATH`, `HOME`, `IFS`, `LANG`, `LC_ALL`, `TZ`, `PWD`, ...) and
near-miss names (`TMPDIRX`, `LD_`, `LD_LIBRARY_PATH_X`). Both builds removed the same 34 names, by
exact name and every occurrence (a repeated `TMPDIR` was removed twice); nothing else. A name
absent from the candidate list could not be observed: the list covers what documentation names.

| Variable | Read by (Liella) | In a secure process | Source |
|---|---|---|---|
| `GCONV_PATH` | not read (iconv uses the installed charmaps, D-6.8) | cleared at start | ld.so(8), probe |
| `GETCONF_DIR` | not read | cleared at start | ld.so(8), probe |
| `GLIBC_TUNABLES` | tunables (D-1.8; `glibc.malloc.perturb` from phase 13); the dynamic loader (phase 14) | cleared at start, so it is not in the environment passed on; no tunable is honoured by the loader or libc (D-1.8 filter) | probe; DL-0a (`spec/behaviour/dynlink-secure.md`): the reference removes it too |
| `HOSTALIASES` | the resolver (`res_hostalias`, host lookups) | cleared at start; ignored on read (`secure_getenv`) | ld.so(8), probe; D-16.3 |
| `LD_AUDIT`, `LD_DEBUG`, `LD_DEBUG_OUTPUT`, `LD_DYNAMIC_WEAK`, `LD_HWCAP_MASK`, `LD_LIBRARY_PATH`, `LD_ORIGIN_PATH`, `LD_PRELOAD`, `LD_PROFILE`, `LD_SHOW_AUXV` | the dynamic loader (phase 14) | cleared at start | ld.so(8), probe |
| `LD_BIND_NOT`, `LD_BIND_NOW`, `LD_VERBOSE`, `LD_WARN` | the dynamic loader (phase 14) | cleared at start | probe |
| `LOCALDOMAIN`, `RES_OPTIONS` | the resolver configuration | cleared at start; ignored on read | ld.so(8), probe; D-16.3 |
| `LOCPATH` | locale loading | cleared at start; ignored on read | ld.so(8), probe |
| `MALLOC_TRACE` | `mtrace` (§3.3, phase 13): names the trace file, created (`O_CREAT`, `O_TRUNC`, mode 0666 less the umask) when the program calls `mtrace` | cleared at start; ignored on read (`secure_getenv`): a secure process never creates or truncates a file its caller's environment names, even when the variable is set after start (`conformance/c/s03_mtrace_secure.c`) | ld.so(8), probe |
| `MALLOC_PERTURB_` | the start code (§3.2 `M_PERTURB`, phase 13): the fill byte of new and released blocks of the C `malloc` family; the tunable `glibc.malloc.perturb` overrides it | cleared at start, so never honoured (no tunable is honoured either) | mallopt(3), probe |
| `MALLOC_ARENA_MAX`, `MALLOC_ARENA_TEST`, `MALLOC_MMAP_MAX_`, `MALLOC_MMAP_THRESHOLD_`, `MALLOC_TOP_PAD_`, `MALLOC_TRIM_THRESHOLD_` | not read (no neutral meaning for this heap, D-3.4) | cleared at start | probe |
| `NIS_PATH` | not read (NIS, phase 15) | cleared at start | ld.so(8), probe |
| `NLSPATH` | `catopen` (§7.8) | cleared at start; ignored on read (`secure_getenv`) | ld.so(8), probe |
| `LANGUAGE` | the gettext family (§7.7): the catalog languages | kept (not on the reference's list); ignored on read (`secure_getenv`): a secure process translates by its `LC_MESSAGES` locale only | owner ruling 2026-10-04; probed with a set-group-ID copy: the reference keeps `LANGUAGE` (and `LANG`, `LC_*`) in the environment of a secure process, so it is not added to the clearing list |
| `LANG` | `setlocale(…, "")`; `catopen` (§7.8) with flag 0 names the catalog's locale (`%L`, `%l`, `%t`, `%c`, and the default templates `/usr/share/locale/%L/%N` …) | kept and honoured; a value containing `/`, or `.` or `..` as a whole, is never substituted into a path (the template is skipped), so it cannot name a catalog outside the template's directory | phase 13 decision (D-7.6; the reference substitutes it unchecked) |
| `RESOLV_HOST_CONF` | not read | cleared at start | ld.so(8), probe |
| `TMPDIR` | `tmpfile`, `tempnam`, the temporary-name functions | cleared at start; ignored on read | ld.so(8), probe |
| `TZDIR` | time-zone selection | cleared at start; ignored on read | ld.so(8), probe |
| `PWD` | `get_current_dir_name` | **kept**; ignored on read (the physical path is returned) | D-13.14 (owner ruling) |
| `LIELLA_NSCACHE` | the name-service cache | kept; ignored on read (the cache stays private in secure mode anyway) | D-16.10 |
| `LIELLA_AIO_URING` | AIO engine (`0` forces worker threads) | kept; ignored on read (secure processes use threads anyway) | D-11.11 (owner ruling) |
| `TZ` | time-zone selection | kept; a zone name or a POSIX rule is honoured; a path (absolute, or `:path`) is honoured only when it lies inside the default zone directory `/usr/share/zoneinfo`; a path outside it, or any value with a `..` component, reads no file and gives offset 0 (abbreviated, as any value that is neither a zone nor a rule, by its leading valid name, empty if none: the DL-0a table); `TZDIR` is cleared, so a relative name always resolves under the default directory | owner approval #2661 (2026-11-04, DL-0d section A); DL-0a probe (`spec/behaviour/dynlink-secure.md`): the reference does the same, so no differences row |
| `LD_ASSUME_KERNEL`, `LD_PROFILE_OUTPUT`, `LD_TRACE_LOADED_OBJECTS` | the dynamic loader (phase 14) | kept in the environment (not on the reference's list); the loader does not act on them in a secure process (loader rule 1): with `LD_TRACE_LOADED_OBJECTS` set, even empty, the program runs normally | ld.so(8); DL-0a probe: the reference keeps them and does not act on them |
| `MALLOC_CHECK_`, `LD_USE_LOAD_BIAS`, other names | not read | kept (the reference keeps them too) | probe |

Liella's own security-sensitive variables named in the ruling (`HOSTALIASES`, `RES_OPTIONS`,
`LOCALDOMAIN`, `TMPDIR`, `TZDIR`, `LOCPATH`, `GCONV_PATH`, `NLSPATH`) are all on the reference
library's list, so clearing them is no difference from glibc. A variable Liella starts to read in
a later phase is added here, with its treatment in a secure process, in the same change. Every
variable the dynamic loader reads that ld.so(8) lists has a row above (owner approval #2661); a
loader variable not listed here is added in the change that makes the loader read it.

## Defaults ruled by the owner (2026-10-04)

* **AIO and io_uring** (D-11.11): io_uring is never tried in a secure-execution process or under a
  seccomp filter, since a filter that kills on an unlisted call would kill the process at
  `io_uring_setup` and no fallback could catch it. Detection reads the calling thread's `Seccomp:`
  line in `/proc/thread-self/status` first (trusted only on procfs, by `fstatfs`): any value but 0
  is a filter, and `prctl` is not called. Only when procfs says 0 or cannot be read is
  `prctl(PR_GET_SECCOMP)` asked, and anything but 0 (an error included) is a filter. A filter that
  kills on `prctl` itself, or answers it `ERRNO(0)`, is therefore covered whenever procfs is
  readable; the limit remains for a process without a readable procfs. Programs and administrators can force threads with `liella_aio_use_threads`
  (`LIELLA_NATIVE_1`) or `LIELLA_AIO_URING=0`. A kernel refusal of io_uring falls back silently
  (tested through the injected-failure kernel). `SIGEV_THREAD` notifications on a caller-supplied
  stack (AIO, timers, message queues) run one at a time on it, so requests completing together
  cannot corrupt each other's frames (the reference starts them at once on the one stack).
* **Locale names** (D-7.7, D-7.12): values containing `/`, or equal to `.` or `..`, and derived
  candidates that are empty, `.` or `..`, are never used as path components in locale, catalog
  (gettext, catopen) or charmap lookup. In a secure process `LOCPATH` is cleared but `LANG`,
  `LC_*` and `LANGUAGE` are kept (as the reference keeps them), so this rule is what keeps them from
  steering file access; `LANGUAGE` is also ignored there.
* **Hosted builds' secure mode** (tests only): when `/proc/self/auxv` is unreadable, differing real
  and effective IDs count as secure. The products read the startup vector.

## Findings recorded before the security pass

Found by the validation stage (2026-10-04, `validation/README.md` T12, T13), fixed in the same
stage; the security pass writes the cache region's section around them.

* **Library descriptors on the standard numbers.** The name-service cache kept its region's
  descriptor at the lowest free number. A program started with its standard output closed (`id
  >&-`) got the region as descriptor 1 and wrote its output into the region every process of the
  user maps, instead of failing with `EBADF`. Every descriptor the library keeps across calls
  (the region, the syslog socket, the utmp file, the `lckpwdf` lock, database cursors, `mtrace`'s
  file, the timers' epoll set and `timerfd`s, the message queues' netlink socket, the AIO ring
  and eventfd) is now moved above 2 (`F_DUPFD_CLOEXEC`) before use.
* **User namespaces and `/dev/shm`.** The region is named and owner-checked with the IDs the
  process sees. In a user namespace mapping UID 0 to user 1001, a process created
  `liella-nscache-0` owned by 1001 in the shared `/dev/shm` (a real root process refused it, by
  the owner check, but the name was squatted and the two views of "UID 0" mixed). A process
  outside the initial user namespace now keeps a private table. A region whose header an
  attacker wrote is rejected or reinitialised on attach (layout, size cap, owner, boot ID, account
  stamp, seqlock and checksums: the `corrupt-*` and `squat-*` checks of `nscache_products.rs`).

## The dynamic loader (written before phase 14)

`ld-linux-x86-64.so.2` (§27, phase 14) runs before any of the program's code, in every
dynamically linked process, including set-user-ID ones. Its behaviour is described from the
ld.so(8), ld.so.conf/ldconfig(8), dlopen(3), rtld-audit(7) and elf(5) documentation and from
black-box probes, never from any loader's source.

**Attackers and what they control.**
* **The caller of a secure program** (`AT_SECURE`: set-user-ID, set-group-ID, file capabilities,
  an LSM): the whole environment (`LD_*`, the variables of the table above), the arguments, the
  current directory, open descriptors, resource limits, and the files it owns, including
  directories it can write that a search path might name.
* **A local user against other users' programs:** files in writable directories (`/tmp`, a
  shared project directory) that a relative `DT_RPATH`/`DT_RUNPATH`, an empty search-path
  element (which means the current directory) or a relative `dlopen` name could reach.
* **Whoever writes the program's own files:** out of scope (they already control the program).
  Likewise root-owned configuration (`/etc/ld.so.cache`, `/etc/ld.so.conf`,
  `/etc/ld.so.preload`, the trusted directories): trusted, but parsed as untrusted input (a
  corrupted cache must not crash the loader or make it map something it should not).
* **A malformed object** (any ELF file the loader is asked to map): headers, program headers,
  dynamic section, relocations, version records, notes and the hash tables are untrusted input.

**What must not be possible.** Rules 1-3 and 5-7 in the checkable form and the new rules 9-12
were approved by the owner on 2026-11-04 (R14-15, ruling #2549); rules 4 and 8 are the 2026-10-04
text. Facts about the reference come from black-box probes: `spec/behaviour/dynlink-secure.md`
(DL-0a) unless another source is named. A *secure process* is one whose auxiliary vector has
`AT_SECURE` set: that is the only test in the products, static and dynamic.

1. **In a secure process, the environment must not choose code or data.** The ld.so(8) variables
   are cleared before anything else runs (the table above; already done for static programs).
   Beyond clearing, the loader itself ignores `LD_LIBRARY_PATH`, `LD_ORIGIN_PATH`,
   `LD_DEBUG_OUTPUT`, `LD_PROFILE`, `LD_HWCAP_MASK`, `GLIBC_TUNABLES` (no tunable is honoured),
   and every command-line option of `ld.so` run as a program (a secure process never reaches that
   mode: the kernel sets `AT_SECURE` for the executed file, which is then the program, not
   `ld.so`). Every entry of `LD_LIBRARY_PATH` is ignored: absolute, relative and empty elements
   alike (probe: the reference ignores all three). `LD_PRELOAD` and `LD_AUDIT`: an entry is used
   only if it contains no `/`, is found in a trusted directory (rule 5) and the file has the
   set-user-ID bit; every other entry is skipped. Probe: every entry containing a `/` is skipped
   without a message, even a file in a trusted directory; a bare name of a trusted-directory
   library without a set-ID bit is not loaded, and the program runs (`LD_PRELOAD` prints
   `object '<name>' from LD_PRELOAD cannot be preloaded (cannot open shared object file):
   ignored.`; `LD_AUDIT` skips it without a message). **Open:** whether a set-user-ID or
   set-group-ID bit on a trusted-directory file admits the entry. Observing it needs such a file
   in a system directory, which needs root; that probe waits for the owner's say-so, and until
   then no rule depends on the answer. `/etc/ld.so.preload` is root-owned configuration and is
   honoured, in a secure process too (probe: the reference loads an untrusted absolute path named
   there in both modes).
2. **`$ORIGIN` and other dynamic string tokens must not let a secure program load from an
   attacker-chosen place.** In a secure process: a `DT_RPATH`/`DT_RUNPATH` element containing
   `$ORIGIN` is used only if the program's own directory is a trusted directory (probe: with the
   program in an untrusted directory the element is not used, for `DT_RUNPATH` and `DT_RPATH`
   alike; a program in a trusted directory needs root to place, so that case is not observed);
   `$LIB` and `$PLATFORM` expand to fixed strings, with no environment input; a `dlopen` name
   containing a token is refused (probe: `dlopen("$ORIGIN/lib/x.so")` fails with `cannot open
   shared object file`, no expansion); any element that is relative after expansion, or empty, is
   skipped. Outside secure mode `$ORIGIN` is the directory of the requesting object's resolved
   path (for the program the kernel's `AT_EXECFN`, not `/proc/self/exe`), or `LD_ORIGIN_PATH`
   when that cannot be determined.
3. **No search path may silently mean the current directory in a secure process.** An empty
   element and a relative element of any search list are skipped. Outside secure mode they mean
   the current directory, as documented. A `dlopen` name with a `/` that does not start with `/`
   is opened relative to the current directory in both modes (documented behaviour, and the
   program chose it; probe: the reference does the same in both modes).
4. **Parsing never trusts lengths, offsets or counts:** every program header, segment, dynamic
   entry, relocation, symbol, version and hash table access is bounds-checked against the mapped
   file, before mapping anything executable; a malformed object is refused with a diagnostic, never
   a crash in the loader. Segments are mapped with exactly the permissions their headers request
   (never writable and executable at once; `PT_GNU_STACK` honoured; `PT_GNU_RELRO` made read-only
   after relocation). For a `PT_GNU_STACK` that asks for an executable stack, rule 10 applies.
5. **Trusted directories and the cache.** The trusted directories are the compiled-in default
   directories of the build (`<prefix>/lib`, and, when the prefix is `/`, the system's `/lib64`,
   `/usr/lib64`, `/lib/x86_64-linux-gnu` and `/usr/lib/x86_64-linux-gnu`) and nothing else.
   `ld.so.conf` directories are *searched* through the cache but are not trusted for rules 1 and
   2. **The cache is untrusted input** (`/etc/ld.so.cache`, format recorded in Appendix D, as the
   locale files were): a corrupt or truncated cache means falling back to the default
   directories, never an out-of-bounds read. Probe: with the cache missing, empty, 4096 bytes of
   garbage, cut to 16 bytes or cut to half, the reference starts the program, exit 0, without a
   message, and a library known only through the cache is not found; a cache naming a library
   outside the default directories is used by a secure program too.
6. **Order guarantee.** No IFUNC resolver, constructor or audit callback runs before every object
   of its load set is relocated and its `PT_GNU_RELRO` range is read-only, and the debugger
   rendezvous (`_r_debug.r_state`) shows `RT_CONSISTENT`. Audit modules are loaded only as rule 1
   allows; a `DT_AUDIT` entry in a secure program goes through the same filter as `LD_AUDIT`
   (probe: an `LD_AUDIT` absolute path to a real audit module is skipped without a message).
7. **NSS modules (F27)** are loaded only from the trusted directories by their fixed names
   (`libnss_<service>.so.2`), the service name being a plain identifier (no `/`, no `.`/`..`), and
   never in a way an environment variable can redirect in a secure process: the module path is
   built only from a trusted directory and that name, and `LD_LIBRARY_PATH` never applies to it in
   a secure process (follows from rule 1).
8. **Secure-process consistency with the C library's own rules:** whatever the loader clears or
   ignores, libc's start code (static programs) does the same, from the one list in
   `rt::start::UNSECURE`, so static and dynamic programs behave alike.
9. **Liella's own libraries cannot be swapped by the environment in a secure process.** The
   pinned sonames (`libc.so.6`, `libm.so.6`, the placeholder libraries, the loader) resolve to the
   build's prefix before any search (R14-3). Outside secure mode `LD_PRELOAD` can still interpose
   symbols, as documented.
10. **No writable and executable mappings, and no executable stack on request.** An object whose
    `PT_GNU_STACK` asks for an executable stack (`PF_X` set) is refused (R14-13): `dlopen` fails
    with a `dlerror` message and maps nothing, and a program or startup dependency that asks for
    one is refused at startup with a diagnostic. No stack is ever made executable. This is a
    deliberate difference: probe (`dl-threat-text`, Ubuntu GLIBC 2.39-0ubuntu8.9, objects linked
    with `-z execstack`), the reference loads such a library and afterwards the main stack and a
    thread stack created before the `dlopen` are both mapped `rwxp`; a program linked that way
    starts with `rwxp` stacks. Recorded in `docs/differences-from-glibc.md`; registered way back:
    COMPAT-19 in `docs/compat-flags.md`, ignored in a secure process (as COMPAT-12).
11. **`dlerror` and diagnostics never print attacker-sized data unbounded:** paths and symbol
    names in messages are truncated to a fixed cap, and `LD_DEBUG` output in a secure process goes
    nowhere. Probe: in a secure process the reference ignores `LD_DEBUG`, `LD_SHOW_AUXV` (also
    when empty) and `LD_TRACE_LOADED_OBJECTS` (also when empty; the program runs normally): nothing
    on stdout or stderr, no `LD_DEBUG_OUTPUT` file.
12. **A loader and libc from different builds are refused** (L14-10), so a secure program can
    never run with a libc whose private interface it does not match.

**Still open** (probes, not decisions): the set-ID bit in rule 1 (needs root; owner's say-so);
`$ORIGIN` for a program that is itself in a trusted directory (rule 2; needs root to place);
whether `LD_BIND_NOW`/`LD_BIND_NOT` change binding in a secure program (the reference removes
both from the environment; any residual effect is not observed); `dlopen` of a set-user-ID library
by a process that is not secure. The environment-section additions of the same draft
(`plan/ph14/threat-model-draft.md` section A: `TZ`, `GLIBC_TUNABLES` removal, the loader's new
variables) were approved separately (#2661, 2026-11-04) and are in "Environment variables in
secure-execution mode" above.

**How it will be tested (DL-29 `DL-SECURE-PASS` checklist).** Secure mode is made without root
as `tests/secure_env.rs` and the DL-0a probe do: a copy of each corpus program with mode 2755 and
a group that is a supplementary group of the caller, linked with `--dynamic-linker` naming the
build's `ld.so` (running `ld.so PROG` cannot test secure mode: the kernel sets `AT_SECURE` from
the executed file). Every rule needs its test before DL-29 closes:

| Rule | Test it needs |
|---|---|
| 1 | Set-group-ID program with `LD_LIBRARY_PATH` (absolute, empty and relative elements), `LD_PRELOAD` and `LD_AUDIT` (absolute path, untrusted and trusted directory; bare name without a set-ID bit) and `GLIBC_TUNABLES`: no listed object is loaded, the `LD_PRELOAD` bare-name message is printed and the program runs; a private `/etc/ld.so.preload` (user and mount namespace) is honoured. The set-ID-bit case stays untested until the root probe is ruled |
| 2 | `DT_RUNPATH` and `DT_RPATH` with `$ORIGIN/lib` from an untrusted directory: not used in secure mode, used in plain mode; `dlopen` of a `$ORIGIN` name refused in secure mode; `$LIB`/`$PLATFORM` give the same expansion with any environment; plain-mode `$ORIGIN` follows `AT_EXECFN` |
| 3 | Empty and relative `DT_RUNPATH`/`LD_LIBRARY_PATH` elements with the library in the current directory: skipped in secure mode, found in plain mode; `dlopen("lib/x.so")` loads from the current directory in both modes |
| 4 | Malformed ELF corpus (truncated headers, out-of-range offsets and counts, overlapping segments, a writable and executable `PT_LOAD`): refused with a diagnostic, no crash; fuzz target `elf` in the security pass; mapped permissions read back from `/proc/self/maps`, `PT_GNU_RELRO` read-only after relocation |
| 5 | Cache corpus (missing, empty, garbage, cut to 16 bytes, cut to half, out-of-range string offsets): the program starts and falls back to the default directories; fuzz target `cache`; an `ld.so.conf` directory does not count as trusted for rules 1 and 2 |
| 6 | An IFUNC resolver and a constructor in a multi-object load set that check every relocation of the set is applied, the `PT_GNU_RELRO` range is read-only and `_r_debug.r_state` is `RT_CONSISTENT`; `DT_AUDIT` in a secure program filtered as `LD_AUDIT` |
| 7 | A secure program resolving through an NSS service with `LD_LIBRARY_PATH` naming a directory with a fake `libnss_<service>.so.2`: the trusted module is used; a service name with `/`, `.` or `..` loads nothing |
| 8 | One environment corpus run against a static and a dynamic secure program: the same names are removed (one list, `rt::start::UNSECURE`) |
| 9 | Secure program with `LD_PRELOAD`/`LD_LIBRARY_PATH` naming a fake `libc.so.6` and `libm.so.6`: the build's own libraries are loaded; plain mode: `LD_PRELOAD` interposition still works |
| 10 | `dlopen` of a library linked with `-z execstack`: fails with a `dlerror` message and leaves no mapping; a program linked with `-z execstack` is refused at startup; no stack is ever `rwx` in `/proc/self/maps` |
| 11 | Secure program with `LD_DEBUG`, `LD_DEBUG_OUTPUT`, `LD_SHOW_AUXV` and `LD_TRACE_LOADED_OBJECTS` (set and empty): nothing on stdout or stderr, no output file, the program runs; a `dlopen` of an over-long path and symbol name gives a `dlerror` string no longer than the cap |
| 12 | A loader started with a libc of another build (mismatched private-interface stamp): refused at startup with a diagnostic |
