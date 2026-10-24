// Specified in part from spec §24 (phase 5: §24.1–24.4 and `syscall`; phase 10: §24.5's
// `gethostid` and `sethostid`; the validation stage: the rest of §24.5–24.6). The compat entries
// of removed system calls are phase 15's.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §24.
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! `sysconf`, `confstr` and `pathconf` are compared name by name with the host's glibc (the
//! reference system's), through a C program compiled against it. `ulimit` and `syscall` are
//! variadic: their fixed-arity targets are tested here and the C entry points in
//! `conformance/c/s24_variadic.c`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/scratch.rs"]
mod scratch;

use core::ptr::null_mut;
use std::collections::BTreeMap;
use std::ffi::CString;

use liella_boundary::errno;
use liella_boundary::posix::s24_system as s;
use liella_kernel::uapi::{Rlimit, Rusage, SchedParam, Sysinfo, Timespec, Utsname};
use liella_legacy::sys_compat::{self, Vtimes};
use spectest::symbol;

const EPERM: i32 = 1;
const ENOENT: i32 = 2;
const ESRCH: i32 = 3;

/// Whether a conformance program dies by `SIGABRT` (fortify checks): `conformance/c/NAME.c`
/// with `EXPECT-SIGNAL: 6`.
fn conformance_dies(name: &str) -> bool {
    conformance(&[name]);
    true
}

/// Run `body` in this test re-executed in a private user and UTS namespace mapping the user to
/// root there (`unshare`); without `unshare` the test says so and passes.
/// Whether this is the re-executed child (then `body` has run there).
fn in_uts_namespace(test: &str, body: impl FnOnce()) -> bool {
    const VAR: &str = "LIELLA_S24_UTSNS";
    if std::env::var(VAR).as_deref() == Ok(test) {
        body();
        return true;
    }
    let ns = ["--user", "--uts", "--map-root-user"];
    if !std::process::Command::new("unshare").args(ns).arg("true").status().is_ok_and(|s| s.success()) {
        eprintln!("note: no private UTS namespace (unshare --user --uts --map-root-user); only the refusal was checked");
        return false;
    }
    let o = std::process::Command::new("unshare").args(ns).arg(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(VAR, test).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("1 passed"), "{test} in its namespace: {:?}\n{out}{}", o.status, String::from_utf8_lossy(&o.stderr));
    false
}

const EACCES: i32 = 13;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const EOVERFLOW: i32 = 75;
const ENOTBLK: i32 = 15;
const ENOSYS: i32 = 38;
const RLIMIT_NOFILE: i32 = 7;
const RLIMIT_CORE: i32 = 4;

fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn root() -> bool {
    liella_kernel::sys::Sys::geteuid(&liella_boundary::sys::CSys) == 0
}

/// The reference table's names of one kind (`sc`, `cs`, `pc`): name -> number.
fn names(kind: &str) -> BTreeMap<String, i32> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/abi/sysconf.tsv");
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0] == kind || (kind == "pc" && f[0].starts_with("pc:"))).then(|| (f[1].to_string(), f[2].parse().unwrap()))
        })
        .collect()
}

/// Runs C `body` against the host's glibc and returns its output lines.
fn host(tag: &str, body: &str) -> Vec<String> {
    let dir = scratch::Scratch::dir(&format!("s24-{tag}"));
    let c = dir.join("p.c");
    // The body's `#include` lines go before `main`.
    let (incl, code): (Vec<&str>, Vec<&str>) = body.split('\n').partition(|l| l.starts_with("#include"));
    let (incl, body) = (incl.join("\n"), code.join("\n"));
    std::fs::write(&c, format!("#define _GNU_SOURCE 1\n#include <unistd.h>\n#include <stdio.h>\n#include <errno.h>\n{incl}\nint main(void){{ char buf[4096]; long v; size_t n; (void)buf; (void)v; (void)n;\n{body}\nreturn 0; }}\n")).unwrap();
    let exe = dir.join("p");
    let st = std::process::Command::new("cc").arg("-w").arg(&c).arg("-o").arg(&exe).status().unwrap();
    assert!(st.success());
    let out = std::process::Command::new(&exe).output().unwrap();
    String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect()
}

fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// Names whose answer moves on its own between the two readings.
const MOVING: &[&str] = &["_SC_AVPHYS_PAGES"];

#[test]
fn sym___sched_yield() {
    assert_eq!(s::__sched_yield(), 0);
}

#[test]
fn sym_sched_yield() {
    assert_eq!(s::sched_yield(), 0);
}

