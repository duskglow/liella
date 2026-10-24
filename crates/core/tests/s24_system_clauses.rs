// Specified in part from spec §24 (phase 5: §24.1–24.4; the validation stage: §24.5–24.6; the
// compat entries, s24_6_c_6: phase 15, LEGACY-STUBS).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §24: the calls through the exports; the computed configuration values
//! through `liella::system` and `liella_legacy::sys_compat`, with the kernel's answers injected
//! by `Faulty` for the fallbacks.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "support/scratch.rs"]
mod scratch;

use core::ptr::null_mut;

use liella::error::Errno;
use liella::system::{self, Aux, Query};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s24_system as x;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{Rlimit, Rusage};
use liella_legacy::sys_compat;
use spectest::clause;

const RLIM_INFINITY: u64 = u64::MAX;

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

/// An auxiliary vector with chosen entries.
struct FakeAux(&'static [(u64, u64)]);
impl Aux for FakeAux {
    fn aux(&self, key: u64) -> Option<u64> {
        self.0.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }
}

/// Runs `body` in a child (this test re-executed alone): limits and priorities are per process.
fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_C24_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env("LIELLA_C24_ALONE", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{name} failed in its child: {}", child_report(&o));
}

fn rlimit(res: i32) -> Rlimit {
    let mut l = Rlimit { cur: 0, max: 0 };
    assert_eq!(x::getrlimit(res, Some(&mut l)), 0);
    l
}

fn root() -> bool {
    CSys.geteuid() == 0
}

/// The host glibc's `sysconf(name)`.
fn host_sysconf(name: i32) -> i64 {
    let dir = scratch::Scratch::dir(&format!("c24-{name}"));
    let c = dir.join("p.c");
    std::fs::write(&c, format!("#include <unistd.h>\n#include <stdio.h>\nint main(void){{ printf(\"%ld\", sysconf({name})); return 0; }}\n")).unwrap();
    let exe = dir.join("p");
    assert!(std::process::Command::new("cc").arg(&c).arg("-o").arg(&exe).status().unwrap().success());
    let out = std::process::Command::new(&exe).output().unwrap();
    String::from_utf8(out.stdout).unwrap().parse().unwrap()
}

#[test]
fn s24_1_c_0() {
    alone("s24_1_c_0", || {
        let l = rlimit(7);
        let lower = Rlimit { cur: l.cur.min(100), max: l.max };
        let set = x::setrlimit(7, Some(&lower)) == 0;
        let now = rlimit(7);
        set && now == lower && now.cur <= now.max
    });
}

#[test]
fn s24_1_c_1() {
    // Every named resource can be read: RLIMIT_CPU (0) .. RLIMIT_RTTIME (15).
    for res in 0..16 {
        let l = rlimit(res);
        assert!(l.cur <= l.max, "resource {res}");
    }
    errno::set_raw(0);
    let mut l = Rlimit { cur: 0, max: 0 };
    assert_eq!((x::getrlimit(16, Some(&mut l)), errno::get()), (-1, Errno::EINVAL.get()), "RLIM_NLIMITS is not a resource");
}

#[test]
fn s24_1_c_2() {
    let core_ = rlimit(4);
    let cpu = rlimit(0);
    assert!(cpu.cur == RLIM_INFINITY || cpu.cur > 0, "RLIMIT_CPU is usually unlimited");
    let text = std::fs::read_to_string("/proc/self/limits").unwrap();
    let row = text.lines().find(|l| l.starts_with("Max cpu time")).unwrap();
    assert_eq!(row.contains("unlimited"), cpu.cur == RLIM_INFINITY, "RLIM_INFINITY is unlimited");
    let _ = core_;
}

#[test]
fn s24_1_c_3() {
    alone("s24_1_c_3", || {
        let l = rlimit(4);
        let lower = l.max.min(1 << 20);
        let lowered = x::setrlimit(4, Some(&Rlimit { cur: 0, max: lower })) == 0;
        errno::set_raw(0);
        let raised = x::setrlimit(4, Some(&Rlimit { cur: 0, max: RLIM_INFINITY }));
        lowered && (root() || (raised == -1 && errno::get() == Errno::EPERM.get()))
    });
}

#[test]
fn s24_1_c_4() {
    let child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
    let pid = child.id() as i32;
    let mut l = Rlimit { cur: 0, max: 0 };
    assert_eq!(x::prlimit(pid, 7, None, Some(&mut l)), 0, "another process's limit");
    let new = Rlimit { cur: 64, max: l.max };
    assert_eq!(x::prlimit(pid, 7, Some(&new), None), 0, "set on the child (same credentials)");
    let text = std::fs::read_to_string(format!("/proc/{pid}/limits")).unwrap();
    let row = text.lines().find(|r| r.starts_with("Max open files")).unwrap();
    assert!(row.split_whitespace().any(|w| w == "64"));
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn s24_1_c_5() {
    let l = rlimit(1);
    let want = if l.cur == RLIM_INFINITY { i64::MAX } else { (l.cur / 512) as i64 };
    assert_eq!(x::liella_ulimit2(1, 0), want, "UL_GETFSIZE: 512-byte blocks");
    alone("s24_1_c_5", || x::liella_ulimit2(2, 10) == 10 && rlimit(1).cur == 5120);
}

#[test]
fn s24_2_c_0() {
    let mut me = Rusage::default();
    let mut kids = Rusage::default();
    let mut th = Rusage::default();
    std::process::Command::new("true").status().unwrap();
    assert_eq!(x::getrusage(0, Some(&mut me)), 0);
    assert_eq!(x::getrusage(-1, Some(&mut kids)), 0);
    assert_eq!(x::getrusage(1, Some(&mut th)), 0);
    assert!(me.maxrss > 0 && me.minflt > 0, "max RSS and faults");
    assert!(kids.maxrss > 0, "the waited-for child counted");
    assert!(me.nvcsw + me.nivcsw > 0, "context switches");
    assert_eq!((me.ixrss, me.idrss, me.isrss, me.nswap), (0, 0, 0, 0), "BSD fields that are always 0 on Linux");
}

#[test]
fn s24_3_c_1() {
    alone("s24_3_c_1", || {
        let first = x::nice(0);
        errno::set_raw(0);
        let n = x::nice(-1 - first); // to -1 needs privilege unless root
        let neg_one_valid = root() && n == -1 && errno::get() == 0;
        let refused = !root() && n == -1 && errno::get() == Errno::EPERM.get();
        errno::set_raw(0);
        let up = x::nice(2);
        (neg_one_valid || refused) && up == first + 2
    });
    let k = Faulty::new(CSys);
    k.fail("setpriority", Errno::EACCES, 0, 1);
    assert_eq!(sys_compat::nice(&k, 1), Err(Errno::EPERM), "EACCES is reported as EPERM");
}

#[test]
fn s24_3_c_2() {
    let me = std::process::id();
    let pgrp: u32 = std::fs::read_to_string("/proc/self/stat").unwrap().rsplit(") ").next().unwrap().split(' ').nth(2).unwrap().parse().unwrap();
    let uid = CSys.getuid();
    errno::set_raw(0);
    let p = x::getpriority(0, me);
    assert!((-20..=19).contains(&p) && errno::get() == 0, "PRIO_PROCESS");
    assert!((-20..=19).contains(&x::getpriority(1, pgrp)), "PRIO_PGRP");
    assert!((-20..=19).contains(&x::getpriority(2, uid)), "PRIO_USER: the highest of the user's processes");
    assert_eq!(x::setpriority(0, me, p), 0);
}

#[test]
fn s24_4_c_1() {
    let k = Faulty::new(CSys);
    assert_eq!(sys_compat::sysconf(&k, &CSys, &liella_boundary::process::ProcessAux, 29), Ok(Some(200809)), "_SC_VERSION");
    assert_eq!(sys_compat::sysconf(&k, &CSys, &liella_boundary::process::ProcessAux, 4), Ok(Some(i64::try_from(rlimit(7).cur).unwrap_or(i64::MAX).min(i64::from(i32::MAX)))), "a runtime limit");
}

#[test]
fn s24_4_c_1_1() {
    let stack = rlimit(3);
    let want = if stack.cur == RLIM_INFINITY { i64::MAX } else { (stack.cur / 4).max(131_072) as i64 };
    if stack.cur != RLIM_INFINITY {
        assert_eq!(x::sysconf(0), want);
    }
    // The floor and the fallback, with the kernel's answers injected.
    let k = Faulty::new(CSys);
    k.fail("prlimit", Errno::EINVAL, 0, 1);
    assert_eq!(system::sysconf(&k, &CSys, &FakeAux(&[]), Query::ArgMax), Ok(Some(131_072)), "without the limit: 128 KiB");
}

#[test]
fn s24_4_c_1_2() {
    let aux = FakeAux(&[(6, 16384), (17, 250)]);
    assert_eq!(system::sysconf(&CSys, &CSys, &aux, Query::PageSize), Ok(Some(16384)), "AT_PAGESZ");
    assert_eq!(system::sysconf(&CSys, &CSys, &aux, Query::ClkTck), Ok(Some(250)), "AT_CLKTCK");
    assert_eq!(system::sysconf(&CSys, &CSys, &FakeAux(&[]), Query::ClkTck), Ok(Some(100)), "else 100");
    for name in [1, 2, 3, 4, 30] {
        assert_eq!(x::sysconf(name), host_sysconf(name), "name {name}");
    }
    alone("s24_4_c_1_2", || {
        let _ = CSys.prlimit(0, 6, Some(&Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY }));
        errno::set_raw(0);
        let child = x::sysconf(1);
        !root() || (child == -1 && errno::get() == 0)
    });
}

#[test]
fn s24_4_c_1_3() {
    for name in [83, 84, 85] {
        assert_eq!(x::sysconf(name), host_sysconf(name), "name {name}");
    }
    let av = x::sysconf(86);
    assert!(av > 0 && av <= x::sysconf(85));
    assert_eq!(system::count_cpu_list(b"0-3,8,10-11\n"), Some(7), "the sysfs list format");
    assert_eq!(system::count_cpu_list(b"0\n"), Some(1));
    assert_eq!(system::count_cpu_list(b"x"), None);
}

#[test]
fn s24_4_c_1_4() {
    for name in 185..=199 {
        assert_eq!(x::sysconf(name), host_sysconf(name), "cache name {name}");
    }
    let line = x::sysconf(190);
    assert!(line == 64 || line == 128 || line == -1, "a plausible line size from CPUID");
}

#[test]
fn s24_4_c_1_5() {
    for name in [249, 250, 75] {
        assert_eq!(x::sysconf(name), host_sysconf(name), "name {name}");
    }
    // Without AT_MINSIGSTKSZ (kernels before 5.14), the frame computed from the XSAVE size.
    let emulated = system::min_sigstksz(&FakeAux(&[]), &CSys);
    assert!(emulated > 1024 && emulated % 4 == 0);
    assert_eq!(system::sigstksz(1776), 8192, "four times, at least SIGSTKSZ");
    assert_eq!(system::sigstksz(4000), 16000);
}

#[test]
fn s24_4_c_1_6() {
    assert_eq!(x::sysconf(70), 1024, "_SC_GETPW_R_SIZE_MAX");
    assert_eq!(x::sysconf(69), 1024, "_SC_GETGR_R_SIZE_MAX");
}

#[test]
fn s24_4_c_1_7() {
    errno::set_raw(1234);
    assert_eq!(x::sysconf(181), -1, "_SC_TRACE: not supported");
    assert_eq!(errno::get(), 1234, "without changing errno");
    errno::set_raw(0);
    assert_eq!((x::sysconf(41), errno::get()), (-1, Errno::EINVAL.get()), "_SC_EQUIV_CLASS_MAX: EINVAL, as glibc");
    assert_eq!((x::sysconf(-1), errno::get()), (-1, Errno::EINVAL.get()), "unknown");
}

#[test]
fn s24_4_c_2() {
    let fd = liella_boundary::posix::s11_lowio::liella_open3(c"/".as_ptr(), 0o200000, 0);
    for name in [0, 3, 4, 5, 6, 7, 13, 16, 17, 18, 20] {
        assert_eq!(x::pathconf(c"/".as_ptr(), name), x::fpathconf(fd, name), "the same answer by path and descriptor: {name}");
    }
    assert_eq!(x::pathconf(c"/proc".as_ptr(), 0), 127, "a file system without its own LINK_MAX: Linux's");
    assert_eq!(x::pathconf(c"/proc".as_ptr(), 13), 32, "FILESIZEBITS by f_type");
    liella_boundary::posix::s11_lowio::close(fd);
}

#[test]
fn s24_4_c_3() {
    let mut b = [0u8; 64];
    let n = x::confstr(2, b.as_mut_ptr().cast(), 64);
    assert_eq!(&b[..n], b"glibc 2.39\0", "_CS_GNU_LIBC_VERSION (the reference's, ABI)");
    assert_eq!(x::confstr(3, b.as_mut_ptr().cast(), 64), 10, "_CS_GNU_LIBPTHREAD_VERSION: NPTL 2.39");
    assert_eq!(x::confstr(1140, b.as_mut_ptr().cast(), 64), 5, "_CS_POSIX_V7_LP64_OFF64_CFLAGS: -m64");
    let mut tiny = [0u8; 2];
    assert_eq!(x::confstr(0, tiny.as_mut_ptr().cast(), 2), 14, "the full length, though truncated");
    assert_eq!(&tiny, b"/\0");
    assert_eq!(x::confstr(0, null_mut(), 0), 14);
}

#[test]
fn s24_4_c_4() {
    assert_eq!(i64::from(x::get_nprocs()), x::sysconf(84));
    assert_eq!(i64::from(x::get_nprocs_conf()), x::sysconf(83));
    assert_eq!(x::get_phys_pages(), x::sysconf(85));
    assert!(x::get_avphys_pages() > 0);
    // The fallbacks: without sysfs, /proc/stat; without that, the affinity mask.
    let k = Faulty::new(CSys);
    k.fail("openat", Errno::ENOENT, 0, 1);
    assert_eq!(system::nprocs(&k), system::nprocs(&CSys), "/proc/stat counts the same CPUs");
}

#[test]
fn s24_5_c_1() {
    let mut u = liella_kernel::uapi::Utsname::default();
    assert_eq!(x::uname(Some(&mut u)), 0);
    let k = CSys.uname().unwrap();
    assert_eq!((u.sysname, u.nodename, u.release, u.version, u.machine, u.domainname), (k.sysname, k.nodename, k.release, k.version, k.machine, k.domainname), "the kernel's fields");
    assert!(u.sysname.starts_with(b"Linux\0"));
    let k = Faulty::new(CSys);
    assert!(k.fail("uname", Errno::EFAULT, 0, 1));
    let mut b = [0u8; 8];
    assert_eq!(system::hostname(&k, &mut b), Err(Errno::EFAULT), "an error of uname is the caller's");
}
#[test]
fn s24_5_c_2() {
    let u = CSys.uname().unwrap();
    let len = u.nodename.iter().position(|&b| b == 0).unwrap();
    let mut b = [b'x'; 80];
    assert_eq!(x::gethostname(b.as_mut_ptr().cast(), len + 1), 0);
    assert_eq!(&b[..=len], &u.nodename[..=len], "the node name and its NUL");
    let mut b = [b'x'; 80];
    errno::set_raw(0);
    assert_eq!(x::gethostname(b.as_mut_ptr().cast(), len), -1);
    assert_eq!(errno::get(), Errno::ENAMETOOLONG.get(), "truncated: ENAMETOOLONG");
    assert_eq!(&b[..len], &u.nodename[..len], "the bytes that fit are written (the reference's)");
    assert_eq!(b[len], b'x');
    // getdomainname cuts silently (getdomainname(2)).
    let dl = u.domainname.iter().position(|&b| b == 0).unwrap();
    if dl > 1 {
        let mut d = [b'x'; 80];
        assert_eq!(x::getdomainname(d.as_mut_ptr().cast(), dl - 1), 0);
        assert_eq!(&d[..dl - 1], &u.domainname[..dl - 1]);
    }
}
#[test]
fn s24_6_c_1() {
    // Version 3: two 32-bit words per set (effective, permitted, inheritable).
    let mut hdr = [0x2008_0522u32, 0];
    let mut data = [0u32; 6];
    assert_eq!(x::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()), 0);
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let field = |name: &str| u64::from_str_radix(status.lines().find(|l| l.starts_with(name)).unwrap().split_whitespace().nth(1).unwrap(), 16).unwrap();
    assert_eq!(u64::from(data[0]) | u64::from(data[3]) << 32, field("CapEff:"));
    assert_eq!(u64::from(data[1]) | u64::from(data[4]) << 32, field("CapPrm:"));
    assert_eq!(x::capset(hdr.as_mut_ptr().cast(), data.as_ptr().cast()), 0, "the same sets again");
    {
        // capget/capset with the kernel's versioned header: an unknown version is EINVAL with the
        // kernel's preferred version written back (version 3, 0x20080522), and no data written;
        // version 3 reads the calling thread's two 32-bit words per set, which /proc agrees with.
        const V3: u32 = 0x2008_0522;
        let mut hdr = [0u32, 0];
        let mut data = [0xAAAA_AAAAu32; 6];
        errno::set_raw(0);
        assert_eq!((x::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()), errno::get()), (-1, Errno::EINVAL.get()));
        assert_eq!((hdr[0], data[0]), (V3, 0xAAAA_AAAA), "the preferred version, nothing read");
        assert_eq!(x::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()), 0);
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let eff = status.lines().find_map(|l| l.strip_prefix("CapEff:")).map(|v| u64::from_str_radix(v.trim(), 16).unwrap()).unwrap();
        assert_eq!(u64::from(data[0]) | (u64::from(data[3]) << 32), eff, "effective: words 0 and 3 (two {{effective, permitted, inheritable}} records)");
        // Writing the same sets back is allowed; a set beyond the permitted one is EPERM.
        assert_eq!(x::capset(hdr.as_mut_ptr().cast(), data.as_ptr().cast()), 0);
        if eff != u64::from(u32::MAX) | (0x1ff << 32) {
            let mut more = data;
            more[0] = u32::MAX;
            more[1] = u32::MAX;
            errno::set_raw(0);
            assert_eq!((x::capset(hdr.as_mut_ptr().cast(), more.as_ptr().cast()), errno::get()), (-1, Errno::EPERM.get()));
        }
        errno::set_raw(0);
        assert_eq!((x::capget(null_mut(), data.as_mut_ptr().cast()), errno::get()), (-1, Errno::EFAULT.get()), "a NULL header");
        // A namespace root's sets, raised and lowered with capset: admin_products.rs (userns).
    }
}
#[test]
fn s24_6_c_2() {
    let mut i = liella_kernel::uapi::Sysinfo::default();
    assert_eq!(x::sysinfo(Some(&mut i)), 0);
    let k = CSys.sysinfo().unwrap();
    assert_eq!((i.totalram, i.totalswap, i.mem_unit), (k.totalram, k.totalswap, k.mem_unit));
    assert!(i.uptime > 0 && i.procs > 0 && i.freeram <= i.totalram);
}
#[test]
fn s24_6_c_3() {
    let mut a = [-1f64; 5];
    assert_eq!(x::getloadavg(a.as_mut_ptr(), 5), 3, "up to three");
    assert_eq!(&a[3..], &[-1.0, -1.0]);
    assert_eq!(x::getloadavg(a.as_mut_ptr(), 2), 2);
    assert_eq!(x::getloadavg(a.as_mut_ptr(), 0), 0);
    assert_eq!(x::getloadavg(a.as_mut_ptr(), -1), 0, "a negative count stores none (the reference: 0)");
    // From /proc/loadavg: its unreadability is the error; its numbers are the values.
    let k = Faulty::new(CSys);
    assert!(k.fail("openat", Errno::EACCES, 0, 1));
    assert_eq!(system::loadavg(&k, &mut a[..3]), Err(Errno::EACCES));
    let text = std::fs::read_to_string("/proc/loadavg").unwrap();
    let want: Vec<f64> = text.split(' ').take(3).map(|v| v.parse().unwrap()).collect();
    assert_eq!(system::loadavg(&CSys, &mut a[..3]), Ok(3));
    for (x, y) in a.iter().zip(&want) {
        assert!((x - y).abs() < 1.0, "{x} vs {y} (the averages may move between the readings)");
    }
}
/// Thin wrappers: the kernel's answer, `errno` as the system call sets it. Without privilege the
/// kernel refuses; the same calls straight through `syscall` give the same answers.
#[test]
fn s24_6_c_4() {
    {
        // Thin wrappers: the kernel's answer, -1 with errno for its errors. Unprivileged, the kernel
        // refuses with EPERM except where it checks an argument first (ioperm beyond the bitmap,
        // iopl above 3, modify_ldt's unknown function: EINVAL, ENOSYS). The namespace cases (reboot
        // reaching a PID namespace's rules) and the comparison with the reference are in
        // admin_products.rs.
        let fails = |r: i32, e: Errno| r == -1 && errno::get() == e.get();
        if CSys.geteuid() != 0 {
            let path = c"/nonexistent/x";
            errno::set_raw(0);
            assert!(fails(x::acct(path.as_ptr()), Errno::EPERM), "acct");
            assert!(fails(x::init_module(null_mut(), 0, c"".as_ptr()), Errno::EPERM), "init_module");
            assert!(fails(x::delete_module(c"liella_nonesuch".as_ptr(), 0), Errno::EPERM), "delete_module");
            assert!(fails(x::ioperm(0x80, 1, 1), Errno::EPERM), "ioperm");
            assert!(fails(x::iopl(3), Errno::EPERM), "iopl");
            assert!(fails(x::reboot(0), Errno::EPERM), "reboot");
            assert!(fails(x::vhangup(), Errno::EPERM), "vhangup");
            let restricted = std::fs::read_to_string("/proc/sys/kernel/dmesg_restrict").is_ok_and(|v| v.trim() == "1");
            if restricted {
                assert!(fails(x::klogctl(10, null_mut(), 0), Errno::EPERM), "klogctl under dmesg_restrict");
            }
        }
        errno::set_raw(0);
        assert!(fails(x::ioperm(0x10000, 1, 1), Errno::EINVAL), "ioperm beyond the bitmap");
        assert!(fails(x::iopl(4), Errno::EINVAL), "iopl above 3");
        assert_eq!(x::ioperm(0x80, 1, 0), 0, "turning access off needs no privilege");
        assert_eq!(x::iopl(0), 0);
        // modify_ldt: the kernel's int-sized result read as an int: -1 with errno, never the
        // negated error number (D-24.11).
        let mut ldt = [0u8; 64];
        errno::set_raw(0);
        assert!(fails(x::modify_ldt(7, ldt.as_mut_ptr().cast(), 64), Errno::ENOSYS), "an unknown function");
        assert!(fails(x::modify_ldt(1, ldt.as_mut_ptr().cast(), 4), Errno::EINVAL), "a short descriptor");
        assert_eq!(x::modify_ldt(2, ldt.as_mut_ptr().cast(), 64), 64, "the default LDT: zeros");
        // quotactl passes the device path: "/" is not a block device.
        errno::set_raw(0);
        let mut info = [0u8; 64];
        assert!(fails(x::quotactl(0x8000_0500_u32 as i32, c"/".as_ptr(), 0, info.as_mut_ptr().cast()), Errno::ENOTBLK), "Q_GETINFO on /");
    }
    if CSys.geteuid() == 0 {
        return;
    }
    let raw = |nr: i64, a: [i64; 3]| {
        errno::set_raw(0);
        let r = x::liella_syscall7(nr, a[0], a[1], a[2], 0, 0, 0);
        (r, errno::get())
    };
    let call = |r: i32| (i64::from(r), errno::get());
    errno::set_raw(0);
    assert_eq!(call(x::reboot(0x12345)), raw(169, [0xfee1_dead, 672_274_793, 0x12345]), "reboot");
    errno::set_raw(0);
    assert_eq!(call(x::klogctl(10, null_mut(), 0)), raw(103, [10, 0, 0]), "klogctl");
    errno::set_raw(0);
    assert_eq!(call(x::acct(core::ptr::null())), raw(163, [0, 0, 0]), "acct");
    errno::set_raw(0);
    assert_eq!(call(x::ioperm(0x80, 1, 1)), raw(173, [0x80, 1, 1]), "ioperm");
    errno::set_raw(0);
    assert_eq!(call(x::iopl(3)), raw(172, [3, 0, 0]), "iopl");
    errno::set_raw(0);
    assert_eq!(call(x::vhangup()), raw(153, [0, 0, 0]), "vhangup");
    errno::set_raw(0);
    assert_eq!(call(x::delete_module(c"liella_none".as_ptr(), 0)), raw(176, [c"liella_none".as_ptr() as i64, 0, 0]), "delete_module");
    let mut b = [0u8; 64];
    errno::set_raw(0);
    assert_eq!(call(x::modify_ldt(0, b.as_mut_ptr().cast(), 64)), raw(154, [0, b.as_mut_ptr() as i64, 64]), "modify_ldt");
}
#[test]
fn s24_6_c_5() {
    assert_eq!(x::liella_syscall7(39, 0, 0, 0, 0, 0, 0), i64::from(CSys.getpid()), "getpid by number");
    errno::set_raw(0);
    assert_eq!(x::liella_syscall7(100_000, 0, 0, 0, 0, 0, 0), -1);
    assert_eq!(errno::get(), Errno::ENOSYS.get(), "an unknown number: -1 with errno");
    // syscall: any call by number with six register-sized arguments, -1 with errno on error;
    // including calls nothing else wraps (pidfd_open, memfd_create here). Real C varargs (six
    // arguments: mmap of a memfd at an offset) in conformance/c/s24_variadic.c.
    assert_eq!(x::liella_syscall7(39, 0, 0, 0, 0, 0, 0), i64::from(CSys.getpid()), "getpid");
    assert_eq!(x::liella_syscall7(186, 0, 0, 0, 0, 0, 0), i64::from(CSys.gettid()), "gettid");
    let fd = x::liella_syscall7(434, i64::from(CSys.getpid()), 0, 0, 0, 0, 0);
    assert!(fd >= 0, "pidfd_open of this process");
    assert_eq!(x::liella_syscall7(3, fd, 0, 0, 0, 0, 0), 0, "close");
    let name = c"liella-c65";
    let m = x::liella_syscall7(319, name.as_ptr() as i64, 1, 0, 0, 0, 0);
    assert!(m >= 0, "memfd_create");
    assert_eq!(x::liella_syscall7(3, m, 0, 0, 0, 0, 0), 0);
    errno::set_raw(0);
    assert_eq!((x::liella_syscall7(3, -1, 0, 0, 0, 0, 0), errno::get()), (-1, Errno::EBADF.get()));
    assert_eq!((x::liella_syscall7(100_000, 0, 0, 0, 0, 0, 0), errno::get()), (-1, Errno::ENOSYS.get()), "an unknown number");
    conformance(&["s24_variadic"]);
}
#[test]
fn s24_6_c_6() {
    // The compat entries (spec/behaviour/compat.md, glibc 2.39 probe): bdflush and sstk are stubs
    // failing with ENOSYS (sstk's result is (void *)-1); create_module, get_kernel_syms,
    // query_module and nfsservctl wrap system calls every current kernel answers with ENOSYS;
    // uselib's still works on a kernel with CONFIG_USELIB (then a missing file is ENOENT).
    use liella_boundary::posix::s29_legacy as l;
    let bad = |r: i64, e: Errno| r == -1 && errno::get() == e.get();
    let mut buf = [0u8; 16];
    let mut out = 0usize;
    assert!(bad(l::liella_compat_bdflush(0, 0).into(), Errno::ENOSYS));
    assert!(bad(l::liella_compat_bdflush(1, 0).into(), Errno::ENOSYS));
    for inc in [0, 4096] {
        assert!(l::liella_compat_sstk(inc).addr() == usize::MAX && errno::get() == Errno::ENOSYS.get(), "sstk");
    }
    assert!(x::liella_compat_create_module(c"liella".as_ptr(), 0).addr() == usize::MAX && errno::get() == Errno::ENOSYS.get(), "create_module");
    assert!(bad(x::liella_compat_get_kernel_syms(null_mut()).into(), Errno::ENOSYS));
    assert!(bad(x::liella_compat_query_module(c"liella".as_ptr(), 1, buf.as_mut_ptr().cast(), buf.len(), &raw mut out).into(), Errno::ENOSYS));
    assert!(bad(x::liella_compat_nfsservctl(0, null_mut(), null_mut()), Errno::ENOSYS));
    let r = x::liella_compat_uselib(c"/nonexistent-liella".as_ptr());
    assert!(r == -1 && [Errno::ENOSYS.get(), Errno::ENOENT.get()].contains(&errno::get()), "uselib");
}
#[test]
fn s24_6_c_6_syscall_numbers() {
    // s24_6_c_6: each compat wrapper makes exactly its own system call (the x86_64 numbers, written
    // here independently of csys/syscalls.tsv: uselib 134, create_module 174, get_kernel_syms 177,
    // query_module 178, nfsservctl 180), and the bdflush and sstk stubs make none. A forked child
    // runs them under a seccomp filter that fails the five numbers with the marker errnos 120..=124
    // (in that order; errno stops at 133) and every other call except exit and exit_group with
    // 125; it exits 10 + the index of
    // the first wrong errno, or 100 when it cannot install the filter.
    use liella_boundary::posix::s29_legacy as l;
    #[repr(C)]
    struct Insn {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct Prog {
        len: u16,
        filter: *const Insn,
    }
    const RET_ERRNO: u32 = 0x0005_0000;
    const RET_ALLOW: u32 = 0x7fff_0000;
    let ld_nr = Insn { code: 0x20, jt: 0, jf: 0, k: 0 };
    let jeq = |k| Insn { code: 0x15, jt: 0, jf: 1, k };
    let ret = |k| Insn { code: 0x06, jt: 0, jf: 0, k };
    let mut f = vec![ld_nr];
    for (n, mark) in [134, 174, 177, 178, 180].into_iter().zip(120..) {
        f.push(jeq(n));
        f.push(ret(RET_ERRNO | mark));
    }
    for n in [60, 231] {
        f.push(jeq(n));
        f.push(ret(RET_ALLOW));
    }
    f.push(ret(RET_ERRNO | 125));
    let prog = Prog { len: u16::try_from(f.len()).unwrap(), filter: f.as_ptr() };
    let mut buf = [0u8; 16];
    let mut out = 0usize;
    let pid = x::liella_syscall7(57, 0, 0, 0, 0, 0, 0); // fork
    assert!(pid >= 0, "fork");
    if pid == 0 {
        // The child: raw system calls and the wrappers only (no allocation, no panic).
        let code = (|| {
            if x::liella_syscall7(157, 38, 1, 0, 0, 0, 0) != 0 {
                return 100; // PR_SET_NO_NEW_PRIVS
            }
            if x::liella_syscall7(157, 22, 2, (&raw const prog) as i64, 0, 0, 0) != 0 {
                return 100; // PR_SET_SECCOMP, SECCOMP_MODE_FILTER
            }
            let got = |failed: bool| if failed { errno::get() } else { 0 };
            let seen = [
                got(x::liella_compat_uselib(c"/nonexistent-liella".as_ptr()) == -1),
                got(x::liella_compat_create_module(c"liella".as_ptr(), 0).addr() == usize::MAX),
                got(x::liella_compat_get_kernel_syms(null_mut()) == -1),
                got(x::liella_compat_query_module(c"liella".as_ptr(), 1, buf.as_mut_ptr().cast(), buf.len(), &raw mut out) == -1),
                got(x::liella_compat_nfsservctl(0, null_mut(), null_mut()) == -1),
                got(l::liella_compat_bdflush(0, 0) == -1),
                got(l::liella_compat_sstk(0).addr() == usize::MAX),
            ];
            let want = [120, 121, 122, 123, 124, Errno::ENOSYS.get(), Errno::ENOSYS.get()];
            seen.iter().zip(want).position(|(s, w)| *s != w).map_or(0, |i| 10 + i64::try_from(i).unwrap_or(0))
        })();
        x::liella_syscall7(231, code, 0, 0, 0, 0, 0); // exit_group
        std::process::abort();
    }
    let mut status = 0i32;
    assert_eq!(x::liella_syscall7(61, pid, (&raw mut status) as i64, 0, 0, 0, 0), pid, "wait4");
    if status == 100 << 8 {
        eprintln!("s24_6_c_6_syscall_numbers: seccomp filter unavailable here, not checked");
        return;
    }
    assert_eq!(status, 0, "child status {status:#x}: exit 10 + n = the n-th wrapper (uselib, create_module, get_kernel_syms, query_module, nfsservctl, bdflush, sstk) saw the wrong errno");
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s24_3_c_3() {
    // The policies apply to a thread: SCHED_BATCH set in one thread is not another's.
    const SCHED_OTHER: i32 = 0;
    const SCHED_BATCH: i32 = 3;
    assert_eq!(x::sched_getscheduler(0), SCHED_OTHER);
    let (set, seen) = std::thread::spawn(|| {
        let p = liella_kernel::uapi::SchedParam { priority: 0 };
        (x::sched_setscheduler(0, SCHED_BATCH, Some(&p)), x::sched_getscheduler(0))
    })
    .join()
    .unwrap();
    assert_eq!((set, seen), (0, SCHED_BATCH), "the calling thread");
    assert_eq!(x::sched_getscheduler(0), SCHED_OTHER, "not the other threads");
}

#[test]
fn s24_3_c_4() {
    // Affinity: cpu_set_t masks of 1024 CPUs.
    let mut mask = [0u8; 128];
    assert_eq!(x::sched_getaffinity(0, mask.len(), mask.as_mut_ptr().cast()), 0);
    assert!(mask.iter().any(|&b| b != 0), "at least one CPU");
}

#[test]
fn s24_3_c_5() {
    // sched_getcpu: the current CPU; getcpu also the NUMA node.
    let cpu = x::sched_getcpu();
    assert!(cpu >= 0 && i64::from(cpu) < system::nprocs_conf(&CSys) * 2);
    let (mut c, mut n) = (u32::MAX, u32::MAX);
    assert_eq!(x::getcpu(Some(&mut c), Some(&mut n)), 0);
    assert!(c != u32::MAX && n != u32::MAX);
}

clause!(s24_5_c_3, "24.5", "Contract", r##"**`getdomainname`** returns the NIS domain name, not the DNS domain. ⚑ HIST: a common confusion."##);

#[test]
fn s24_5_c_4() {
    // gethostid: from the identifier file if present (its 4 bytes), else from the host's
    // address; sethostid writes the file: EACCES without permission to write it (sethostid(3)).
    let d = scratch::Scratch::dir("c54");
    let f = d.join("hostid");
    std::fs::write(&f, 0x1234_5678i32.to_ne_bytes()).unwrap();
    let mut path = f.as_os_str().as_encoded_bytes().to_vec();
    path.push(0);
    assert_eq!(sys_compat::gethostid(&CSys, &path), 0x1234_5678);
    assert_eq!(x::gethostid(), x::gethostid(), "stable");
    if CSys.geteuid() != 0 {
        errno::set_raw(0);
        assert_eq!((x::sethostid(1), errno::get()), (-1, Errno::EACCES.get()));
    }
}

#[test]
fn s24_5_c_5() {
    // The identification strings: "2.39" and "stable", the version the ABI promises (D-24.11:
    // the spec, gnu_get_libc_version(3) and the reference agree); one library-owned string each,
    // the same number as confstr's "glibc 2.39".
    let text = |p: *const core::ffi::c_char| {
        let mut v = vec![0u8; st::strlen(p)];
        st::memcpy(v.as_mut_ptr().cast(), p.cast(), v.len());
        v
    };
    assert_eq!(text(x::gnu_get_libc_version()), b"2.39");
    assert_eq!(text(x::gnu_get_libc_release()), b"stable");
    assert_eq!(x::gnu_get_libc_version(), x::gnu_get_libc_version(), "the same string each call");
    let mut b = [0u8; 32];
    let n = x::confstr(2, b.as_mut_ptr().cast(), b.len());
    assert_eq!(&b[..n - 1], b"glibc 2.39", "_CS_GNU_LIBC_VERSION names the same version");
}

/// The child's exit status, stdout and stderr (the last 60 lines of each) for a failure message.
fn child_report(o: &std::process::Output) -> String {
    let tail = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(60)..].join("\n")
    };
    format!("exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}", o.status, tail(&o.stdout), tail(&o.stderr))
}
