# Code map (generated: `cargo xtask codemap`; do not edit by hand above the hand-written sections)

Where each spec section lives. Exported symbols: `grep -P '^strlen\t' docs/codemap/symbols.tsv`
(symbol, version, defining file:line, first implementation call). Plan index: implementation-plan.md.
Differential (`*_products.rs`) tests are listed once at the end; `cargo xtask affected --list` shows
exactly which tests a change reaches.

| § | Title | Boundary (C entry points) | Library modules used | Clause tests | Conformance | Decisions | Behaviour records |
|---|---|---|---|---|---|---|---|
| 1 | Program startup, termination and the program environment | crates/boundary/src/posix/s01_startup.rs | abi::ctypes, boundary::argp, boundary::conv, boundary::options, boundary::process, boundary::ret, boundary::runtime, boundary::strz, legacy::exit_handlers, legacy::stubs | s01_startup_symbols.rs, crates/core/tests/s01_startup_clauses.rs | s01_abort.c, s01_argp.c, s01_cxa_finalize.c, s01_env.c, s01_envfind.c, s01_envscan.c, s01_exit_order.c, s01_exit_status.c, s01_exit_threads.c, s01_ifunc.c, s01_irelative_relro.c, s01_quick_exit.c, s01_relro.c, s01_secure.c, s01_startup.c, s01_static_pie.c, s01_static_pie_multi_writable.c, s01_static_pie_rela.c, s01_streams.c, s01_symbols_options.c, s01_thread_dtors.c, s01_tunables.c | plan/07-decisions/s01-program-startup-termination-and-environment.md | aio.md, argp.md, backtrace.md, catalogs.md, dns.md, env-cache.md, fstree.md, getopt.md, mallocdbg.md, mntent.md, netaddr.md, netdb.md, noproc.md, patterns.md, posixipc.md, process.md, specman.md, syslog.md, terminals.md, timefmt.md, timers.md, users.md, zones.md |
| 2 | Error reporting and diagnostics | crates/boundary/src/posix/s02_diagnostics.rs | abi::ctypes, boundary::backtrace, boundary::conv, boundary::fmtmsg, boundary::process, boundary::report, boundary::ret, boundary::strz, boundary::tb | s02_diagnostics_symbols.rs, crates/core/tests/s02_diagnostics_clauses.rs | s02_assert.c, s02_assert_perror.c, s02_backtrace.c, s02_backtrace_static_pie.c, s02_chk_fail.c, s02_report.c, s02_stack_smash.c | plan/07-decisions/s02-error-reporting-and-diagnostics.md | backtrace.md, compat-objects.md, diagnostics.md, fmtmsg.md |
| 3 | Memory allocation | crates/boundary/src/posix/s03_memory.rs | abi::ctypes, abi::layout, boundary::caller_alloc, boundary::conv, boundary::data, boundary::heap, boundary::raw, boundary::ret, legacy::malloc_compat, legacy::obstack, rt::heap | s03_memory_symbols.rs, crates/core/tests/s03_memory_clauses.rs | s03_mcheck.c, s03_mcheck_default.c, s03_mcheck_pedantic.c, s03_mtrace.c, s03_mtrace_secure.c, s03_perturb.c, s03_perturb_tunable.c, s03_rlimit_as.c, s03_rlimit_as_raised.c | plan/07-decisions/s03-memory-allocation.md | compat-objects.md, mallocdbg.md |
| 4 | Byte arrays and strings | crates/boundary/src/posix/s04_strings.rs | abi::ctypes, boundary::caller_alloc, boundary::conv, boundary::dispatch, boundary::ffi, boundary::hot, boundary::locale, boundary::native, boundary::ret, boundary::strz, boundary::sys, kernel::errno, legacy::cstring, legacy::string_compat | s04_strings_symbols.rs, crates/core/tests/s04_strings_clauses.rs | s04_casecmp_locales.c, s04_classed_memset.c, s04_strings_early_copies.c, s04_strings_smoke.c | plan/07-decisions/s04-byte-arrays-and-strings.md |  |
| 5 | Character classification and case mapping | crates/boundary/src/posix/s05_ctype.rs | abi::ctypes, boundary::conv, boundary::locale, boundary::ret, boundary::strz, legacy::ctype_tables | s05_ctype_symbols.rs, crates/core/tests/s05_ctype_clauses.rs | s05_ctype_words.c | plan/07-decisions/s05-character-classification.md | ctype-words.md |
| 6 | Multibyte text, wide characters and character-set conversion | crates/boundary/src/posix/s06_multibyte.rs | abi::ctypes, boundary::conv, boundary::iconv, boundary::locale, boundary::ret, boundary::strz, legacy::mb_compat, legacy::mb_state | s06_multibyte_symbols.rs, crates/core/tests/s06_multibyte_clauses.rs | s06_mbrtowc_quick_ceiling.c | plan/07-decisions/s06-multibyte-text-wide-characters-and.md |  |
| 7 | Locales and message translation | crates/boundary/src/posix/s07_locale.rs | abi::ctypes, abi::layout, boundary::catalog, boundary::conv, boundary::locale, boundary::monetary, boundary::ret, boundary::strz | s07_locale_symbols.rs, crates/core/tests/s07_locale_clauses.rs | s07_langinfo_items.c, s07_setlocale_first_case.c, s07_strfmon.c, s07_symbols_strfmon.c | plan/07-decisions/s07-locales-and-message-translation.md | catalogs-format.md, catalogs.md, ctype-words.md, env-cache.md, strfmon.md |
| 8 | Numeric conversion and integer arithmetic | crates/boundary/src/posix/s08_numeric.rs | abi::ctypes, abi::layout, abi::pack, boundary::conv, boundary::locale, boundary::ret, boundary::strz, boundary::sys, boundary::users, kernel::machine, legacy::num_compat, legacy::string_compat | s08_numeric_symbols.rs, crates/core/tests/s08_numeric_clauses.rs | s08_strto_fenv_flags.c, s08_strto_internal_group.c | plan/07-decisions/s08-numeric-conversion-and-integer-arithmetic.md | zones.md |
| 9 | Random numbers | crates/boundary/src/posix/s09_random.rs | abi::ctypes, abi::layout, boundary::cancel, boundary::conv, boundary::ret, boundary::sys, kernel::sys, legacy::prng_global, legacy::prng_r | s09_random_symbols.rs, crates/core/tests/s09_random_clauses.rs |  | plan/07-decisions/s09-random-numbers.md |  |
| 10 | Mathematics (`libm`) | crates/boundary/src/posix/s10_math.rs, crates/boundary/src/posix/s10_math_gen.rs | abi::ctypes, abi::layout, boundary::conv, boundary::hot, boundary::mathret, boundary::ret, boundary::strz, boundary::sys, rt::fpu | s10_math_symbols.rs, crates/math/tests/s10_math_clauses.rs | s10_sqrt_env.c, s10_sqrtl_entry.c | plan/07-decisions/s10-mathematics.md | libm-complex.md, libm-elem-ld.md, libm-elem.md, libm-exact.md, libm-special-ld.md, libm-special.md, math-ftz-daz.md |
| 11 | Low-level I/O on file descriptors | crates/boundary/src/posix/s11_lowio.rs | abi::ctypes, abi::layout, boundary::aio, boundary::cancel, boundary::conv, boundary::raw, boundary::ret, boundary::strz, boundary::sys, kernel::fd, kernel::mem, kernel::sys, kernel::uapi, legacy::io_compat | s11_lowio_symbols.rs, crates/core/tests/s11_lowio_clauses.rs | s11_private_descriptors.c, s11_variadic.c | plan/07-decisions/s11-low-level-i-o.md | aio.md, noproc.md |
| 12 | Buffered streams (`<stdio.h>`) | crates/boundary/src/posix/s12_stdio.rs | abi::ctypes, abi::layout, boundary::conv, boundary::printf, boundary::ret, boundary::scanf, boundary::stdio, boundary::strz, boundary::users, legacy::printf_registry, legacy::stdio_compat | s12_stdio_symbols.rs, crates/core/tests/s12_stdio_clauses.rs | s12_ftell_alone.c, s12_malloc_fails.c, s12_printf.c, s12_registry.c, s12_scanf.c, s12_stdio_front.c, s12_stdio_lockfile.c, s12_stdio_mt_front.c, s12_stdio_window.c, s12_symbols_libio.c, s12_symbols_printf.c, s12_symbols_printf_chk.c, s12_symbols_registry.c, s12_symbols_scanf.c | plan/07-decisions/s12-buffered-streams.md | compat-libio.md, getline-info.md, printf-registry.md, scanf.md, tempnam.md, terminals.md |
| 13 | File-system interface | crates/boundary/src/posix/s13_filesystem.rs | abi::callback, abi::ctypes, abi::layout, boundary::conv, boundary::dirtree, boundary::errno, boundary::locale, boundary::mntent, boundary::raw, boundary::ret, boundary::strz, boundary::sys, kernel::errno, kernel::fd, kernel::path, kernel::sys, kernel::uapi, legacy::fs_compat, legacy::fs_tree, legacy::stubs, legacy::versioned | s13_filesystem_symbols.rs, crates/core/tests/s13_filesystem_clauses.rs | s13_efault.c, s13_readdir_alone.c | plan/07-decisions/s13-file-system.md | efault.md, fstree.md, mntent.md, noproc.md |
| 14 | Searching, sorting and pattern matching | crates/boundary/src/posix/s14_search.rs | abi::callback, abi::ctypes, abi::layout, boundary::conv, boundary::patterns, boundary::regex, boundary::ret, boundary::strz, legacy::search_c | s14_search_symbols.rs, crates/core/tests/s14_search_clauses.rs | s14_glob_dir.c, s14_regexec_min_stack.c | plan/07-decisions/s14-searching-sorting-and-patterns.md | env-cache.md, patterns.md, regex-pre14-triage.md, regex-submatch-analysis.md |
| 15 | Sockets and network interfaces | crates/boundary/src/posix/s15_sockets.rs | abi::ctypes, boundary::cancel, boundary::conv, boundary::errno, boundary::netaddr, boundary::raw, boundary::ret, boundary::strz, boundary::sys, kernel::mem, kernel::sys, kernel::uapi, legacy::inet_compat, legacy::net_compat | s15_sockets_symbols.rs, crates/core/tests/s15_sockets_clauses.rs |  | plan/07-decisions/s15-sockets-and-network-interfaces.md | netaddr.md, netdb.md |
| 16 | Name services and databases | crates/boundary/src/posix/s16_nameservices.rs | abi::ctypes, abi::layout, boundary::conv, boundary::gai_async, boundary::netaddr, boundary::netdb, boundary::resolv, boundary::ret, boundary::strz, kernel::errno, kernel::uapi | s16_nameservices_symbols.rs, crates/core/tests/s16_nameservices_clauses.rs |  | plan/07-decisions/s16-name-services.md | dns.md, netdb.md, ns-gaps.md |
| 17 | Users, groups, credentials and login accounting | crates/boundary/src/posix/s17_users.rs | abi::ctypes, abi::layout, boundary::conv, boundary::nr, boundary::ret, boundary::setxid, boundary::strz, boundary::sys, boundary::users, kernel::sys, legacy::cred_compat, legacy::nss_files | s17_users_symbols.rs, crates/core/tests/s17_users_clauses.rs | s17_initgroups.c, s17_setxid.c, s17_setxid_abort.c, s17_setxid_ids.c | plan/07-decisions/s17-users-groups-and-credentials.md | noproc.md, users.md |
| 18 | Processes | crates/boundary/src/posix/s18_processes.rs | abi::ctypes, abi::layout, boundary::cancel, boundary::conv, boundary::fork, boundary::nscache, boundary::pthread, boundary::raw, boundary::ret, boundary::spawn, boundary::strz, boundary::sys, kernel::fd, kernel::sys, kernel::uapi, legacy::spawn_compat | s18_processes_symbols.rs, crates/core/tests/s18_processes_clauses.rs | s18_exec.c, s18_fork.c, s18_fork_aliases.c, s18_fork_reset.c, s18_spawn_vm.c, s18_system_cancel.c, s18_vfork.c | plan/07-decisions/s18-processes.md | noproc.md, process.md |
| 19 | Terminals, job control and pseudo-terminals | crates/boundary/src/posix/s19_terminals.rs | abi::ctypes, abi::layout, abi::pack, boundary::cancel, boundary::conv, boundary::ret, boundary::strz, boundary::sys, boundary::terminal, kernel::uapi, legacy::stubs, legacy::term_compat | s19_terminals_symbols.rs, crates/core/tests/s19_terminals_clauses.rs | s19_terminals.c | plan/07-decisions/s19-terminals.md | noproc.md, terminals.md |
| 20 | Signals | crates/boundary/src/posix/s20_signals.rs | abi::ctypes, abi::layout, boundary::cancel, boundary::conv, boundary::errno, boundary::raw, boundary::report, boundary::ret, boundary::strz, boundary::sys, boundary::thread, kernel::sys, kernel::uapi, legacy::signal_compat, rt::thread | s20_signals_symbols.rs, crates/core/tests/s20_signals_clauses.rs | s20_internal_signal.c, s20_psignal_stream.c, s20_pthread_kill.c | plan/07-decisions/s20-signals.md | compat-objects.md |
| 21 | Non-local control transfer and execution contexts | crates/boundary/src/posix/s21_contexts.rs | abi::ctypes, abi::layout, boundary::context, boundary::conv | s21_contexts_symbols.rs, crates/core/tests/s21_contexts_clauses.rs | s21_context.c, s21_longjmp_chk.c, s21_setjmp.c | plan/07-decisions/s21-contexts.md | setjmp.md |
| 22 | Threads and synchronization | crates/boundary/src/posix/s22_threads.rs | abi::ctypes, abi::layout, boundary::cancel, boundary::conv, boundary::ffi, boundary::fork, boundary::pthread, boundary::ret, boundary::strz, boundary::sync, kernel::uapi | s22_threads_symbols.rs, crates/core/tests/s22_threads_clauses.rs | s22_cancel.c, s22_cancel_fast.c, s22_cancel_points.c, s22_cancel_race.c, s22_compat_condvar.c, s22_errno.c, s22_key_nomem.c, s22_last_exit.c, s22_lifecycle.c, s22_more.c, s22_mutex_alone.c, s22_mutex_clone.c, s22_once_cxx.c, s22_once_unwind.c, s22_order.c, s22_symbols.c, s22_sync.c | plan/07-decisions/s22-threads.md | key-nomem.md, noproc.md |
| 23 | Time, clocks and timers | crates/boundary/src/posix/s23_time.rs | abi::ctypes, abi::layout, boundary::cancel, boundary::conv, boundary::locale, boundary::ret, boundary::strz, boundary::sys, boundary::timefmt, boundary::timer, boundary::zone, kernel::sys, kernel::uapi, legacy::clock_compat, legacy::getdate, legacy::time_compat | s23_time_symbols.rs, crates/core/tests/s23_time_clauses.rs, crates/core/tests/s23_strptime_engine.rs | s23_timefmt.c, s23_tzfresh.c, s23_zones.c | plan/07-decisions/s23-time.md | env-cache.md, time-gaps.md, timefmt.md, timers.md, zones.md |
| 24 | Resource limits, scheduling and system information | crates/boundary/src/posix/s24_system.rs | abi::ctypes, boundary::conv, boundary::netaddr, boundary::nr, boundary::process, boundary::raw, boundary::ret, boundary::strz, boundary::sys, kernel::errno, kernel::sys, kernel::uapi, legacy::sys_compat, legacy::sysconf_table | s24_system_symbols.rs, crates/core/tests/s24_system_clauses.rs | s24_hostname_chk.c, s24_variadic.c | plan/07-decisions/s24-resources-and-system-information.md | compat.md, noproc.md |
| 25 | Inter-process communication | crates/boundary/src/posix/s25_ipc.rs | abi::ctypes, boundary::cancel, boundary::conv, boundary::mqueue, boundary::raw, boundary::ret, boundary::strz, boundary::sys, boundary::timer, kernel::sys, kernel::uapi, legacy::ipc_compat | s25_ipc_symbols.rs, crates/core/tests/s25_ipc_clauses.rs | s25_mqueue.c, s25_semctl.c | plan/07-decisions/s25-ipc.md | posixipc.md |
| 26 | System logging | crates/boundary/src/posix/s26_syslog.rs | abi::ctypes, boundary::conv, boundary::strz, boundary::syslog, legacy::log_compat | s26_syslog_symbols.rs, crates/core/tests/s26_syslog_clauses.rs |  | plan/07-decisions/s26-29.md | syslog.md |
| 27 | Dynamic linking and loading | crates/boundary/src/posix/s27_dynlink.rs | abi::ctypes, abi::layout, boundary::conv, boundary::process, boundary::sys, boundary::tb, kernel::auxv, kernel::uapi, rt::cpu | s27_dynlink_symbols.rs, crates/core/tests/s27_dynlink_clauses.rs | s27_cpuid_feature_leaf.c, s27_dl_find_object.c, s27_dl_iterate_phdr.c, s27_dlopen_failures.c, s27_dlsym_handles.c, s27_dynlink_helper_dlsym.c, s27_static_pie_load_offset.c |  | dynlink-secure.md, dynlink.md |
| 28 | Profiling and instrumentation | crates/boundary/src/posix/s28_profiling.rs | abi::ctypes, boundary::ffi, boundary::process, boundary::raw, boundary::ret, boundary::strz, boundary::sys, kernel::errno, kernel::fd, kernel::sys, kernel::uapi, legacy::signal_compat | s28_profiling_symbols.rs, crates/core/tests/s28_profiling_clauses.rs |  |  |  |
| 29 | Legacy and compatibility subsystems | crates/boundary/src/posix/s29_legacy.rs, crates/boundary/src/posix/s29_rpc_auth.rs, crates/boundary/src/posix/s29_rpc_clnt.rs, crates/boundary/src/posix/s29_rpc_svc.rs, crates/boundary/src/posix/s29_xdr_streams.rs | abi::ctypes, abi::layout, boundary::errno, boundary::heap, boundary::report, boundary::ret, boundary::rpc_text, boundary::sys, boundary::users, kernel::sys, kernel::uapi, legacy::stubs, legacy::sunrpc, legacy::sunrpc_rec | s29_legacy_symbols.rs, crates/core/tests/s29_legacy_clauses.rs, s29_rpc_auth_stubs.rs, s29_rpc_clnt_svc_stubs.rs |  |  | compat-objects.md, compat.md, regexp-h.md |
| 30 | Catalogue of historical accidents and path dependencies |  |  |  |  |  |  |