#[test]
fn sym___sysconf() {
    let sc = names("sc");
    let body: String = sc.iter().map(|(k, v)| format!("errno = 0; v = sysconf({v}); printf(\"{k} %ld %d\\n\", v, errno);")).collect();
    let mut diffs = Vec::new();
    for line in host("__sysconf", &body) {
        let f: Vec<&str> = line.split(' ').collect();
        let (name, want, want_err) = (f[0], f[1].parse::<i64>().unwrap(), f[2].parse::<i32>().unwrap());
        errno::set_raw(0);
        let got = s::__sysconf(sc[name]);
        let got_err = errno::get();
        if MOVING.contains(&name) {
            assert!((got - want).abs() < want / 10, "{name}: {got} vs {want}");
            continue;
        }
        if (got, got_err) != (want, want_err) {
            diffs.push(format!("{name}: liella {got}/{got_err}, glibc {want}/{want_err}"));
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    errno::set_raw(0);
    assert_eq!((s::__sysconf(9999), errno::get()), (-1, EINVAL), "an unknown name");
}

/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_acct() {
    if root() {
        return;
    }
    let want = host("acct", "#include <sys/acct.h>\nerrno = 0; int r = acct(NULL); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::acct(core::ptr::null());
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    // The kernel refuses an unprivileged caller, whatever the path.
    errno::set_raw(0);
    assert!(fails(s::acct(c"/nonexistent/x".as_ptr()) as isize, EPERM));
    assert!(fails(s::acct(core::ptr::null()) as isize, EPERM));
}
symbol!(sym_bdflush, "bdflush", "24", "2.2.5", r##"(no installed header declares it)"##);
/// The calling thread's capabilities in the kernel's version 3 layout (`_LINUX_CAPABILITY_VERSION_3`),
/// as `/proc/self/status` shows them; an unknown version gets the kernel's preferred one and
/// `EINVAL`.
#[test]
fn sym_capget() {
    let mut hdr = [0x2008_0522u32, 0];
    let mut data = [0u32; 6];
    assert_eq!(s::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()), 0);
    let eff = u64::from(data[0]) | u64::from(data[3]) << 32;
    let status = std::fs::read_to_string("/proc/thread-self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("CapEff:")).unwrap();
    assert_eq!(eff, u64::from_str_radix(line.split_whitespace().nth(1).unwrap(), 16).unwrap());
    // An unknown version: the kernel writes its preferred one (capget(2); without a data
    // pointer the call is a version query and may succeed).
    let mut bad = [0x1234u32, 0];
    errno::set_raw(0);
    let r = s::capget(bad.as_mut_ptr().cast(), null_mut());
    assert!(r == 0 || fails(r as isize, EINVAL), "{r} {}", errno::get());
    assert_eq!(bad[0], 0x2008_0522, "the kernel's preferred version");
    // Version 0 is EINVAL with version 3 written back; NULL data only checks the version; an
    // unknown process is ESRCH.
    let mut hdr = [0u32, 0];
    errno::set_raw(0);
    assert!(fails(s::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()) as isize, EINVAL));
    assert_eq!(hdr[0], 0x2008_0522);
    assert_eq!(s::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()), 0);
    assert_eq!(s::capget(hdr.as_mut_ptr().cast(), null_mut()), 0, "NULL data: only the version check");
    hdr[1] = 0x7fff_fff0;
    assert!(fails(s::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()) as isize, ESRCH), "no such process");
}
/// Setting the thread's own current capabilities again changes nothing and succeeds.
#[test]
fn sym_capset() {
    let mut hdr = [0x2008_0522u32, 0];
    let mut data = [0u32; 6];
    assert_eq!(s::capget(hdr.as_mut_ptr().cast(), data.as_mut_ptr().cast()), 0);
    assert_eq!(s::capset(hdr.as_mut_ptr().cast(), data.as_ptr().cast()), 0);
    // An unknown version: EINVAL, the kernel's version written back.
    hdr[0] = 0x1234_5678;
    errno::set_raw(0);
    assert!(fails(s::capset(hdr.as_mut_ptr().cast(), data.as_ptr().cast()) as isize, EINVAL));
    assert_eq!(hdr[0], 0x2008_0522);
}
#[test]
fn sym_confstr() {
    let cs = names("cs");
    let body: String = cs.iter().map(|(k, v)| format!("errno = 0; n = confstr({v}, buf, sizeof buf); printf(\"{k}\\t%zu\\t%d\\t%s\\n\", n, errno, n ? buf : \"\");")).collect();
    for line in host("confstr", &body) {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let (name, want_n, want_err, text) = (f[0], f[1].parse::<usize>().unwrap(), f[2].parse::<i32>().unwrap(), f[3]);
        let mut buf = [0xffu8; 256];
        errno::set_raw(0);
        let got = s::confstr(cs[name], buf.as_mut_ptr().cast(), 256);
        assert_eq!((got, errno::get()), (want_n, want_err), "{name}");
        if got > 0 {
            assert_eq!(&buf[..got], [text.as_bytes(), &[0]].concat().as_slice(), "{name}");
        }
    }
    // Truncation: the full length is returned, the text cut and terminated.
    let mut small = [0xffu8; 4];
    assert_eq!(s::confstr(0, small.as_mut_ptr().cast(), 4), 14, "_CS_PATH is /bin:/usr/bin");
    assert_eq!(&small, b"/bi\0");
    assert_eq!(s::confstr(0, null_mut(), 0), 14, "NULL: only the length");
    errno::set_raw(0);
    assert_eq!((s::confstr(777, small.as_mut_ptr().cast(), 4), errno::get()), (0, EINVAL));
}

#[test]
fn sym___confstr_chk() {
    let cs = names("cs");
    let body: String = cs.iter().map(|(k, v)| format!("errno = 0; n = confstr({v}, buf, sizeof buf); printf(\"{k}\\t%zu\\t%d\\t%s\\n\", n, errno, n ? buf : \"\");")).collect();
    for line in host("__confstr_chk", &body) {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let (name, want_n, want_err, text) = (f[0], f[1].parse::<usize>().unwrap(), f[2].parse::<i32>().unwrap(), f[3]);
        let mut buf = [0xffu8; 256];
        errno::set_raw(0);
        let got = s::__confstr_chk(cs[name], buf.as_mut_ptr().cast(), 256, 256);
        assert_eq!((got, errno::get()), (want_n, want_err), "{name}");
        if got > 0 {
            assert_eq!(&buf[..got], [text.as_bytes(), &[0]].concat().as_slice(), "{name}");
        }
    }
    // Truncation: the full length is returned, the text cut and terminated.
    let mut small = [0xffu8; 4];
    assert_eq!(s::__confstr_chk(0, small.as_mut_ptr().cast(), 4, 4), 14, "_CS_PATH is /bin:/usr/bin");
    assert_eq!(&small, b"/bi\0");
    assert_eq!(s::__confstr_chk(0, null_mut(), 0, 0), 14, "NULL: only the length");
    errno::set_raw(0);
    assert_eq!((s::__confstr_chk(777, small.as_mut_ptr().cast(), 4, 4), errno::get()), (0, EINVAL));
}

symbol!(sym_create_module, "create_module", "24", "2.2.5", r##"(no installed header declares it)"##);
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_delete_module() {
    if root() {
        return;
    }
    let want = host("delete_module", "#include <sys/syscall.h>\nerrno = 0; int r = (int)syscall(SYS_delete_module, \"liella_no_such_module\", 0); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::delete_module(c"liella_no_such_module".as_ptr(), 0);
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    errno::set_raw(0);
    assert!(fails(s::delete_module(c"liella_nonesuch".as_ptr(), 0) as isize, EPERM));
}
#[test]
fn sym_fpathconf() {
    let pc = names("pc");
    let mut diffs = Vec::new();
    let mut p = [-1; 2];
    liella_boundary::posix::s25_ipc::pipe(Some(&mut p));
    for (label, target) in [("/", 0), ("pipe", 1)] {
        let fd = if target == 0 { liella_boundary::posix::s11_lowio::liella_open3(c"/".as_ptr(), 0o200000, 0) } else { p[0] };
        let open = if target == 0 { "int fd = open(\"/\", 0200000);".to_string() } else { "int fd[2]; pipe(fd);".to_string() };
        let use_ = if target == 0 { "fd" } else { "fd[0]" };
        let body: String = open + &pc.iter().map(|(k, v)| format!("errno = 0; v = fpathconf({use_}, {v}); printf(\"{k} %ld %d\\n\", v, errno);")).collect::<String>();
        for line in host("fpathconf", &format!("#include <fcntl.h>\n{body}")) {
            let f: Vec<&str> = line.split(' ').collect();
            let (name, want, want_err) = (f[0], f[1].parse::<i64>().unwrap(), f[2].parse::<i32>().unwrap());
            errno::set_raw(0);
            let got = s::fpathconf(fd, pc[name]);
            if (got, errno::get()) != (want, want_err) {
                diffs.push(format!("{label} {name}: liella {got}/{}, glibc {want}/{want_err}", errno::get()));
            }
        }
        liella_boundary::posix::s11_lowio::close(fd);
    }
    liella_boundary::posix::s11_lowio::close(p[1]);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    errno::set_raw(0);
    assert!(fails(s::fpathconf(999, 0) as isize, 9));
}

#[test]
fn sym_get_avphys_pages() {
    let want: i64 = host("get_avphys_pages", "#include <sys/sysinfo.h>\nprintf(\"%ld\\n\", get_avphys_pages());")[0].parse().unwrap();
    let got = s::get_avphys_pages();
    assert!((got - want).abs() < want / 10, "free memory moves: {got} vs {want}");
}

symbol!(sym_get_kernel_syms, "get_kernel_syms", "24", "2.2.5", r##"(no installed header declares it)"##);
#[test]
fn sym_get_nprocs() {
    let want = host("get_nprocs", "#include <sys/sysinfo.h>\nprintf(\"%d\\n\", get_nprocs());");
    assert_eq!(s::get_nprocs().to_string(), want[0]);
}

#[test]
fn sym_get_nprocs_conf() {
    let want = host("get_nprocs_conf", "#include <sys/sysinfo.h>\nprintf(\"%d\\n\", get_nprocs_conf());");
    assert_eq!(s::get_nprocs_conf().to_string(), want[0]);
}

#[test]
fn sym_get_phys_pages() {
    let want: i64 = host("get_phys_pages", "#include <sys/sysinfo.h>\nprintf(\"%ld\\n\", get_phys_pages());")[0].parse().unwrap();
    assert_eq!(s::get_phys_pages(), want);
}

#[test]
fn sym_getcpu() {
    let (mut cpu, mut node) = (u32::MAX, u32::MAX);
    assert_eq!(s::getcpu(Some(&mut cpu), Some(&mut node)), 0);
    assert!((cpu as i64) < s::sysconf(83) && node < 1024);
    assert_eq!(s::getcpu(None, None), 0, "either may be NULL");
}

#[test]
fn sym_getdomainname() {
    let len = uts_len(&uts().domainname);
    name_sizes("getdomainname", "getdomainname", len, |b| s::getdomainname(b.as_mut_ptr().cast(), b.len()));
}
#[test]
fn sym___getdomainname_chk() {
    let len = uts_len(&uts().domainname);
    let mut b = [0u8; 80];
    assert_eq!(s::__getdomainname_chk(b.as_mut_ptr().cast(), len + 1, b.len()), 0);
    assert_eq!(&b[..len], &uts().domainname[..len]);
}
#[test]
fn sym_getdtablesize() {
    let mut l = Rlimit { cur: 0, max: 0 };
    s::getrlimit(RLIMIT_NOFILE, Some(&mut l));
    assert_eq!(s::getdtablesize() as u64, l.cur.min(i32::MAX as u64));
}

/// The host identifier computed independently (gethostid(3)): the first 4 bytes of
/// /etc/hostid, else the node name's IPv4 address in /etc/hosts (the first line naming it) read
/// as a native `u32` with its 16-bit halves exchanged, else 0.
fn expected_hostid() -> i32 {
    if let Ok(b) = std::fs::read("/etc/hostid") {
        if b.len() >= 4 {
            return i32::from_ne_bytes(b[..4].try_into().unwrap());
        }
    }
    expected_hostid_without_file()
}

#[test]
fn sym_gethostid() {
    assert_eq!(s::gethostid(), i64::from(expected_hostid()), "sign-extended to long");
    // The file's forms, through the same function with a fixture file.
    let k = &liella_boundary::sys::CSys;
    let dir = scratch::Scratch::dir("s24-hostid");
    let file = dir.join("hostid");
    let path = [file.to_str().unwrap().as_bytes(), b"\0"].concat();
    let fallback = sys_compat::gethostid(k, b"/nonexistent/hostid\0");
    assert_eq!(fallback, expected_hostid_without_file(), "no file: from the address");
    for (bytes, want) in [(&[0x78u8, 0x56, 0x34, 0x12][..], 0x1234_5678), (&[1, 2, 3, 4, 5, 6, 7, 8][..], i32::from_ne_bytes([1, 2, 3, 4])), (&[0xff; 4][..], -1)] {
        std::fs::write(&file, bytes).unwrap();
        assert_eq!(sys_compat::gethostid(k, &path), want, "{bytes:?}: the first 4 bytes, native order");
    }
    std::fs::write(&file, [1u8, 2]).unwrap();
    assert_eq!(sys_compat::gethostid(k, &path), fallback, "a short file counts as absent");
}

/// [`expected_hostid`] as if /etc/hostid did not exist.
fn expected_hostid_without_file() -> i32 {
    let node = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_string();
    let hosts = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    for line in hosts.lines() {
        let line = line.split('#').next().unwrap();
        let mut f = line.split_whitespace();
        let Some(Ok(a)) = f.next().map(str::parse::<std::net::Ipv4Addr>) else { continue };
        if f.any(|n| n.eq_ignore_ascii_case(&node)) {
            return u32::from_ne_bytes(a.octets()).rotate_left(16) as i32;
        }
    }
    0
}
/// The node name of length `len` into a buffer of every size from 0 to `len + 2`: the bytes
/// that fit, the result and `errno`, as `name` (a C call through `f`) and the host's glibc give.
fn name_sizes(tag: &str, cfun: &str, len: usize, f: impl Fn(&mut [u8]) -> i32) {
    let body = format!("for (n = 0; n <= {}; n++) {{ char b[80]; memset(b, 'x', sizeof b); errno = 0; int r = {cfun}(b, n); printf(\"%zu %d %d \", n, r, r ? errno : 0); for (size_t i = 0; i <= n; i++) putchar(b[i] ? b[i] : '0'); printf(\"\\n\"); }}", len + 2);
    let want = host(tag, &format!("#include <string.h>\n{body}"));
    let mut got = Vec::new();
    for n in 0..=len + 2 {
        let mut b = [b'x'; 80];
        errno::set_raw(0);
        let r = f(&mut b[..n]);
        let shown: String = b[..=n].iter().map(|&c| if c == 0 { '0' } else { c as char }).collect();
        got.push(format!("{n} {r} {} {shown}", if r != 0 { errno::get() } else { 0 }));
    }
    assert_eq!(got, want, "{cfun}");
}

/// The C text at `p`, through the exported strlen/memcpy (no unsafe in tests).
fn text(p: *const core::ffi::c_char) -> String {
    assert!(!p.is_null());
    let n = liella_boundary::posix::s04_strings::strlen(p);
    let mut v = vec![0u8; n];
    liella_boundary::posix::s04_strings::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

fn uts() -> Utsname {
    let mut u = Utsname::default();
    assert_eq!(s::uname(Some(&mut u)), 0);
    u
}

fn uts_len(f: &[u8; 65]) -> usize {
    f.iter().position(|&b| b == 0).unwrap()
}

#[test]
fn sym_gethostname() {
    let len = uts_len(&uts().nodename);
    name_sizes("gethostname", "gethostname", len, |b| s::gethostname(b.as_mut_ptr().cast(), b.len()));
    errno::set_raw(0);
    assert!(fails(s::gethostname(null_mut(), 10) as isize, EFAULT), "NULL with a size (glibc crashes)");
}
#[test]
fn sym___gethostname_chk() {
    let len = uts_len(&uts().nodename);
    let mut b = [0u8; 80];
    assert_eq!(s::__gethostname_chk(b.as_mut_ptr().cast(), len + 1, b.len()), 0);
    assert_eq!(&b[..len], &uts().nodename[..len]);
    assert!(conformance_dies("s24_hostname_chk"), "a size beyond the object is __chk_fail");
}
/// How many averages each count gets (the host's glibc: 0 for a count of 0 or less, at most
/// 3), and the values `/proc/loadavg` shows.
#[test]
fn sym_getloadavg() {
    let want = host("loadavg", "#include <stdlib.h>\ndouble a[5]; for (int n = -1; n <= 5; n++) printf(\"%d\\n\", getloadavg(a, n));");
    let got: Vec<String> = (-1..=5).map(|n| { let mut a = [-1f64; 5]; s::getloadavg(a.as_mut_ptr(), n).to_string() }).collect();
    assert_eq!(got, want);
    let mut a = [-1f64; 4];
    assert_eq!(s::getloadavg(a.as_mut_ptr(), 4), 3);
    assert_eq!(a[3], -1.0, "only three are written");
    let text = std::fs::read_to_string("/proc/loadavg").unwrap();
    let proc: Vec<f64> = text.split(' ').take(3).map(|v| v.parse().unwrap()).collect();
    for (k, (x, y)) in a.iter().zip(&proc).enumerate() {
        // The averages may move between the two readings.
        assert!((x - y).abs() < 1.0 && *x >= 0.0, "average {k}: {x} vs {y}");
    }
}
#[test]
fn sym_getpagesize() {
    assert_eq!(s::getpagesize(), 4096);
}

#[test]
fn sym_getpriority() {
    let line = std::fs::read_to_string("/proc/self/stat").unwrap();
    let nice: i32 = line.rsplit(") ").next().unwrap().split(' ').nth(16).unwrap().parse().unwrap();
    errno::set_raw(0);
    assert_eq!(s::getpriority(0, 0), nice, "the nice value itself (the kernel's bias undone)");
    assert_eq!(errno::get(), 0);
    assert!(fails(s::getpriority(9, 0) as isize, EINVAL));
}

#[test]
fn sym_getrlimit() {
    let mut l = Rlimit { cur: 0, max: 0 };
    assert_eq!(s::getrlimit(RLIMIT_NOFILE, Some(&mut l)), 0);
    let line = std::fs::read_to_string("/proc/self/limits").unwrap();
    let row = line.lines().find(|r| r.starts_with("Max open files")).unwrap();
    let nums: Vec<u64> = row.split_whitespace().filter_map(|w| w.parse().ok()).collect();
    assert_eq!((l.cur, l.max), (nums[0], nums[1]), "the kernel's record");
    errno::set_raw(0);
    assert!(fails(s::getrlimit(99, Some(&mut l)) as isize, EINVAL));
    assert!(fails(s::getrlimit(RLIMIT_NOFILE, None) as isize, EFAULT));
}

#[test]
fn sym_getrlimit64() {
    let mut l = Rlimit { cur: 0, max: 0 };
    assert_eq!(s::getrlimit64(RLIMIT_NOFILE, Some(&mut l)), 0);
    let line = std::fs::read_to_string("/proc/self/limits").unwrap();
    let row = line.lines().find(|r| r.starts_with("Max open files")).unwrap();
    let nums: Vec<u64> = row.split_whitespace().filter_map(|w| w.parse().ok()).collect();
    assert_eq!((l.cur, l.max), (nums[0], nums[1]), "the kernel's record");
    errno::set_raw(0);
    assert!(fails(s::getrlimit64(99, Some(&mut l)) as isize, EINVAL));
    assert!(fails(s::getrlimit64(RLIMIT_NOFILE, None) as isize, EFAULT));
}

#[test]
fn sym_getrusage() {
    let mut r = Rusage::default();
    // Spin on this thread's CPU time, not the wall clock (a busy machine may not run the thread
    // for the whole of a wall-clock interval), with a wall-clock bound.
    let spin = std::time::Instant::now();
    let mut t = Rusage::default();
    while spin.elapsed() < std::time::Duration::from_secs(10) {
        assert_eq!(s::getrusage(1, Some(&mut t)), 0);
        if t.utime.sec * 1_000_000 + t.utime.usec >= 20_000 {
            break;
        }
        for _ in 0..10_000 {
            std::hint::black_box(0);
        }
    }
    assert_eq!(s::getrusage(0, Some(&mut r)), 0);
    assert!(r.utime.sec * 1_000_000 + r.utime.usec >= 15_000, "user time");
    assert!(r.maxrss > 0 && r.minflt > 0);
    assert_eq!(s::getrusage(1, Some(&mut r)), 0, "RUSAGE_THREAD");
    errno::set_raw(0);
    assert!(fails(s::getrusage(7, Some(&mut r)) as isize, EINVAL));
}

#[test]
fn sym_gnu_get_libc_release() {
    let v = text(s::gnu_get_libc_release());
    assert_eq!(v, "stable");
    let want = host("release", "#include <gnu/libc-version.h>\nprintf(\"%s\\n\", gnu_get_libc_release());");
    assert_eq!(vec![v], want);
}
#[test]
fn sym_gnu_get_libc_version() {
    let v = text(s::gnu_get_libc_version());
    assert_eq!(v, "2.39");
    assert_eq!(s::gnu_get_libc_version(), s::gnu_get_libc_version(), "one string");
    let want = host("version", "#include <gnu/libc-version.h>\nprintf(\"%s\\n\", gnu_get_libc_version());");
    assert_eq!(vec![v], want);
}
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_init_module() {
    if root() {
        return;
    }
    let want = host("init_module", "#include <sys/syscall.h>\nerrno = 0; int r = (int)syscall(SYS_init_module, buf, 16, \"\"); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::init_module([0u8; 16].as_mut_ptr().cast(), 16, c"".as_ptr());
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    errno::set_raw(0);
    assert!(fails(s::init_module([0u8; 16].as_mut_ptr().cast(), 16, c"".as_ptr()) as isize, EPERM));
}
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_ioperm() {
    errno::set_raw(0);
    assert!(fails(s::ioperm(0x10000, 1, 1) as isize, EINVAL), "beyond the I/O bitmap");
    assert_eq!(s::ioperm(0x80, 1, 0), 0);
    if root() {
        return;
    }
    let want = host("ioperm", "#include <sys/io.h>\nerrno = 0; int r = ioperm(0x80, 1, 1); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::ioperm(0x80, 1, 1);
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
}
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_iopl() {
    errno::set_raw(0);
    assert!(fails(s::iopl(4) as isize, EINVAL));
    assert!(fails(s::iopl(-1) as isize, EINVAL));
    assert_eq!(s::iopl(0), 0);
    if root() {
        return;
    }
    let want = host("iopl", "#include <sys/io.h>\nerrno = 0; int r = iopl(4); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::iopl(4);
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    errno::set_raw(0);
    assert!(fails(s::iopl(3) as isize, EPERM));
}
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_klogctl() {
    if root() {
        return;
    }
    let want = host("klogctl", "#include <sys/klog.h>\nerrno = 0; int r = klogctl(10, NULL, 0); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::klogctl(10, null_mut(), 0);
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    errno::set_raw(0);
    assert!(fails(s::klogctl(99, null_mut(), 0) as isize, if want[0].starts_with("-1 1") { EPERM } else { EINVAL }));
    // Under dmesg_restrict an unprivileged read is EPERM.
    if std::fs::read_to_string("/proc/sys/kernel/dmesg_restrict").is_ok_and(|v| v.trim() == "1") {
        let mut b = [0u8; 64];
        errno::set_raw(0);
        assert!(fails(s::klogctl(3, b.as_mut_ptr().cast(), 64) as isize, EPERM));
    }
}
/// Reading the thread's local descriptor table: none is set up, so no bytes (as the host's
/// glibc reports through `syscall`, the only way it offers the call).
#[test]
fn sym_modify_ldt() {
    let want = host("modify_ldt", "#include <sys/syscall.h>\nprintf(\"%ld\\n\", syscall(SYS_modify_ldt, 0, buf, sizeof buf));");
    let mut b = [0u8; 4096];
    assert_eq!(vec![s::modify_ldt(0, b.as_mut_ptr().cast(), 4096).to_string()], want);
    // An unknown function: the kernel answers -ENOSYS in a 32-bit int it zero-extends
    // (`syscall` sees 4294967258); the wrapper reports -1 with `errno` (modify_ldt(2)), where
    // the reference returns -38 and leaves `errno` alone (docs/differences-from-glibc.md).
    errno::set_raw(0);
    let raw = s::liella_syscall7(154, 99, b.as_mut_ptr() as i64, 16, 0, 0, 0);
    assert_eq!(raw, 0xffff_ffda, "the kernel's zero-extended -ENOSYS");
    errno::set_raw(0);
    assert!(fails(s::modify_ldt(99, b.as_mut_ptr().cast(), 16) as isize, 38), "ENOSYS");
    // -1 with errno, never the negated error number; a bad size is EINVAL; a written entry
    // reads back (the test process uses no LDT segment; only this test writes).
    let mut ldt = [0u8; 64];
    errno::set_raw(0);
    assert!(fails(s::modify_ldt(7, ldt.as_mut_ptr().cast(), 64) as isize, ENOSYS));
    assert!(fails(s::modify_ldt(1, ldt.as_mut_ptr().cast(), 4) as isize, EINVAL));
    let desc: [u32; 4] = [0, 0, 0xfff, 1 | (1 << 4) | (1 << 6)];
    assert_eq!(s::modify_ldt(1, desc.as_ptr().cast_mut().cast(), 16), 0);
    assert_eq!(s::modify_ldt(0, ldt.as_mut_ptr().cast(), 64), 64);
    assert_eq!(ldt[..8], [0xff, 0x0f, 0, 0, 0, 0xf3, 0xc0, 0], "entry 0: limit 0xfff, 32-bit, in pages, present, user");
}
symbol!(sym_nfsservctl, "nfsservctl", "24", "2.2.5", r##"(no installed header declares it)"##);
#[test]
fn sym_nice() {
    if std::env::var("LIELLA_S24_NICE").is_ok() {
        let a = s::nice(0);
        let b = s::nice(3);
        errno::set_raw(0);
        let c = if root() { -1 } else { s::nice(-1) };
        let refused = root() || errno::get() == EPERM;
        std::process::exit(if b == a + 3 && c == -1 && refused { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_nice", "--exact", "--test-threads=1"])
        .env("LIELLA_S24_NICE", "1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "the new value; lowering needs privilege (EPERM, not EACCES)");
}

#[test]
fn sym_pathconf() {
    let pc = names("pc");
    let mut diffs = Vec::new();
    for path in ["/", "/proc", "/dev/null", "/tmp", "/etc/passwd"] {
        let body: String = pc.iter().map(|(k, v)| format!("errno = 0; v = pathconf(\"{path}\", {v}); printf(\"{k} %ld %d\\n\", v, errno);")).collect();
        let cpath = CString::new(path).unwrap();
        for line in host("pathconf", &body) {
            let f: Vec<&str> = line.split(' ').collect();
            let (name, want, want_err) = (f[0], f[1].parse::<i64>().unwrap(), f[2].parse::<i32>().unwrap());
            errno::set_raw(0);
            let got = s::pathconf(cpath.as_ptr(), pc[name]);
            if (got, errno::get()) != (want, want_err) {
                diffs.push(format!("{path} {name}: liella {got}/{}, glibc {want}/{want_err}", errno::get()));
            }
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    errno::set_raw(0);
    assert!(fails(s::pathconf(c"/no/such".as_ptr(), 0) as isize, 2));
    assert!(fails(s::pathconf(c"/".as_ptr(), 999) as isize, EINVAL));
}

#[test]
fn sym_prlimit64() {
    let mut old = Rlimit { cur: 0, max: 0 };
    assert_eq!(s::prlimit64(0, RLIMIT_NOFILE, None, Some(&mut old)), 0);
    let mut own = Rlimit { cur: 0, max: 0 };
    assert_eq!(s::prlimit64(std::process::id() as i32, RLIMIT_NOFILE, None, Some(&mut own)), 0, "by PID");
    assert_eq!(old, own);
    errno::set_raw(0);
    assert!(fails(s::prlimit64(i32::MAX - 1, RLIMIT_NOFILE, None, Some(&mut old)) as isize, ESRCH));
}

#[test]
fn sym_prlimit() {
    let mut old = Rlimit { cur: 0, max: 0 };
    assert_eq!(s::prlimit(0, RLIMIT_NOFILE, None, Some(&mut old)), 0);
    let mut own = Rlimit { cur: 0, max: 0 };
    assert_eq!(s::prlimit(std::process::id() as i32, RLIMIT_NOFILE, None, Some(&mut own)), 0, "by PID");
    assert_eq!(old, own);
    errno::set_raw(0);
    assert!(fails(s::prlimit(i32::MAX - 1, RLIMIT_NOFILE, None, Some(&mut old)) as isize, ESRCH));
}

symbol!(sym_query_module, "query_module", "24", "2.2.5", r##"(no installed header declares it)"##);
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_quotactl() {
    // The device path passes through: "/" is not a block device; a missing one ENOENT.
    let mut b = [0u8; 64];
    errno::set_raw(0);
    assert!(fails(s::quotactl(0x8000_0500_u32 as i32, c"/".as_ptr(), 0, b.as_mut_ptr().cast()) as isize, ENOTBLK));
    assert!(fails(s::quotactl(0x8000_0700_u32 as i32, c"/nonexistent".as_ptr(), 0, b.as_mut_ptr().cast()) as isize, ENOENT));
    if root() {
        return;
    }
    let want = host("quotactl", "#include <sys/quota.h>\nerrno = 0; int r = quotactl(QCMD(Q_SYNC, USRQUOTA), NULL, 0, NULL); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::quotactl(0x80_0001 << 8, core::ptr::null(), 0, null_mut());
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
}
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_reboot() {
    if root() {
        return;
    }
    let want = host("reboot", "#include <sys/reboot.h>\nerrno = 0; int r = reboot(0x12345); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::reboot(0x12345);
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    // Unprivileged the kernel refuses every command (the namespace cases are admin_products.rs).
    errno::set_raw(0);
    assert!(fails(s::reboot(0) as isize, EPERM));
}
#[test]
fn sym___sched_cpualloc() {
    let p = s::__sched_cpualloc(1025);
    assert!(!p.is_null());
    assert!(liella_boundary::posix::s03_memory::malloc_usable_size(p) >= 136, "CPU_ALLOC_SIZE(1025) = 17 words");
    s::__sched_cpufree(p);
}

#[test]
fn sym___sched_cpucount() {
    let set = [0b1011u8, 0, 0x80, 0];
    assert_eq!(s::__sched_cpucount(4, set.as_ptr().cast()), 4);
    assert_eq!(s::__sched_cpucount(0, set.as_ptr().cast()), 0);
}

#[test]
fn sym___sched_cpufree() {
    let p = s::__sched_cpualloc(64);
    assert!(!p.is_null());
    s::__sched_cpufree(p);
    s::__sched_cpufree(null_mut());
}

#[test]
fn sym___sched_get_priority_max() {
    assert_eq!(s::__sched_get_priority_max(1), 99, "SCHED_FIFO");
    assert_eq!(s::__sched_get_priority_max(0), 0, "SCHED_OTHER");
    errno::set_raw(0);
    assert!(fails(s::__sched_get_priority_max(77) as isize, EINVAL));
}

#[test]
fn sym_sched_get_priority_max() {
    assert_eq!(s::sched_get_priority_max(1), 99, "SCHED_FIFO");
    assert_eq!(s::sched_get_priority_max(0), 0, "SCHED_OTHER");
    errno::set_raw(0);
    assert!(fails(s::sched_get_priority_max(77) as isize, EINVAL));
}

#[test]
fn sym___sched_get_priority_min() {
    assert_eq!(s::__sched_get_priority_min(1), 1, "SCHED_FIFO");
    assert_eq!(s::__sched_get_priority_min(0), 0, "SCHED_OTHER");
    errno::set_raw(0);
    assert!(fails(s::__sched_get_priority_min(77) as isize, EINVAL));
}

#[test]
fn sym_sched_get_priority_min() {
    assert_eq!(s::sched_get_priority_min(1), 1, "SCHED_FIFO");
    assert_eq!(s::sched_get_priority_min(0), 0, "SCHED_OTHER");
    errno::set_raw(0);
    assert!(fails(s::sched_get_priority_min(77) as isize, EINVAL));
}

#[test]
fn sym_sched_getaffinity() {
    let mut mask = [0xffu8; 256];
    assert_eq!(s::sched_getaffinity(0, 256, mask.as_mut_ptr().cast()), 0);
    let ones: u32 = mask.iter().map(|b| b.count_ones()).sum();
    assert!(ones >= 1);
    assert!(mask[128..].iter().all(|&b| b == 0), "beyond the kernel's mask: zeroed");
    errno::set_raw(0);
    assert!(fails(s::sched_getaffinity(0, 1, mask.as_mut_ptr().cast()) as isize, EINVAL) || ones <= 8, "too small for the kernel's mask");
}

#[test]
fn sym_sched_getcpu() {
    let c = s::sched_getcpu();
    assert!(c >= 0 && (c as i64) < s::sysconf(83));
}

#[test]
fn sym___sched_getparam() {
    let mut p = SchedParam { priority: 9 };
    assert_eq!(s::__sched_getparam(0, Some(&mut p)), 0);
    assert_eq!(p.priority, 0);
    errno::set_raw(0);
    assert!(fails(s::__sched_getparam(i32::MAX - 1, Some(&mut p)) as isize, ESRCH));
}

#[test]
fn sym_sched_getparam() {
    let mut p = SchedParam { priority: 9 };
    assert_eq!(s::sched_getparam(0, Some(&mut p)), 0);
    assert_eq!(p.priority, 0);
    errno::set_raw(0);
    assert!(fails(s::sched_getparam(i32::MAX - 1, Some(&mut p)) as isize, ESRCH));
}

#[test]
fn sym___sched_getscheduler() {
    assert_eq!(s::__sched_getscheduler(0), 0, "SCHED_OTHER");
    errno::set_raw(0);
    assert!(fails(s::__sched_getscheduler(-1) as isize, EINVAL));
}

#[test]
fn sym_sched_getscheduler() {
    assert_eq!(s::sched_getscheduler(0), 0, "SCHED_OTHER");
    errno::set_raw(0);
    assert!(fails(s::sched_getscheduler(-1) as isize, EINVAL));
}

#[test]
fn sym_sched_rr_get_interval() {
    let mut ts = Timespec { sec: 9, nsec: 9 };
    assert_eq!(s::sched_rr_get_interval(0, Some(&mut ts)), 0);
    assert!(ts.sec < 9);
}

#[test]
fn sym_sched_setaffinity() {
    let mut mask = [0u8; 128];
    s::sched_getaffinity(0, 128, mask.as_mut_ptr().cast());
    let first = mask.iter().position(|&b| b != 0).unwrap();
    let mut one = [0u8; 128];
    one[first] = mask[first] & mask[first].wrapping_neg();
    let t = std::thread::spawn(move || {
        let r = s::sched_setaffinity(0, 128, one.as_ptr().cast());
        let mut now = [0u8; 128];
        s::sched_getaffinity(0, 128, now.as_mut_ptr().cast());
        (r, now == one)
    });
    assert_eq!(t.join().unwrap(), (0, true), "a thread pinned to one CPU");
    errno::set_raw(0);
    assert!(fails(s::sched_setaffinity(0, 128, [0u8; 128].as_ptr().cast()) as isize, EINVAL), "no CPU at all");
}

#[test]
fn sym_sched_setparam() {
    assert_eq!(s::sched_setparam(0, Some(&SchedParam { priority: 0 })), 0);
    errno::set_raw(0);
    assert!(fails(s::sched_setparam(0, Some(&SchedParam { priority: 5 })) as isize, EINVAL), "SCHED_OTHER takes only 0");
}

#[test]
fn sym___sched_setscheduler() {
    let p = SchedParam { priority: 0 };
    assert_eq!(s::__sched_setscheduler(0, 0, Some(&p)), 0, "SCHED_OTHER again");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::__sched_setscheduler(0, 1, Some(&SchedParam { priority: 10 })) as isize, EPERM), "SCHED_FIFO needs privilege");
    }
    assert!(fails(s::__sched_setscheduler(0, 0, None) as isize, EFAULT));
}

#[test]
fn sym_sched_setscheduler() {
    let p = SchedParam { priority: 0 };
    assert_eq!(s::sched_setscheduler(0, 0, Some(&p)), 0, "SCHED_OTHER again");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::sched_setscheduler(0, 1, Some(&SchedParam { priority: 10 })) as isize, EPERM), "SCHED_FIFO needs privilege");
    }
    assert!(fails(s::sched_setscheduler(0, 0, None) as isize, EFAULT));
}

#[test]
fn sym_setdomainname() {
    let child = in_uts_namespace("sym_setdomainname", || {
        assert_eq!(s::setdomainname(c"liella.test".as_ptr(), 11), 0);
        let mut b = [0u8; 80];
        assert_eq!(s::getdomainname(b.as_mut_ptr().cast(), b.len()), 0);
        assert_eq!(&b[..12], b"liella.test\0");
    });
    if child || root() {
        return;
    }
    let want = host("setdomainname", "errno = 0; int r = setdomainname(\"x\", 1); printf(\"%d %d\\n\", r, errno);");
    errno::set_raw(0);
    let r = s::setdomainname(c"x".as_ptr(), 1);
    assert_eq!(vec![format!("{r} {}", errno::get())], want);
}
#[test]
fn sym_sethostid() {
    // Never against the system's file as the superuser.
    errno::set_raw(0);
    for big in [1i64 << 31, -(1i64 << 31) - 1, 1 << 40] {
        assert!(fails(s::sethostid(big) as isize, EOVERFLOW), "{big}: outside int32_t, checked first");
    }
    if !root() {
        assert!(fails(s::sethostid(5) as isize, EACCES), "a user may not write /etc/hostid");
    }
    // The file's form, on a fixture file.
    let k = &liella_boundary::sys::CSys;
    let dir = scratch::Scratch::dir("s24-sethostid");
    let file = dir.join("hostid");
    let path = [file.to_str().unwrap().as_bytes(), b"\0"].concat();
    std::fs::write(&file, b"longer old contents").unwrap();
    sys_compat::sethostid(k, &path, -2).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), (-2i32).to_ne_bytes(), "4 bytes, native order, replacing the old contents");
    assert_eq!(sys_compat::gethostid(k, &path), -2, "read back");
}
/// Without privilege the kernel refuses (`EPERM`, as the host's glibc reports); in a private
/// user and UTS namespace the name changes there only (`unshare`, run as a child test).
#[test]
fn sym_sethostname() {
    let child = in_uts_namespace("sym_sethostname", || {
        assert_eq!(s::sethostname(c"liella-test-host".as_ptr(), 16), 0);
        let mut b = [0u8; 80];
        assert_eq!(s::gethostname(b.as_mut_ptr().cast(), b.len()), 0);
        assert_eq!(&b[..17], b"liella-test-host\0");
        errno::set_raw(0);
        let long = [b'x'; 65];
        assert!(fails(s::sethostname(long.as_ptr().cast(), 65) as isize, EINVAL), "longer than the kernel's 64");
    });
    if child || root() {
        return;
    }
    let want = host("sethostname", "errno = 0; int r = sethostname(\"x\", 1); printf(\"%d %d\\n\", r, errno);");
    errno::set_raw(0);
    let r = s::sethostname(c"x".as_ptr(), 1);
    assert_eq!(vec![format!("{r} {}", errno::get())], want);
}
#[test]
fn sym_setns() {
    // The export is in posix::s18_processes (its contract is §18.9): thin, -1 and errno.
    use liella_boundary::posix::s18_processes as p;
    errno::set_raw(0);
    assert_eq!((p::setns(-1, 0), errno::get()), (-1, 9), "EBADF");
    let f = CString::new("/proc/self/ns/uts").unwrap();
    let fd = liella_boundary::posix::s11_lowio::liella_open3(f.as_ptr(), 0o2000000, 0);
    assert!(fd >= 0);
    assert_eq!((p::setns(fd, 0x0400_0000), errno::get()), (-1, EPERM), "joining a UTS namespace needs CAP_SYS_ADMIN");
    liella_boundary::posix::s11_lowio::close(fd);
}
#[test]
fn sym_setpriority() {
    let me = std::process::id();
    let cur = s::getpriority(0, me);
    assert_eq!(s::setpriority(0, me, cur), 0);
    errno::set_raw(0);
    if !root() && cur > -20 {
        assert!(fails(s::setpriority(0, me, cur - 1) as isize, 13), "raising priority: EACCES from the kernel");
    }
}

#[test]
fn sym_setrlimit() {
    if std::env::var("LIELLA_S24_RL").is_ok() {
        let mut l = Rlimit { cur: 0, max: 0 };
        s::getrlimit(RLIMIT_CORE, Some(&mut l));
        let ok = s::setrlimit(RLIMIT_CORE, Some(&Rlimit { cur: 0, max: l.max })) == 0;
        let mut after = Rlimit { cur: 9, max: 9 };
        s::getrlimit(RLIMIT_CORE, Some(&mut after));
        errno::set_raw(0);
        let raise = root() || (l.max != u64::MAX && fails(s::setrlimit(RLIMIT_CORE, Some(&Rlimit { cur: 0, max: u64::MAX })) as isize, EPERM)) || l.max == u64::MAX;
        std::process::exit(if ok && after.cur == 0 && raise { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_setrlimit", "--exact", "--test-threads=1"])
        .env("LIELLA_S24_RL", "1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "lowered in a child; raising the hard limit needs privilege");
}

#[test]
fn sym_setrlimit64() {
    if std::env::var("LIELLA_S24_RL").is_ok() {
        let mut l = Rlimit { cur: 0, max: 0 };
        s::getrlimit(RLIMIT_CORE, Some(&mut l));
        let ok = s::setrlimit64(RLIMIT_CORE, Some(&Rlimit { cur: 0, max: l.max })) == 0;
        let mut after = Rlimit { cur: 9, max: 9 };
        s::getrlimit(RLIMIT_CORE, Some(&mut after));
        errno::set_raw(0);
        let raise = root() || (l.max != u64::MAX && fails(s::setrlimit64(RLIMIT_CORE, Some(&Rlimit { cur: 0, max: u64::MAX })) as isize, EPERM)) || l.max == u64::MAX;
        std::process::exit(if ok && after.cur == 0 && raise { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_setrlimit64", "--exact", "--test-threads=1"])
        .env("LIELLA_S24_RL", "1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "lowered in a child; raising the hard limit needs privilege");
}

symbol!(sym_sstk, "sstk", "24", "2.2.5", r##"(no installed header declares it)"##);
#[test]
fn sym_syscall() {
    assert_eq!(s::liella_syscall7(39, 0, 0, 0, 0, 0, 0), std::process::id() as i64, "SYS_getpid");
    let mut p = [0i32; 2];
    assert_eq!(s::liella_syscall7(293, p.as_mut_ptr() as i64, 0, 0, 0, 0, 0), 0, "SYS_pipe2 with an address");
    errno::set_raw(0);
    assert_eq!((s::liella_syscall7(3, -1, 0, 0, 0, 0, 0), errno::get()), (-1, 9), "SYS_close(-1): EBADF");
    assert_eq!((s::liella_syscall7(100_000, 0, 0, 0, 0, 0, 0), errno::get()), (-1, 38), "ENOSYS");
    liella_boundary::posix::s11_lowio::close(p[0]);
    liella_boundary::posix::s11_lowio::close(p[1]);
}

#[test]
fn sym_sysconf() {
    let sc = names("sc");
    let body: String = sc.iter().map(|(k, v)| format!("errno = 0; v = sysconf({v}); printf(\"{k} %ld %d\\n\", v, errno);")).collect();
    let mut diffs = Vec::new();
    for line in host("sysconf", &body) {
        let f: Vec<&str> = line.split(' ').collect();
        let (name, want, want_err) = (f[0], f[1].parse::<i64>().unwrap(), f[2].parse::<i32>().unwrap());
        errno::set_raw(0);
        let got = s::sysconf(sc[name]);
        let got_err = errno::get();
        if MOVING.contains(&name) {
            assert!((got - want).abs() < want / 10, "{name}: {got} vs {want}");
            continue;
        }
        if (got, got_err) != (want, want_err) {
            diffs.push(format!("{name}: liella {got}/{got_err}, glibc {want}/{want_err}"));
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    errno::set_raw(0);
    assert_eq!((s::sysconf(9999), errno::get()), (-1, EINVAL), "an unknown name");
}

symbol!(sym___sysctl, "__sysctl", "24", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_sysctl, "sysctl", "24", "2.2.5", r##"(no installed header declares it)"##);
#[test]
fn sym_sysinfo() {
    let mut i = Sysinfo::default();
    assert_eq!(s::sysinfo(Some(&mut i)), 0);
    let want = host("sysinfo", "#include <sys/sysinfo.h>\nstruct sysinfo i; sysinfo(&i); printf(\"%lu %lu %u\\n\", i.totalram, i.totalswap, i.mem_unit);");
    assert_eq!(vec![format!("{} {} {}", i.totalram, i.totalswap, i.mem_unit)], want);
    assert!(i.procs > 0 && i.uptime > 0);
    errno::set_raw(0);
    assert!(fails(s::sysinfo(None) as isize, EFAULT));
}
#[test]
fn sym_ulimit() {
    let b = s::liella_ulimit2(1, 0);
    let mut l = Rlimit { cur: 0, max: 0 };
    s::getrlimit(1, Some(&mut l));
    assert_eq!(b, if l.cur == u64::MAX { i64::MAX } else { (l.cur / 512) as i64 }, "UL_GETFSIZE: 512-byte blocks");
    errno::set_raw(0);
    assert!(fails(s::liella_ulimit2(42, 0) as isize, EINVAL));
    conformance(&["s24_variadic"]);
}

/// `uname`: the kernel's six fields, as the host's glibc reports them; NULL is `EFAULT`.
#[test]
fn sym_uname() {
    let mut u = Utsname::default();
    assert_eq!(s::uname(Some(&mut u)), 0);
    let text = |f: &[u8; 65]| String::from_utf8_lossy(&f[..f.iter().position(|&b| b == 0).unwrap()]).into_owned();
    let mine = [&u.sysname, &u.nodename, &u.release, &u.version, &u.machine, &u.domainname].map(text).join("|");
    let want = host("uname", "#include <sys/utsname.h>\nstruct utsname u; uname(&u); printf(\"%s|%s|%s|%s|%s|%s\\n\", u.sysname, u.nodename, u.release, u.version, u.machine, u.domainname);");
    assert_eq!(want, vec![mine]);
    errno::set_raw(0);
    assert!(fails(s::uname(None) as isize, EFAULT));
}
#[test]
fn sym_unshare() {
    // The export is in posix::s18_processes (its contract is §18.9): thin, -1 and errno.
    use liella_boundary::posix::s18_processes as p;
    errno::set_raw(0);
    assert_eq!((p::unshare(1), errno::get()), (-1, EINVAL), "an unknown flag");
    assert_eq!(p::unshare(0), 0, "nothing to detach");
}
symbol!(sym_uselib, "uselib", "24", "2.2.5", r##"(no installed header declares it)"##);
/// A thin wrapper: the result and `errno` the host's glibc gives for the same call (without
/// privilege the kernel refuses most of them).
#[test]
fn sym_vhangup() {
    if root() {
        return;
    }
    let want = host("vhangup", "errno = 0; int r = vhangup(); printf(\"%d %d\\n\", r, r < 0 ? errno : 0);");
    errno::set_raw(0);
    let r = s::vhangup();
    assert_eq!(vec![format!("{r} {}", if r < 0 { errno::get() } else { 0 })], want);
    errno::set_raw(0);
    assert!(fails(s::vhangup() as isize, EPERM));
}
#[test]
fn sym_vlimit() {
    errno::set_raw(0);
    assert!(fails(s::vlimit(0, 1) as isize, EINVAL), "LIM_NORAISE is not settable");
    assert!(fails(s::vlimit(7, 1) as isize, EINVAL));
    if std::env::var("LIELLA_S24_VL").is_ok() {
        let ok = s::vlimit(5, 0) == 0; // LIM_CORE: RLIMIT_CORE's soft limit
        let mut l = Rlimit { cur: 9, max: 9 };
        s::getrlimit(RLIMIT_CORE, Some(&mut l));
        std::process::exit(if ok && l.cur == 0 { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_vlimit", "--exact", "--test-threads=1"])
        .env("LIELLA_S24_VL", "1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn sym_vtimes() {
    let (mut cur, mut child) = (Vtimes::default(), Vtimes::default());
    // Spin until 50 ms of CPU time (3 units of 1/60 s) were charged, not 50 ms of wall time: on a loaded host
    // (the 5.10 guest under the parallel gate, 2026-10-08) 50 ms of wall time gave fewer than 2 units. The
    // wall-time cap only bounds a broken vtimes.
    let spin = std::time::Instant::now();
    loop {
        for _ in 0..100_000 {
            std::hint::black_box(0);
        }
        assert_eq!(s::vtimes(Some(&mut cur), Some(&mut child)), 0);
        if cur.utime >= 3 || spin.elapsed() > std::time::Duration::from_secs(20) {
            break;
        }
    }
    assert!(cur.utime >= 3, "1/60 s units: 50 ms of CPU time is 3");
    assert!(cur.maxrss > 0 && cur.minflt > 0);
    assert_eq!(s::vtimes(None, Some(&mut child)), 0, "either may be NULL");
}

#[test]
fn sym___getpagesize() {
    assert_eq!(s::__getpagesize(), 4096);
}

