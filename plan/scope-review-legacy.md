<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

# Scope review: legacy, obsolete and insecure-by-design interfaces (SCOPE-REVIEW-LEGACY, 2026-11-04)

Owner, 2026-10-27, after the Sun RPC ruling (LEGACY-RPC-NIS-STUBS): every interface family that is obsolete,
deprecated, removed from glibc's public API or insecure by design gets an explicit decision before more legacy
work is built. glibc dropping something is a strong reason to decide, never the decision itself. **This page
changes no code. Every recommendation below is RULING NEEDED.**

## Sources and evidence

* **Candidates:** spec §29 (29.1 to 29.4), §30 H12 ("interfaces that embed insecure designs"), the phase 15 plan
  (`plan/06-phases/18-phase-15-legacy-tier-and.md`), `docs/spec-symbol-gaps.md`, and the man pages' deprecation
  notes. Manifest status (`default` / `compat`) is `spec/abi/versions.tsv`; "in Liella" is
  `docs/codemap/symbols.tsv` plus a search of `crates/` and `csys/`.
* **Users (black box only):** `legacy_users.py` (scratch, job `scope-review-legacy-usage`, 2026-11-04)
  read the undefined dynamic symbols (`readelf -W --dyn-syms`) of all 2,867 x86-64 ELFs under `/usr/bin`,
  `/usr/sbin`, `/usr/lib*`, `/usr/libexec` and `/opt` on a Xeon Platinum 8151 build host. It mapped each importing file to its package with
  `dpkg -S` and left out glibc's own packages (`libc6`, `libc-bin`, ...). The **Users** column gives
  ELFs/packages that import the name, followed by examples. Earlier result to compare: the LG-0c sweep
  (`validation/results/bincompat/README.md`) found **no compat `sym@VER` bound by any non-glibc binary** on
  two other build hosts.
* **Limits:** a server image, about 300 packages. It cannot see static links, `dlsym` lookups or the
  wider Debian archive, so a 0 means "no user seen", not "no user". I did not use Debian Code Search because its
  results include glibc's own source (clean room). The binary-compatibility corpus (LG-19, R15-8) is the evidence
  that DROP would need, so no row here recommends DROP for a name glibc 2.39 exports.

Recommendation kinds (brief): **IMPLEMENT** (as now); **COMPAT-ONLY** (export for old binaries, behave as
specified, not declared in Liella's headers for new links); **STUB** (export, fail cleanly, as
LEGACY-RPC-NIS-STUBS); **DROP** (not exported). The symbol and version stay as they are under COMPAT-ONLY. Only
the header declaration goes, so binaries see no ABI difference. Whether a missing header declaration is a
difference from glibc that needs a row in `docs/differences-from-glibc.md` is item R0.

## Table

Risk: **I** insecure by design, **U** unbounded buffer, **R** racy or global state, **O** merely old. Users column: ELFs/packages.