Differential tests (`crates/*/tests/*_products.rs`): crates/boundary/tests/failinject_products.rs, crates/core/tests/admin_products.rs, crates/core/tests/aio_products.rs, crates/core/tests/argp_products.rs, crates/core/tests/backtrace_products.rs, crates/core/tests/catalogs_products.rs, crates/core/tests/cold_bss_products.rs, crates/core/tests/cxx_throw_products.rs, crates/core/tests/diag_products.rs, crates/core/tests/digits_dots_products.rs, crates/core/tests/dns_products.rs, crates/core/tests/env_contract_products.rs, crates/core/tests/fstree_products.rs, crates/core/tests/getopt_products.rs, crates/core/tests/gmon_products.rs, crates/core/tests/heap_meta_products.rs, crates/core/tests/hostconf_diag_products.rs, crates/core/tests/hostconf_env_products.rs, crates/core/tests/hot_text_products.rs, crates/core/tests/iconv_products.rs, crates/core/tests/langpack_products.rs, crates/core/tests/locale_products.rs, crates/core/tests/mallocdbg_products.rs, crates/core/tests/mntent_products.rs, crates/core/tests/monetary_products.rs, crates/core/tests/netaddr_products.rs, crates/core/tests/netdb_products.rs, crates/core/tests/noproc_products.rs, crates/core/tests/nscache_products.rs, crates/core/tests/patterns_products.rs, crates/core/tests/posixipc_products.rs, crates/core/tests/printf_corpus_products.rs, crates/core/tests/printf_products.rs, crates/core/tests/process_products.rs, crates/core/tests/regex_corpus_products.rs, crates/core/tests/registry_products.rs, crates/core/tests/scanf_products.rs, crates/core/tests/stdio_products.rs, crates/core/tests/syslog_products.rs, crates/core/tests/terminals_products.rs, crates/core/tests/timefmt_products.rs, crates/core/tests/timers_products.rs, crates/core/tests/users_products.rs, crates/core/tests/versions_products.rs, crates/core/tests/wstream_products.rs, crates/core/tests/zones_products.rs, crates/math/tests/complex_f128_products.rs, crates/math/tests/complex_ld_cr_products.rs, crates/math/tests/complex_ld_products.rs, crates/math/tests/complex_products.rs, crates/math/tests/elem_f128_products.rs, crates/math/tests/elem_ld_products.rs, crates/math/tests/elem_products.rs, crates/math/tests/exact_products.rs, crates/math/tests/special_f128_products.rs, crates/math/tests/special_ld_products.rs, crates/math/tests/special_products.rs, crates/math/tests/x87_nan_products.rs

