// Specified from spec C0.1–C0.8 (C0.1 clauses 0–2, C0.2, C0.4, C0.5 and C0.7: milestone 5, 2026-10-04,
// through the conformance programs conformance/c/s00_*.c).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §0 (22 clauses).

#![allow(clippy::arithmetic_side_effects, clippy::identity_op, clippy::drop_non_drop, clippy::bool_assert_comparison, clippy::erasing_op, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/kernel.rs"]
mod kernel;

use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::machine::Machine;
use liella_kernel::mem::KPtrArray;
// ARCH(x86_64): checks the thread pointer through ARCH_GET_FS; TPIDR_EL0 on aarch64 (A-4)
use liella_kernel::ops::ArchGet;
use liella_kernel::sys::{FutexArg, Sys};
use liella_kernel::uapi::*;
use liella_kernel::{auxv, Errno, Fd, KPathBuf, KStr};
use spectest::clause;

const K: CSys = CSys;

fn kp(s: &str) -> KPathBuf {
    KPathBuf::new(s.as_bytes()).unwrap()
}

fn auxv_words() -> Vec<u64> {
    let bytes = std::fs::read("/proc/self/auxv").unwrap();
    let mut words = vec![0u64; bytes.len() / 8];
    let n = auxv::words_from_bytes(&bytes, &mut words);
    words.truncate(n);
    words
}

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// Arm a one-shot SIGALRM after `ms` milliseconds (no allocation: used in children).
fn arm_alarm(k: &CSys, ms: i64) {
    let v = Itimerval { interval: Timeval::default(), value: Timeval { sec: 0, usec: ms * 1000 } };
    k.setitimer(ITIMER_REAL as u32, &v).unwrap();
}

