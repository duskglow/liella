# Behaviour documents

Black-box observations of the reference library (glibc 2.39 as installed on Ubuntu 24.04,
x86_64), recorded for the clean-room remediation of 2026-10-02 (implementation plan §1.4).

Each document covers one item. It names the spec sections, the probe program, and the recorded
output of that probe against the reference library. The probes are C programs in
`crates/core/tests/oracle/`; they call only public interfaces and print what they observe. The
recorded outputs (`*.glibc.txt`) are what the reference library printed. Nothing here describes
or derives from the reference library's source code.

The same probes are the differential tests (`crates/core/tests/*_products.rs`): each builds the
probe against the host C library and statically against Liella's products and compares the
outputs line by line, except where a deliberate difference is recorded.

How the records were made: on the reference system, each probe built with
`cc -std=gnu17 -O1 <probe>.c` and run with an empty environment and `LOCPATH` pointing at the
test locales (`target/test-locales`, compiled with `localedef`). `stdio_diff` takes a scratch
directory argument; its output shows that directory as `<dir>`.

| Document | Items (plan §1.4) |
|---|---|
| [printf-registry.md](printf-registry.md) | 1 |
| [getopt.md](getopt.md) | 2 |
| [strfmon.md](strfmon.md) | 3 |
| [fmtmsg.md](fmtmsg.md) | 4 |
| [tempnam.md](tempnam.md) | 5 |
| [diagnostics.md](diagnostics.md) | 6, 7 |
| [scanf.md](scanf.md) | 8 |
| [getline-info.md](getline-info.md) | 9 |
| [argp.md](argp.md) | `argp` (phase 8, written under the same rules before implementation) |
| [backtrace.md](backtrace.md) | `backtrace` (phase 8, written under the same rules before implementation) |
| [timers.md](timers.md) | §23.4 timers (phase 9, written under the same rules before implementation) |
| [zones.md](zones.md) | §23.5-23.6 broken-down time and time zones (phase 9, same rules) |
| [timefmt.md](timefmt.md) | §23.7 strftime, strptime, getdate (phase 9, same rules) |
| [netaddr.md](netaddr.md) | §15.3-15.8 address conversion, interfaces, IPv6 options, Ethernet addresses (phase 10, same rules) |
| [users.md](users.md) | §17.3-17.6 user, group, shadow databases, shells, utmp (phase 10, same rules; run with `users_run.sh`) |
| [netdb.md](netdb.md) | §16 netdb databases through `files`, getaddrinfo without DNS (phase 10, same rules; run with `netdb_run.sh`) |
| [dns.md](dns.md) | §16.1-16.4 the resolver, lookups through `dns`, nsswitch.conf actions (phase 10, same rules; run with `dns_run.sh`) |
| [process.md](process.md) | §18.3-18.9 exec, spawn, pidfds, wait, system/popen, groups and sessions, daemon (phase 11, same rules; run with `process_run.sh`) |
| [terminals.md](terminals.md) | §19 terminal attributes and speeds, job control, terminal names, pseudo-terminals, ttyent (phase 11, same rules; run with `terminals_run.sh`) |
| [fstree.md](fstree.md) | §13.1-13.3, §13.8-13.9 working directory, directory streams, scandir, ftw/nftw/fts, realpath, temporary names (phase 12, same rules; run with `fstree_run.sh`; directory order recorded on ext4) |
| [patterns.md](patterns.md) | §14.7-14.10 fnmatch, glob (both versions), POSIX and GNU regex with generated corpora, wordexp (phase 12, same rules; run with `patterns_run.sh`; record gzipped) |
| [mntent.md](mntent.md) | §13.13 mount tables and fstab: setmntent, getmntent, getmntent_r, addmntent, endmntent, hasmntopt, the fsent functions (phase 12, same rules; run with `mntent_run.sh`; the root phase reads a fixture `/etc` and the probe's own tmpfs mounts in a private mount namespace) |
| [mallocdbg.md](mallocdbg.md) | §3.2-3.3 mallopt, mallinfo/mallinfo2, malloc_stats, malloc_info, malloc_trim, the hook variables, malloc_get_state/set_state, mcheck, mprobe, mtrace, and the `MALLOC_*_`/`glibc.malloc.*` settings (phase 13, same rules; run with `mallocdbg_run.sh`; statistics recorded as relations, D-3.4; the part after `== reference only` is the reference with `libc_malloc_debug.so` preloaded) |
| [syslog.md](syslog.md) | §26.1 openlog, syslog, vsyslog, closelog, setlogmask, __syslog_chk, __vsyslog_chk (phase 13, same rules; run with `syslog_run.sh`; the probe binds its own `/dev/log` and `/dev/console` in a tmpfs over `/dev` in a private mount namespace) |
| [posixipc.md](posixipc.md) | §25.3-25.4 POSIX message queues (with SIGEV_SIGNAL and SIGEV_THREAD notification) and shared memory (phase 13, same rules; run with `posixipc_run.sh`; private IPC namespace with its own `/dev/mqueue`, `/dev/shm` a private tmpfs, also ext4 and absent) |
| [aio.md](aio.md) | §11.11 POSIX asynchronous I/O: aio_read, aio_write, aio_fsync, aio_error, aio_return, aio_suspend, aio_cancel, lio_listio, aio_init and the `64` forms (phase 13, same rules; run with `aio_run.sh`; the observable contract only, not the reference's thread pool, D-11.8; observations outside the record with `aio_run.sh PROGRAM SCRATCH extra`) |
| [catalogs.md](catalogs.md) | §7.7-7.8 the gettext family (catalog paths, `LANGUAGE`, codesets, plural forms, contexts) and catopen, catgets, catclose (`NLSPATH`) (phase 13, same rules; run with `catalogs_run.sh`, fixtures from `msgfmt` and `gencat`; the `.mo` and `.cat` formats in [catalogs-format.md](catalogs-format.md); damaged-file cases outside the record with `catalogs_run.sh PROGRAM SCRATCH unstable`) |
| [noproc.md](noproc.md) | the functions that read `/proc` or `/sys`, with and without them (2026-10-04, same rules; OD-5, D-X.12; the second run in a private namespace with an empty tmpfs over `/proc` and `/sys`) |
| [specman.md](specman.md) | the 31 spec-versus-man-page findings of 2026-10-04, each settled under the three-way rule (probes `specman_probe.c`, `specman_dns.sh`) |
| [setjmp.md](setjmp.md) | §21.1 which forms of `setjmp` save the signal mask and which jumps restore it (2026-10-04, same rules; D-21.4; recorded dynamically and statically linked) |
| [digits-dots.md](digits-dots.md) | §16.1 `__nss_hostname_digits_dots` (milestone 5, 2026-10-04: a signature for a spec gap; RULING NEEDED) |
| [libm-exact.md](libm-exact.md) | §10.1, §10.2, §10.4, §10.6 classification, the exact and correctly rounded functions in all four formats, the floating-point environment (Track M milestone 1, same rules; probe `crates/math/tests/oracle/exact_diff.c`, record gzipped) |
| [libm-elem.md](libm-elem.md) | §10.3 the elementary functions of `float` and `double`: special operands, flags, `errno`, NaN signs (Track M milestone 2; probe `crates/math/tests/oracle/elem_diff.c`, record gzipped) |
| [libm-elem-ld.md](libm-elem-ld.md) | §10.3 the elementary functions of `long double`: special operands, NaN signs, flags, `errno` (Track M milestone 5a; probe `crates/math/tests/oracle/elem_ld_diff.c`, record gzipped) |
| [libm-special.md](libm-special.md) | §10.3 the special functions (`erf`, `erfc`, the gamma functions and `signgam`, the Bessel functions) of `float` and `double` (Track M milestone 3; probe `crates/math/tests/oracle/special_diff.c`, record gzipped) |
| [libm-special-ld.md](libm-special-ld.md) | §10.3 the special functions of `long double`: special operands, the sign of Γ(x), zero limits, flags, `errno` (Track M milestone 5b; probe `crates/math/tests/oracle/special_ld_diff.c`, record gzipped) |
| [libm-complex.md](libm-complex.md) | §10.5 the complex functions of `float` and `double`: the Annex G special values, NaN choices, flags, `errno`, `cpow` of special operands (Track M milestone 4a; probe `crates/math/tests/oracle/complex_diff.c`, record gzipped, the grid part) |
| [env-cache.md](env-cache.md) | how the reference library's environment readers (`getenv`, `wordexp` IFS, `gettext` LANGUAGE, `localtime`/`localtime_r` TZ) see changes to the environment, and what each costs per call; input to the decision on a generation-counter cache (ENV-PROBE 2026-10-09; probe `env-cache.probe.c`, record `env-cache.glibc.txt`) |
| [efault.md](efault.md) | what path-taking calls do when given a pointer the process cannot read or write: `EFAULT` from the kernel, or a crash where the reference reads the text first (EFAULT-PATHS 2026-10-09; probe `conformance/c/s13_efault.c`, record `efault.glibc.txt`) |
| [dynlink-secure.md](dynlink-secure.md) | §27 the loader in a secure (`AT_SECURE`) process: `LD_PRELOAD`/`LD_AUDIT`, `$ORIGIN`, `LD_LIBRARY_PATH`, `/etc/ld.so.preload`, damaged caches, the environment passed on, `TZ`, the diagnostic variables (DL-PROBE-SECURE 2026-10-23; run with `tools/probe/dlsecure/run.sh`; set-group-ID without root, R14-18) |
| [dynlink.md](dynlink.md) | §27 the dynamic loader as seen from outside: dlerror texts per failure, loader exit status and messages, the init/fini order corpus, the static-TLS surplus, `$LIB`/`$PLATFORM`, `dl_iterate_phdr` counters, the placeholder libraries, the BIND_NOW share of `/usr/bin` (DL-0b `DL-PROBE-BEHAVIOUR` 2026-10-23; run with `tools/probe/dl/run.sh`, record `dynlink.glibc.txt`) |

**Directory order** (owner ruling 2026-10-04, plan D-T.1): an order that follows directory order is recorded together with the filesystem that produced it, and the differential tests compare such results as sets; an extra tmpfs run is added only where order is the subject of a test.