Other crates: abi, boundary, core, kernel, ld, ld-so, legacy, libc-so, libm-so, math, mathtools, mem, native, rt, spectest, thread-db

## System files the library reads or writes

| Path | Named at (file:line; the first is usually the constant) |
|---|---|
| `/dev/` | crates/legacy/src/accounting.rs:470 |
| `/dev/console` | crates/boundary/src/fmtmsg.rs:268, crates/core/src/log.rs:65, crates/core/src/term.rs:506 |
| `/dev/log` | crates/core/src/log.rs:63 |
| `/dev/null` | crates/boundary/src/selftest.rs:442, crates/boundary/src/spawn.rs:778, crates/boundary/src/spawn.rs:851, crates/rt/src/start.rs:245 |
| `/dev/ptmx` | crates/core/src/term.rs:335, crates/core/src/term.rs:507 |
| `/dev/pts/` | crates/core/src/term.rs:391, crates/core/src/term.rs:502 |
| `/dev/shm/` | crates/boundary/src/mqueue.rs:370, crates/boundary/src/nscache.rs:54 |
| `/dev/shm/sem.` | crates/boundary/src/sync.rs:851 |
| `/dev/shm/sem.liella-` | crates/boundary/src/sync.rs:903 |
| `/dev/tty` | crates/boundary/src/stdio.rs:2140, crates/boundary/src/users.rs:893, crates/core/src/term.rs:503, crates/core/src/term.rs:505 |
| `/dev/ttyS` | crates/core/src/term.rs:504 |
| `/etc/.pwd.lock` | crates/legacy/src/accounting.rs:42 |
| `/etc/aliases` | crates/legacy/src/nss.rs:93 |
| `/etc/ethers` | crates/legacy/src/nss_files.rs:136 |
| `/etc/fstab` | crates/boundary/src/mntent.rs:40 |
| `/etc/gai.conf` | crates/legacy/src/nss.rs:85 |
| `/etc/group` | crates/legacy/src/nss_files.rs:45 |
| `/etc/gshadow` | crates/legacy/src/nss_files.rs:49 |
| `/etc/host.conf` | crates/legacy/src/nss.rs:84 |
| `/etc/hostid` | crates/legacy/src/sys_compat.rs:282 |
| `/etc/hosts` | crates/legacy/src/nss.rs:86 |
| `/etc/localtime` | crates/core/src/time.rs:39 |
| `/etc/netgroup` | crates/legacy/src/nss.rs:92 |
| `/etc/networks` | crates/legacy/src/nss.rs:87 |
| `/etc/nsswitch.conf` | crates/legacy/src/nss.rs:83 |
| `/etc/passwd` | crates/legacy/src/nss_files.rs:43 |
| `/etc/protocols` | crates/legacy/src/nss.rs:89 |
| `/etc/resolv.conf` | crates/legacy/src/resolv_compat.rs:51 |
| `/etc/rpc` | crates/legacy/src/nss.rs:90 |
| `/etc/services` | crates/legacy/src/nss.rs:88 |
| `/etc/shadow` | crates/legacy/src/nss_files.rs:47 |
| `/etc/shells` | crates/legacy/src/accounting.rs:44 |
| `/etc/ttys` | crates/legacy/src/accounting.rs:46 |
| `/proc/loadavg` | crates/core/src/system.rs:191 |
| `/proc/self/auxv` | crates/boundary/src/sys.rs:343 |
| `/proc/self/fd` | crates/core/src/process.rs:453, crates/legacy/src/io_compat.rs:40 |
| `/proc/self/fd/` | crates/core/src/term.rs:462, crates/legacy/src/fs_compat.rs:66 |
| `/proc/self/fd/0123456789` | crates/legacy/src/accounting.rs:447 |
| `/proc/self/fdinfo/` | crates/boundary/src/spawn.rs:439 |
| `/proc/self/loginuid` | crates/legacy/src/accounting.rs:48 |
| `/proc/self/maps` | crates/boundary/src/pthread.rs:299, crates/mathtools/src/check_cache.rs:192 |
| `/proc/self/mountinfo` | crates/core/src/system.rs:592 |
| `/proc/self/ns/user` | crates/legacy/src/nscache.rs:319 |
| `/proc/self/pagemap` | crates/boundary/src/classed.rs:132 |
| `/proc/self/status` | crates/legacy/src/cred_compat.rs:55 |
| `/proc/self/task/` | crates/legacy/src/pthread_compat.rs:345 |
| `/proc/self/task/4321/comm` | crates/legacy/src/pthread_compat.rs:436 |
| `/proc/stat` | crates/core/src/system.rs:321 |
| `/proc/sys/kernel/ngroups_max` | crates/core/src/system.rs:492 |
| `/proc/sys/kernel/random/boot_id` | crates/legacy/src/nscache.rs:347 |
| `/proc/sys/vm/mmap_min_addr` | crates/boundary/src/dispatch.rs:340 |
| `/proc/thread-self/status` | crates/core/src/system.rs:228 |
| `/sys/devices/system/cpu` | crates/core/src/system.rs:351 |
| `/sys/devices/system/cpu/online` | crates/core/src/system.rs:331 |
| `/usr/` | crates/core/src/catalog/xopen.rs:197 |
| `/usr/bin/ls` | crates/rt/src/start.rs:492 |
| `/usr/bin/prog` | crates/legacy/src/argp.rs:72 |
| `/usr/lib` | crates/ld/src/arch.rs:305 |
| `/usr/lib/locale` | crates/core/src/locale/mod.rs:533 |
| `/usr/lib/locale/locale-archive` | crates/core/src/locale/mod.rs:535 |
| `/usr/lib/x86_64-linux-gnu` | crates/ld/src/arch.rs:305 |
| `/usr/share/i18n/charmaps/` | crates/legacy/src/global_locale.rs:96 |
| `/usr/share/locale` | crates/legacy/src/intl.rs:62 |
| `/usr/share/locale-langpack` | crates/legacy/src/intl.rs:66 |
| `/usr/share/locale/locale.alias` | crates/core/src/locale/mod.rs:537 |
| `/usr/share/zoneinfo` | crates/core/src/time.rs:41 |
| `/var/log/wtmp` | crates/legacy/src/accounting.rs:40 |
| `/var/run/utmp` | crates/legacy/src/accounting.rs:38 |