| # | Family | Manifest | Users (Xeon host) | Risk | In Liella | Recommend |
|---|---|---|---|---|---|---|
| 1 | `gets` (removed from C11; man page: never use) | default | 0/0 | I, U | yes, `s12_stdio.rs` | COMPAT-ONLY |
| 2 | `tmpnam`, `tmpnam_r` (ISO C; man page: use `mkstemp`/`tmpfile`) | default | 0/0 | R (name-then-open) | yes, `s12_stdio.rs` | IMPLEMENT, header deprecation warning |
| 3 | `tempnam`, `mktemp` | default | tempnam 1/1 (libnewt); mktemp 2/2 (bash, busybox) | R | yes | IMPLEMENT (in use) |
| 4 | DES `crypt`/`crypt_r`, `encrypt`/`setkey`(`_r`) | **absent from the 2.39 manifest** | crypt 10/7, crypt_r 12/5, all bound to `libcrypt.so.1` (package `libcrypt1`, not glibc); encrypt/setkey 0 | I (DES, 8-char keys) | not exported (`des.rs` serves RPC only) | DROP (already absent): `libcrypt` stays outside Liella |
| 5 | RPC DES helpers (`des_setparity`, `ecb_crypt`, `cbc_crypt`, `xencrypt`, `passwd2des`) | compat (§29.1) | 0/0 | I | partly (`des.rs`, 302 lines) | STUB, under LEGACY-RPC-NIS-STUBS |
| 6 | `utmp`/`wtmp` readers and writers (`getutent`, `pututline`, `updwtmp`, `login`, `logout`, `logwtmp`) | default | setutent 6/6, getutent 4/4, pututline 3/3 (login, screen, libutempter), login/logout/logwtmp 1/1 (openssh-server) | R (unlocked shared file; 32-bit `ut_tv` is a Y2038 limit) | yes, `s17_users.rs`, `accounting.rs` | IMPLEMENT (in use) |
| 7 | `getpw` (man page: dangerous, overflow; use `getpwuid`) | default | 0/0 | U | yes | COMPAT-ONLY |
| 8 | `getwd` (no buffer size; removed from POSIX.1-2008) | default | 0/0 | U | yes | COMPAT-ONLY |
| 9 | `cuserid` (removed from POSIX; static buffer) | default | 2/2 (perl-base, libperl) | U, R | yes | IMPLEMENT (perl links it) |
| 10 | `getpass` (man page: obsolete) | default | 11/9 (login, passwd, ipmitool, rsync, ntfs-3g) | O | yes | IMPLEMENT (in use) |
| 11 | BSD signal masks: `sigblock`, `sigsetmask`, `siggetmask`, `sigpause`; System V `sigset`, `sighold`, `sigrelse`, `sigignore`; `bsd_signal`, `sysv_signal`, `siginterrupt` | default | sigsetmask 1/1 (dash); the others 0/0 | O | yes, `s20_signals.rs` | IMPLEMENT (thin, dash uses one) |
| 12 | `sigvec` | compat | 0/0 | O | yes | COMPAT-ONLY (as now) |
| 13 | `sigstack` (no stack size; `sigaltstack` replaces it) | default | 0/0 | U | yes | COMPAT-ONLY |
| 14 | `ftime` (removed from POSIX.1-2008) | default | 0/0 | O | yes, `clock_compat.rs` | COMPAT-ONLY |
| 15 | `re_comp`/`re_exec` (one hidden global pattern) | default | 0/0 | R | yes, `regex::re_comp` | COMPAT-ONLY |
| 16 | `<regexp.h>`: `step`, `advance`, `loc1`, `loc2`, `locs` (removed 2.28) | compat | 0/0 | R | objects exported (`compat_aliases.S`); logic is story LG-10 | STUB (no match) until the LG-19 corpus finds a user; defer LG-10 |
| 17 | `ecvt`/`fcvt`/`gcvt`, `qecvt` family (static buffers; removed from POSIX.1-2008) | default | 0/0 (`_r` forms too) | R | yes, `s08_numeric.rs`, `num_compat.rs` | COMPAT-ONLY |
| 18 | `_IO_*` vtable and stream objects (§12.13, F32) | default | 0/0 outside glibc | I (a writable dispatch table invites redirection) | objects defined (`data.rs`) | COMPAT-ONLY: export the objects with the manifest's sizes; LG-8 builds no dispatch through them (R15-3) |
| 19 | malloc hooks (`__malloc_hook`, `__free_hook`, `__realloc_hook`, `__memalign_hook`, `__after_morecore_hook`, `__malloc_initialize_hook`) | compat | 0/0 | I (any write redirects every allocation) | variables in `data.rs` | COMPAT-ONLY, inert (never read), as the phase 15 plan says |
| 20 | `libc_malloc_debug.so.0` (hooks, `malloc_get_state`/`set_state`, `mtrace` there) | compat | mtrace 2/2 (gawk, parted), all bound to libc.so.6 | O | `malloc_debug.rs`, 919 lines; the .so is not built | IMPLEMENT `mtrace`/`muntrace` in libc (as now); the .so waits for DL-1 at the lowest priority, `malloc_get_state` as STUB |
| 21 | `mallinfo` (`int` fields wrap above 2 GiB; man page: use `mallinfo2`) | default | 2/2 (lvm2); `mallinfo2` 15/12 | U (wraps) | yes, `malloc_compat.rs` | IMPLEMENT as specified (R12 asks about saturating instead) |
| 22 | Old resolver calls: `gethostbyname`(`2`), `gethostbyaddr`, `_r` forms, `getservbyname` | default | gethostbyname 21/19, gethostbyaddr 13/8, getservbyname 20/12 | R (static result), O (no IPv6 in the base form) | yes, `s16_nameservices.rs` | IMPLEMENT (widely used) |
| 23 | `inet_addr`, `inet_aton`, `inet_ntoa`, `inet_network`, `inet_makeaddr`, `inet_lnaof`, `inet_netof` | default (POSIX for most) | inet_ntoa 20/16, inet_aton 12/12, inet_addr 5/4; classful four 0/0 | O (`inet_addr`: -1 is also a valid address; `inet_aton` leniency, H12) | yes, `s15_sockets.rs` | IMPLEMENT |
| 24 | `rcmd`, `rresvport`, `ruserok`, `iruserok` (`_af` forms): trust by source address | default | ruserok_af 1/1 (libpam-modules, `pam_rhosts`); the others 0/0 | I | yes, `rcmd.rs`, 752 lines | IMPLEMENT ruserok family (pam uses it); rcmd/rresvport COMPAT-ONLY |
| 25 | `rexec`, `rexec_af` (sends the password in clear) | default | 0/0 | I | yes, `rcmd.rs` | STUB (fail with `ENOSYS`; needs a differences row and COMPAT flag) |
| 26 | Removed kernel calls and tty relics: `gtty`, `stty`, `revoke`, `sstk`, `ustat`, `stime`, `vtimes`, `vlimit`, `bdflush`, `uselib`, `create_module`, `nfsservctl`, `sysctl` | mixed | 0/0 | O | yes, stubs (`stubs.rs`) | STUB (as specified; nothing to change) |
| 27 | STREAMS (`getmsg`, `putmsg`, `isastream`, `fattach`, ...) | compat | 0/0 | O | yes, stubs | STUB (as specified) |
| 28 | `sys_errlist`, `sys_nerr`, `sys_siglist`, `h_errlist` (fixed-size arrays; 4 sized versions of `sys_errlist`) | compat (h_errlist default) | 0/0 | U (readers index by a count they assume) | yes | COMPAT-ONLY (as now) |
| 29 | libm `matherr`, `_LIB_VERSION`, `pow10` | compat | 0/0 | O | pow10 aliased; matherr/_LIB_VERSION missing (gaps (b)) | COMPAT-ONLY: matherr never called, `_LIB_VERSION` inert |
| 30 | `pthread_kill_other_threads_np`, `pthread_attr_{get,set}stackaddr` | compat / default | 0/0 | O (stackaddr has no size) | yes | COMPAT-ONLY |
| 31 | `inet6_option_*` (RFC 2292; RFC 3542 replaced it with `inet6_opt_*`) | default | 0/0 | O | yes, `inet_compat.rs` | COMPAT-ONLY |
| 32 | `__xstat`/`__fxstat`/`__lxstat`/`__xmknod` (pre-2.33 stat ABI) | default | 0/0 on this host | O | yes, `fs_compat.rs` | IMPLEMENT (thin; binaries built before 2.33 bind them) |
| 33 | Sun RPC/XDR (§29.1), NIS/NIS+ (§29.2) | compat | 0 outside glibc (LG-0c) | I | `sunrpc*.rs`, about 1,800 lines | STUB: **already ruled** (LEGACY-RPC-NIS-STUBS). Stories LG-12 to LG-18 need re-scoping to match |
| 34 | Compat-only versions with no known user (845 compat rows; 287 compat-only names in libc.so.6, 112 in libm) | compat | 0 on two other build hosts (LG-0c) | O | 272 exported as aliases | aliases (LG-1): IMPLEMENT (free at run time); old-behaviour versions (LG-2, LG-3 old condvars): only when the LG-19 corpus finds a user |