/// C0.1: "unless an entry says otherwise": the defaults hold across the sections, and the entries
/// that state another convention follow it (`posix_memalign` and `pthread_join` return the error
/// number; `fts_read` sets `errno` to 0 at the end, fts(3); D-13.14). Checked as C code sees it
/// by `conformance/c/s00_errno.c`.
#[test]
fn s0_6_c0_1_0() {
    conformance(&["s00_errno"]);
}
/// C0.1: a failing call returns its sentinel and stores a positive code in the calling thread's
/// `errno`: -1, NULL, EOF, `(size_t)-1`, `MAP_FAILED`, `(iconv_t)-1`; another thread's failure
/// does not reach this thread's `errno` (`conformance/c/s00_errno.c`, through C), and here
/// through the exports.
#[test]
fn s0_6_c0_1_1() {
    use liella_boundary::errno;
    use liella_boundary::posix::{s11_lowio as io, s12_stdio as st};
    errno::set_raw(0);
    assert_eq!((io::close(-1), errno::get()), (-1, Errno::EBADF.get()));
    errno::set_raw(0);
    assert!(st::fopen(c"/nonexistent/c01".as_ptr(), c"r".as_ptr()).is_null());
    assert_eq!(errno::get(), Errno::ENOENT.get());
    errno::set_raw(77);
    let other = std::thread::spawn(|| {
        errno::set_raw(0);
        io::close(-1);
        errno::get()
    })
    .join()
    .unwrap();
    assert_eq!((other, errno::get()), (Errno::EBADF.get(), 77), "errno is the calling thread's");
    conformance(&["s00_errno"]);
}
/// C0.1: no library function sets `errno` to 0: a sweep of successful calls across the
/// sections, as C code makes them (`conformance/c/s00_errno.c`), leaves a nonzero `errno`
/// nonzero. It found `glob(GLOB_ALTDIRFUNC)` leaving the 0 it stores to detect a callback's
/// failure (fixed, milestone 5).
#[test]
fn s0_6_c0_1_2() {
    conformance(&["s00_errno"]);
}
/// C0.2: functions are MT-Safe unless stated: eight threads run a mix of them (formatting,
/// parsing, sorting, regular expressions, broken-down time, allocation) at once, and every result
/// equals the single-threaded one (`conformance/c/s00_threads.c`).
#[test]
fn s0_6_c0_2_0() {
    conformance(&["s00_threads"]);
}
/// C0.2: static result buffers: the next call overwrites the library's storage and the `_r`
/// form writes the caller's; per thread where D-C0.2 gives them a slot (another thread's calls
/// leave this thread's result alone). `conformance/c/s00_threads.c`, and for every function with
/// a library-owned result `conformance/c/s00_result_buffers.c` (two threads at once, eight under
/// load, and the storage released when a thread ends; milestone 6).
#[test]
fn s0_6_c0_2_1() {
    conformance(&["s00_threads", "s00_result_buffers"]);
}
/// C0.2: the process-global state the contract exposes: what one thread changes (the
/// environment, the current directory, the umask, a disposition, the global locale, `rand`'s
/// seed, `optind`) every thread sees (`conformance/c/s00_threads.c`).
#[test]
fn s0_6_c0_2_2() {
    conformance(&["s00_threads"]);
}
/// C0.2: async-signal safety: a handler that interrupts the library in non-reentrant work
/// (allocation, formatting, streams, the environment, time zones) 5,000 times calls AS-Safe
/// functions, which all answer correctly, and the interrupted work completes
/// (`conformance/c/s00_as_safe.c`).
#[test]
fn s0_6_c0_2_3() {
    conformance(&["s00_as_safe"]);
}
/// C0.2: cancellation points act on a pending deferred cancellation; other functions do not
/// (`conformance/c/s00_as_safe.c`: `close` acts, `strlen` does not, nothing acts while
/// cancellation is disabled); every listed cancellation point, section by section, is
/// `conformance/c/s22_cancel_points.c`.
#[test]
fn s0_6_c0_2_4() {
    conformance(&["s00_as_safe", "s22_cancel_points"]);
}
/// C0.3: blocking calls interrupted by a handler restart under `SA_RESTART` or fail with
/// `EINTR`; some never restart. (Kernel ABI, observed through the C layer.)
#[test]
fn s0_6_c0_3_0() {
    // A restartable wait (wait4) with and without SA_RESTART.
    assert_eq!(selftest::in_child(|k| {
        let pid = selftest::spawn_exiting_after(300, 4).unwrap();
        let (r, n) = selftest::with_handler_flags(SIGALRM, 0, || {
            arm_alarm(k, 50);
            k.wait4(pid, 0, None)
        }).unwrap();
        if r != Err(Errno::EINTR) || n != 1 {
            return 1;
        }
        let (r, n) = selftest::with_handler_flags(SIGALRM, SA_RESTART, || {
            arm_alarm(k, 50);
            k.wait4(pid, 0, None)
        }).unwrap();
        i32::from(r.map(|(p, s)| (p, s >> 8)) != Ok((pid, 4)) || n != 1) * 2
    }).unwrap(), ChildEnd::Exited(0));
    // Calls that never restart, even under SA_RESTART: poll, nanosleep, sigsuspend-style waits.
    assert_eq!(selftest::in_child(|k| {
        let (r, _) = selftest::with_handler_flags(SIGALRM, SA_RESTART, || {
            arm_alarm(k, 50);
            k.ppoll(&mut [], Some(&mut Timespec { sec: 5, nsec: 0 }), None)
        }).unwrap();
        if r != Err(Errno::EINTR) {
            return 1;
        }
        let (r, _) = selftest::with_handler_flags(SIGALRM, SA_RESTART, || {
            arm_alarm(k, 50);
            k.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 5, nsec: 0 }, None)
        }).unwrap();
        if r != Err(Errno::EINTR) {
            return 2;
        }
        let (r, _) = selftest::with_handler_flags(SIGALRM, SA_RESTART, || {
            arm_alarm(k, 50);
            k.pause()
        }).unwrap();
        i32::from(r != Err(Errno::EINTR)) * 3
    }).unwrap(), ChildEnd::Exited(0));
}
/// C0.4: 64-bit types; each `*64` symbol is an exact alias of its unsuffixed one: the same answers
/// beyond 32-bit offsets (`conformance/c/s00_largefile.c`), and every `*64` name of the reference
/// whose unsuffixed name the products export is exported too (D-C0.4).
#[test]
fn s0_6_c0_4_0() {
    conformance(&["s00_largefile"]);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = std::process::Command::new("nm").args(["-g", "--defined-only"]).arg(root.join("target/liella/libc.a")).output();
    let Ok(o) = o.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()) else { return };
    let ours: std::collections::BTreeSet<&str> = o.lines().filter_map(|l| l.split_whitespace().nth(2)).collect();
    let versions = std::fs::read_to_string(root.join("spec/abi/versions.tsv")).unwrap();
    let reference: std::collections::BTreeSet<&str> = versions.lines().filter(|l| l.starts_with("libc.so.6\t")).filter_map(|l| l.split('\t').nth(1)).collect();
    let missing: Vec<&str> = reference
        .iter()
        .filter(|n| n.ends_with("64") && !n.starts_with("__") && !n.contains("int64") && !n.contains("_64"))
        .filter(|n| ours.contains(&n[..n.len() - 2]) && reference.contains(&n[..n.len() - 2]))
        .filter(|n| !ours.contains(**n))
        .copied()
        .collect();
    assert!(missing.is_empty(), "*64 aliases missing beside their base names: {missing:?}");
}
/// C0.5: `__<name>_chk` is `<name>` with the destination's size: within it the same result, beyond
/// it `__chk_fail`'s "buffer overflow detected" and SIGABRT (`conformance/c/s02_chk_fail.c`);
/// the format forms reject `%n` in writable memory (`conformance/c/s12_symbols_printf_chk.c`).
#[test]
fn s0_6_c0_5_0() {
    use liella_boundary::posix::s04_strings as st;
    let src = *b"fortify\0";
    let (mut a, mut b) = ([0u8; 8], [0u8; 8]);
    assert_eq!(st::__memcpy_chk(a.as_mut_ptr().cast(), src.as_ptr().cast(), 8, 8), a.as_mut_ptr().cast());
    st::memcpy(b.as_mut_ptr().cast(), src.as_ptr().cast(), 8);
    assert_eq!(a, b, "within the object: memcpy's result");
    let mut c = [0 as core::ffi::c_char; 8];
    assert_eq!(st::__strcpy_chk(c.as_mut_ptr(), src.as_ptr().cast(), 8), c.as_mut_ptr());
    assert_eq!(c.map(|v| v as u8), src);
    conformance(&["s02_chk_fail", "s12_symbols_printf_chk"]);
}
clause!(s0_6_c0_6_0, "0.6", "Convention", r##"Every exported name carries a version node (`GLIBC_2.2.5` is the x86_64 baseline). Some names have several versions with different contracts (for example `memcpy@GLIBC_2.2.5` permits overlapping arguments, while `memcpy@GLIBC_2.14` does not guarantee it; `realpath`, `sched_setaffinity`, `pthread_cond_*`, `regexec`, `glob`, `posix_spawn`, `nftw`, `fmemopen`, `quick_exit` all have older versioned contracts). Some names are exported only for old binaries and can no longer be linked against (`gets`, `matherr`, `__malloc_hook`, `sys_errlist`, `pow10`, `sigvec`, `ustat`, `stime`, `_sys_siglist`, the RPC/NIS stack moved to `libtirpc`/`libnsl`, ...). ⚑ COMPAT: an ABI-compatible replacement must keep every versioned contract that old binaries were linked against. Appendix B lists these."##);
/// C0.7: borrowed arguments are copied (`setenv`, `regcomp`, `fopen`'s mode; `putenv` states
/// otherwise and keeps the string); returned pointers are caller-owned heap blocks (`strdup`,
/// `realpath(p, NULL)`, `getcwd(NULL, 0)`, `asprintf`, `open_memstream`, `scandir`), library
/// storage (C0.2), or views into the caller's object (`strchr`, `strstr`, `strtok_r`,
/// `realpath(p, buf)`, `getcwd(buf, n)`), as each entry states (`conformance/c/s00_ownership.c`).
#[test]
fn s0_6_c0_7_0() {
    conformance(&["s00_ownership"]);
}
/// C0.8: the kernel ABI includes more than system-call numbers. Each listed interface is
/// reachable through the boundary.
#[test]
fn s0_6_c0_8_0() {
    assert!(auxv_words().len() > 10);
    assert!(K.vdso_clock_gettime_available());
    assert_ne!(K.thread_pointer(), 0);
    assert_eq!(selftest::in_child(|_| i32::from(selftest::signal_roundtrip(SIGUSR1) != Ok((1, SIGUSR1)))).unwrap(), ChildEnd::Exited(0));
}
/// C0.8: process entry state — argc, argv, envp and the auxiliary vector as the kernel lays
/// them out for a new image.
#[test]
fn s0_6_c0_8_1() {
    if std::env::var("LIELLA_PROBE").as_deref() == Ok("entry") {
        let args: Vec<String> = std::env::args().collect();
        let ok = args.len() == 5 && args[0] == "entry-probe" && std::env::var("ONLY").as_deref() == Ok("this")
            && std::env::vars().count() == 2;
        K.exit_group(if ok { 46 } else { 1 });
    }
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let exe = kp(exe.to_str().unwrap());
    let args = [kp("entry-probe"), kp("s0_6_c0_8_1"), kp("--exact"), kp("--test-threads=1"), kp("-q")];
    let env = [kp("LIELLA_PROBE=entry"), kp("ONLY=this")];
    let a: Vec<KStr<'_>> = args.iter().map(|p| p.as_kstr()).collect();
    let e: Vec<KStr<'_>> = env.iter().map(|p| p.as_kstr()).collect();
    let (mut sa, mut se) = ([0usize; 8], [0usize; 4]);
    let argv = KPtrArray::build(&a, &mut sa).unwrap();
    let envp = KPtrArray::build(&e, &mut se).unwrap();
    assert_eq!(selftest::exec_child(exe.as_kstr(), &argv, &envp).unwrap(), ChildEnd::Exited(46));
}
/// C0.8: the auxiliary vector carries the listed keys.
#[test]
fn s0_6_c0_8_2() {
    let words = auxv_words();
    let av = auxv::Auxv::from_words(&words);
    for key in [AT_PAGESZ, AT_HWCAP, AT_HWCAP2, AT_RANDOM, AT_SYSINFO_EHDR, AT_SECURE, AT_PHDR, AT_ENTRY, AT_BASE, AT_EXECFN, AT_PLATFORM, AT_CLKTCK, AT_UID, AT_EUID, AT_GID, AT_EGID, AT_MINSIGSTKSZ] {
        // x86 has AT_MINSIGSTKSZ since Linux 5.14.
        if key == AT_MINSIGSTKSZ && kernel::before(5, 14) {
            kernel::note("AT_MINSIGSTKSZ on x86", "5.14");
            continue;
        }
        assert!(av.get(key).is_some(), "AT_{key} missing");
    }
}
/// C0.8: the vDSO provides clock_gettime, gettimeofday, time, getcpu and clock_getres, and
/// they agree with the system calls.
#[test]
fn s0_6_c0_8_3() {
    assert!(K.vdso_clock_gettime_available());
    let a = K.clock_gettime(CLOCK_MONOTONIC).unwrap();
    let b = K.clock_gettime_syscall(CLOCK_MONOTONIC).unwrap();
    assert!((a.sec, a.nsec) <= (b.sec, b.nsec));
    assert_eq!(K.clock_getres(CLOCK_REALTIME).unwrap(), K.clock_getres_syscall(CLOCK_REALTIME).unwrap());
    assert!((K.gettimeofday().unwrap().0.sec - K.gettimeofday_syscall().unwrap().0.sec).abs() <= 1);
    assert!((K.time().unwrap() - K.time_syscall().unwrap()).abs() <= 1);
    assert!(K.getcpu().unwrap().0 < 4096);
}
/// C0.8: the signal frame and the SA_RESTORER / rt_sigreturn convention.
#[test]
fn s0_6_c0_8_4() {
    assert_eq!(selftest::in_child(|_| {
        let ok = selftest::signal_roundtrip(SIGUSR2) == Ok((1, SIGUSR2)) && selftest::altstack_roundtrip(SIGUSR1) == Ok(true);
        i32::from(!ok)
    }).unwrap(), ChildEnd::Exited(0));
}
/// C0.8: the thread pointer register (FS base): self-pointing, per thread.
#[test]
fn s0_6_c0_8_5() {
    let tp = K.thread_pointer();
    // ARCH(x86_64): checks the thread pointer through ARCH_GET_FS; TPIDR_EL0 on aarch64 (A-4)
    assert_eq!(K.arch_prctl_get(ArchGet::Fs).unwrap() as usize, tp);
    assert_ne!(std::thread::spawn(|| K.thread_pointer()).join().unwrap(), tp);
}
/// C0.8: shared user/kernel memory protocols — futex words, the robust list, clear_child_tid.
#[test]
fn s0_6_c0_8_6() {
    let word = selftest::robust_list_owner_died().unwrap();
    assert_eq!(word & FUTEX_OWNER_DIED, FUTEX_OWNER_DIED);
    let w = std::sync::atomic::AtomicU32::new(0);
    assert_eq!(K.futex(&w, FUTEX_WAIT | FUTEX_PRIVATE_FLAG, 1, FutexArg::None, None, 0), Err(Errno::EAGAIN));
    static TID_WORD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    assert_eq!(selftest::in_child(|k| i32::from(k.set_tid_address(&TID_WORD) != Ok(k.gettid()))).unwrap(), ChildEnd::Exited(0));
}
/// C0.8: pseudo-file systems the library depends on.
#[test]
fn s0_6_c0_8_7() {
    let mut buf = [0u8; 4096];
    let n = K.readlinkat(Fd::CWD, kp("/proc/self/exe").as_kstr(), &mut buf).unwrap();
    assert_eq!(std::path::Path::new(std::str::from_utf8(&buf[..n]).unwrap()), std::env::current_exe().unwrap());
    for p in ["/proc/self/fd", "/proc/self/maps", "/proc/stat", "/proc/meminfo", "/proc/sys/kernel/ngroups_max", "/sys/devices/system/cpu/online", "/dev/ptmx", "/dev/null"] {
        assert!(K.fstatat(Fd::CWD, kp(p).as_kstr(), 0).is_ok(), "{p}");
    }
}
/// C0.8: netlink NETLINK_ROUTE is available for interface enumeration.
#[test]
fn s0_6_c0_8_8() {
    let s = K.socket(AF_NETLINK, SOCK_RAW | SOCK_CLOEXEC, NETLINK_ROUTE).unwrap();
    let mut req = [0u8; 32];
    req[..4].copy_from_slice(&32u32.to_ne_bytes());
    req[4..6].copy_from_slice(&RTM_GETLINK.to_ne_bytes());
    req[6..8].copy_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    assert_eq!(K.write(s, &req).unwrap(), 32);
    let mut buf = vec![0u8; 1 << 16];
    let n = K.read(s, &mut buf).unwrap();
    assert!(n >= 16);
    assert_eq!(u16::from_ne_bytes([buf[4], buf[5]]), RTM_NEWLINK);
    K.close(s).unwrap();
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s0_6_c0_1_3() {
    // The error codes are the kernel's numbers; a failure the library detects itself uses a
    // kernel code with its library meaning (EINVAL for a bad argument).
    assert_eq!((Errno::EPERM.get(), Errno::EINVAL.get(), Errno::EOVERFLOW.get(), Errno::EHWPOISON.get()), (1, 22, 75, 133));
    assert_eq!(CSys.close(Fd::new(-1)), Err(Errno::EBADF), "the kernel's EBADF, 9");
    assert_eq!(Errno::EBADF.get(), 9);
    let mut p = core::ptr::null_mut();
    assert_eq!(liella_boundary::posix::s03_memory::posix_memalign(Some(&mut p), 3, 8), Errno::EINVAL.get(), "library-detected: EINVAL");
}

#[test]
fn s0_6_c0_1_4() {
    // The newer interfaces return the error number and leave errno alone.
    use liella_boundary::errno;
    errno::set_raw(1234);
    let mut p = core::ptr::null_mut();
    assert_eq!(liella_boundary::posix::s03_memory::posix_memalign(Some(&mut p), 3, 8), Errno::EINVAL.get());
    assert_eq!(errno::get(), 1234, "posix_memalign: errno untouched");
    assert_eq!(liella_boundary::posix::s22_threads::pthread_join(liella_abi::layout::pthread_t(0x10), None), Errno::ESRCH.get());
    assert_eq!(errno::get(), 1234, "pthread_join: errno untouched");
}

// opaque-ok: audited in GATE-SPEED-4/5: the direct calls here use their results and pass at opt-level 1 and 2 in both pass types; any case LLVM folds goes through selftest::opaque.