## Shared helpers (errno save/restore, reading files, ...)

* crates/boundary/src/errno.rs: `set` :11, `get` :18, `set_raw` :23
* crates/core/src/system.rs: `read_file` :99, `find_line` :119, `hostname` :173, `domainname` :181, `loadavg` :189, `seccomp_mode` :227, `new` :258, `feed` :264, `count_cpu_list` :294, `nprocs` :330, `nprocs_conf` :349, `pages` :386, `min_sigstksz` :397, `sigstksz` :419, `cache` :469, `sysconf` :478, `link_max` :552, `filesize_bits` :571, `has_symlinks` :581
* crates/kernel/src/errno.rs: `name` :47

## Moving an export into assembly (checklist, COPY-FILL 2026-10-10)

1. Define the C name as a second label on the csys kernel under `#ifdef LIELLA_EXPORT` (`csys/src/machine/fold.S` shows
   it); no `.macro`; `.p2align 6` per entry (D-4.22 MEM-K5b).
2. Keep the Rust function as an `export! { asm ... }` test twin (never `no_mangle`) so the boundary tests still reach the
   assembly; paths the assembly leaves go to an `export! { local ... }` function (hidden C name, tail-jumped to).
3. unsafe-audit: `unsafe` for the export machinery is allowed only in `crates/boundary/src/export.rs`; every block needs
   `// UNSAFE(<category>) SAFETY:` with the category the audit allows for that file.
