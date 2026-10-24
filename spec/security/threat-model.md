# Threat model

Liella's trust boundaries: for each, the attacker, what they control, and what they must not be
able to achieve. This document is completed in the security pass after phase 15
(`implementation-plan.md`, "Security pass"); the sections the dynamic loader (phase 14) depends on,
the environment and secure-execution mode, are written first, before phase 14 starts.

Status: started 2026-10-03 with the environment-variable list; the dynamic loader's section
written 2026-10-04, before phase 14 (owner). Other boundaries (the name-service cache region,
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
| `GLIBC_TUNABLES` | tunables (D-1.8; `glibc.malloc.perturb` from phase 13) | cleared at start; no tunable is honoured | probe |
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
| `TZ` | time-zone selection | kept and honoured (a zone name or rule; `:path` names any file) | to be reviewed with the loader |
| `MALLOC_CHECK_`, `LD_USE_LOAD_BIAS`, other names | not read | kept (the reference keeps them too) | probe |

Liella's own security-sensitive variables named in the ruling (`HOSTALIASES`, `RES_OPTIONS`,
`LOCALDOMAIN`, `TMPDIR`, `TZDIR`, `LOCPATH`, `GCONV_PATH`, `NLSPATH`) are all on the reference
library's list, so clearing them is no difference from glibc. A variable Liella starts to read in
a later phase is added here, with its treatment in a secure process, in the same change.

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

**What must not be possible.**
1. **In a secure process, the environment must not choose code or data.** The ld.so(8) variables
   are cleared before anything else runs (the table above; already done for static programs).
   Beyond clearing, the loader itself ignores, in secure mode: `LD_LIBRARY_PATH`, `LD_PRELOAD`
   (except, as ld.so(8) documents, names without a slash found in the trusted directories and
   marked set-user-ID: the exact rule to be settled by probing), `LD_AUDIT` (same rule),
   `LD_DEBUG_OUTPUT`, `LD_PROFILE`, `LD_ORIGIN_PATH`, `LD_HWCAP_MASK`, `GLIBC_TUNABLES` (no tunable
   honoured), and any `--library-path`/`--preload`-style option that reaches it.
2. **`$ORIGIN` and other dynamic string tokens must not let a secure program load from an
   attacker-chosen place.** In secure mode a `DT_RPATH`/`DT_RUNPATH` element using `$ORIGIN`,
   `$LIB` or `$PLATFORM` is honoured only where ld.so(8) allows it (expansion limited to trusted
   directories), and a relative element (not starting with `/` after expansion) or an empty
   element is ignored. The exact rules are to be established from the documentation and a
   set-group-ID probe before implementation.
3. **No search path may silently mean the current directory in a secure process,** and relative
   `dlopen` names (containing a slash but not starting with one) are resolved against the current
   directory only as documented.
4. **Parsing never trusts lengths, offsets or counts:** every program header, segment, dynamic
   entry, relocation, symbol, version and hash table access is bounds-checked against the mapped
   file, before mapping anything executable; a malformed object is refused with a diagnostic, never
   a crash in the loader. Segments are mapped with exactly the permissions their headers request
   (never writable and executable at once; `PT_GNU_STACK` honoured; `PT_GNU_RELRO` made read-only
   after relocation).
5. **The cache is untrusted input** (`/etc/ld.so.cache`, format to be inspected and recorded in
   Appendix D, as the locale files were): a corrupt or truncated cache means falling back to the
   default directories, never an out-of-bounds read.
6. **IFUNC resolvers and constructors run only after relocation is complete and with the process
   in a consistent state**; audit modules (`LD_AUDIT`, `DT_AUDIT`) are loaded only where rule 1
   allows.
7. **NSS modules (F27)** are loaded only from the trusted directories by their fixed names
   (`libnss_<service>.so.2`), the service name being a plain identifier (no `/`, no `.`/`..`), and
   never in a way an environment variable can redirect in a secure process.
8. **Secure-process consistency with the C library's own rules:** whatever the loader clears or
   ignores, libc's start code (static programs) does the same, from the one list in
   `rt::start::UNSECURE`, so static and dynamic programs behave alike.

**Open questions for the phase-14 behaviour documents** (probes, not decisions): the exact
`LD_PRELOAD`/`LD_AUDIT` exception for secure programs; `$ORIGIN` in secure mode; the behaviour for
an empty `LD_LIBRARY_PATH` element; how `/etc/ld.so.preload` interacts with secure mode; what the
reference does with a corrupt cache; whether `LD_BIND_NOW`/`LD_BIND_NOT` (cleared at start) have any
residual effect; `dlopen` of a set-user-ID library by a non-secure process.

**How it will be tested:** a set-group-ID copy (as `tests/secure_env.rs` does, no root needed) for
every rule of 1–3 and 7; a corpus of malformed ELF files and caches, fuzzed in the security pass,
for 4–5; each finding gets a test or a documented limitation.