Also checked, old but in use, with no change recommended (IMPLEMENT): `usleep` 65/48, `getdtablesize` 13/12,
`sbrk` 35/11, `memalign` 13/6, `valloc` 1/1, `lckpwdf` 28/6, `putpwent` 22/4, `innetgr` 11/5, `getlogin` 18/12,
`getusershell` 4/3, `lchmod` 7/4. Modern counterparts for scale: `getaddrinfo` 141/92, `inet_ntop` 108/74,
`mkstemp` 41/33, `sigaction` 254/130, `getutxent` 11/8.

## RULING NEEDED (batch for the owner)

* **R0 (policy).** COMPAT-ONLY means the symbol keeps its version and Liella's headers do not declare it. Recommend:
  every name in that state gets one shared differences row ("declared only for binary compatibility") and a COMPAT
  flag that restores the declarations. Why: source code that still uses one of them builds against glibc's headers
  but not against Liella's.
* **R1 `gets`: COMPAT-ONLY.** It is insecure by design and gone from ISO C since C11, and no user was seen.
* **R2 `tmpnam`/`tmpnam_r`: IMPLEMENT with a deprecation warning in the header.** `tmpnam` is still ISO C, and the
  warning points users at `mkstemp`.
* **R3 DES `crypt`/`encrypt`/`setkey`: DROP; record it as decided.** No 2.39 manifest row exists, and every user
  binds the separate `libcrypt.so.1`.
* **R4 `getpw`, `getwd`, `sigstack`: COMPAT-ONLY.** Each writes into a buffer whose size it is never given, and no
  user was seen.
* **R5 `ftime`, `re_comp`/`re_exec`, `ecvt` family, `inet6_option_*`, `pthread_kill_other_threads_np`,
  `pthread_attr_*stackaddr`: COMPAT-ONLY.** Each has a modern replacement, and no user was seen.
* **R6 `<regexp.h>` (`step`/`advance`): STUB returning no match; defer LG-10 until the corpus finds a user.** A
  regex adapter would be built with no evidence of demand.
* **R7 `_IO_*` objects: COMPAT-ONLY objects with no dispatch through them (R15-3).** No dispatch means no
  redirection surface. No user was seen outside glibc.