4. xtask/src/copies.rs: a member that DEFINES memcpy/memmove/memset must also define the `liella_internal_*` entry unless
   it never uses the name (the asm object); check `cargo xtask so` still passes.
5. Symbol versions: `nm -D --defined-only target/liella/libc.so.6` shows the name with its GLIBC_x version and no new
   `liella_` global outside LIELLA_NATIVE_1.
6. `cargo xtask opaque-audit` (tests calling exported names go through selftest::opaque), then `cargo xtask static` and
   `cargo xtask clippy` before vm_affected.
7. Data the asm reads and Rust writes: declare it in ffi.rs with an atomic type of the same layout (AtomicUsize), never a
   plain static.

## Layout contracts (LAYOUT-MANIFEST, D-X.48)

Every deliberate code or data layout of the products (64-byte entries, the classed pages, JCC padding, `.lbss`,
the start-up names window, the gettext slots page, the memchr/rawmemchr `.org` pins, path remapping) is a row of
[`layout/contracts.tsv`](../../layout/contracts.tsv): `grep -P '^MEMCHR-SSE2-PIN\t' layout/contracts.tsv`. `cargo xtask so`
checks every row on the products it built (`cargo xtask layout-check [<products dir>]` by hand); `cargo xtask static`
checks the rows against `plan/07-decisions/` and the `LAYOUT(<ID>)` tags at the source directives. Changing a
row changes a decision: see D-X.48 in `plan/07-decisions/x-implementation-decisions.md`.