* **R8 malloc hooks: COMPAT-ONLY and inert.** This keeps the current plan. Inert hooks are not an allocation
  redirection surface.
* **R9 `libc_malloc_debug.so.0`: lowest priority after DL-1; `malloc_get_state` as STUB.** Its two users bind
  `mtrace` in libc.so.6, which Liella already has.
* **R10 `rexec`/`rexec_af`: STUB (`ENOSYS`), with a differences row and a COMPAT flag.** It sends a password in
  clear text, and no user was seen.
* **R11 `rcmd`/`rresvport`: COMPAT-ONLY. Keep the `ruserok` family as it is (pam_rhosts).** Address-based trust is
  kept only where a user exists.
* **R12 `mallinfo`: IMPLEMENT as specified (wrapping), not saturating.** Saturation would be a behaviour
  difference with little gain, since `mallinfo2` is the fix.
* **R13 phase 15 stories LG-12 to LG-18: re-scope them to the RPC/NIS STUB ruling.** As written they still build
  XDR, RPC and NIS.
* **R14 old-behaviour compat versions (LG-2, LG-3): build them only for a user the corpus finds.** No host binary
  binds any compat version.

## Rulings (owner, 2026-11-04)

The owner accepted the coordinator's view on every row (source: liella-ops `state/PH14-OWNER-RULINGS.md`). As ruled:

* **R0.** COMPAT-ONLY = symbol and version kept, Liella's headers do not declare it. ONE shared differences row
  ("declared only for binary compatibility") and one COMPAT flag that restores the declarations.
* **R1 `gets`:** COMPAT-ONLY.
* **R2 `tmpnam`/`tmpnam_r`:** IMPLEMENT, with a header deprecation warning that points to `mkstemp`.
* **R3 DES `crypt`/`encrypt`/`setkey`:** DROP, recorded as decided (absent from the 2.39 manifest; `libcrypt.so.1` is separate).
* **R4 `getpw`, `getwd`, `sigstack`:** COMPAT-ONLY (unbounded buffers).
* **R5 `ftime`, `re_comp`/`re_exec`, `ecvt` family, `inet6_option_*`, `pthread_kill_other_threads_np`,
  `pthread_attr_*stackaddr`:** IMPLEMENT and KEEP DECLARED. **Differs from the recommendation (COMPAT-ONLY):**
  they are old but not unsafe, and hiding them breaks source builds that work against glibc, which does not meet
  the ABI rule's "very good reason" bar.
* **R6 `<regexp.h>` `step`/`advance`:** STUB (no match) plus a differences row; LG-10 deferred until the corpus finds a user.
* **R7 `_IO_*` objects:** COMPAT-ONLY objects, no dispatch through them.
* **R8 malloc hooks:** COMPAT-ONLY, inert.
* **R9 `libc_malloc_debug.so.0`:** lowest priority after DL-1; `malloc_get_state` is a STUB.
* **R10 `rexec`/`rexec_af`:** STUB (`ENOSYS`) plus a differences row and a COMPAT flag.
* **R11 `rcmd` family:** STUB, per the 2026-10-27 RPC/NIS ruling. **Differs from the recommendation (`rcmd`
  COMPAT-ONLY, `ruserok` IMPLEMENT).** `ruserok`/`iruserok` (`_af`) are stubs that always DENY, so `pam_rhosts`
  still loads and address-based trust never succeeds.
* **R12 `mallinfo`:** IMPLEMENT as specified (wrapping), not saturating.
* **R13 LG-12 to LG-18:** re-scoped to the RPC/NIS STUB ruling (phase 15 plan updated).
* **R14 old-behaviour compat versions (LG-2, LG-3):** built only for a user the LG-19 corpus finds.
* The "Check scope first" paragraph for CONTRIBUTING.md was shown to the owner on 2026-11-04; approval is pending.

## Proposed standing check (text for the owner to approve)

For the brief template (Rules section):

> **Should Liella have it at all?** Before implementing an interface, check whether it is obsolete, deprecated,
> removed upstream (compat-only in `spec/abi/versions.tsv`), or insecure by design (spec §30 H12,
> `plan/scope-review-legacy.md`). If it is and no ruling covers it, stop and write RULING NEEDED (IMPLEMENT,
> COMPAT-ONLY, STUB or DROP) with your recommendation and evidence of users. Do not replicate it just because
> glibc has it.

For CONTRIBUTING.md (after "Clean-room design"):

> **Check scope first.** Before implementing an interface, check whether Liella should have it at all.
> Deprecated, removed-upstream and insecure-by-design interfaces need a maintainer's ruling first.
> `plan/scope-review-legacy.md` lists the families already reviewed. Evidence of users comes from black-box
> binary scans or the binary-compatibility corpus, never from another libc's source.
