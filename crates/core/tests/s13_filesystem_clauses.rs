// Specified from spec §13 (phase 5: §13.4–13.7, §13.10–13.12, §13.14–13.15; phase 12:
// §13.1–13.3, §13.8–13.9 and the mount-table streams, §13.13).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §13 (71 clauses).
//!
//! The calls are tested through the exports (the C caller's contract) and, where glibc adds
//! something to the kernel call, through `liella_legacy::fs_compat` with the kernel's answers
//! injected by `Faulty` (a missing `fchmodat2` or `faccessat2`).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "support/kernel.rs"]
mod kernel;

use core::ptr::null_mut;
use std::ffi::CString;
use std::sync::atomic::{AtomicU32, Ordering};

use liella::error::Errno;
use liella_abi::ctypes::c_int;
use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s13_filesystem as x;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{Stat, Statfs, Timespec, Timeval};
use liella_legacy::fs_compat;

const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 2;

const O_DIRECTORY: c_int = 0o200000;
const AT_FDCWD: c_int = -100;
const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
const AT_EACCESS: c_int = 0x200;
const AT_EMPTY_PATH: c_int = 0x1000;
const S_IFMT: u32 = 0o170000;

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Dir(std::path::PathBuf);
impl Dir {
    fn new(tag: &str) -> Dir {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13d-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir(&p).unwrap();
        Dir(p)
    }
    fn at(&self, name: &str) -> CString {
        CString::new(self.0.join(name).into_os_string().into_encoded_bytes()).unwrap()
    }
    fn file(&self, name: &str, content: &[u8]) -> CString {
        std::fs::write(self.0.join(name), content).unwrap();
        self.at(name)
    }
    fn open(&self) -> c_int {
        let c = CString::new(self.0.clone().into_os_string().into_encoded_bytes()).unwrap();
        io::liella_open3(c.as_ptr(), O_RDONLY | O_DIRECTORY, 0)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn k(c: &CString) -> KStr<'_> {
    KStr::from_nul_terminated(c.as_bytes_with_nul()).unwrap()
}

fn stat(p: &CString) -> Stat {
    let mut st = Stat::default();
    assert_eq!(x::lstat(p.as_ptr(), Some(&mut st)), 0);
    st
}

/// -1 with `errno == e`.
fn err(r: c_int, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

fn root() -> bool {
    CSys.geteuid() == 0
}

/// Runs `body` in a child (this test re-executed alone): process-wide state (the umask, IDs).
fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_C13_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env("LIELLA_C13_ALONE", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{name} failed in its child: {}", child_report(&o));
}

fn k_(c: &CString) -> KStr<'_> {
    k(c)
}

#[allow(dead_code)]
fn unused(_: Timeval, _: c_int) {}

#[test]
fn s13_n_1() {
    // The non-at forms are the at forms with AT_FDCWD: same answers, relative to the cwd.
    let d = Dir::new("n1");
    let f = d.file("f", b"abc");
    let (mut a, mut b) = (Stat::default(), Stat::default());
    assert_eq!(x::stat(f.as_ptr(), Some(&mut a)), 0);
    assert_eq!(x::fstatat(AT_FDCWD, f.as_ptr(), Some(&mut b), 0), 0);
    assert_eq!((a.ino, a.size), (b.ino, b.size));
    assert_eq!(x::unlink(f.as_ptr()), 0);
    errno::set_raw(0);
    assert!(err(x::unlinkat(AT_FDCWD, f.as_ptr(), 0), Errno::ENOENT), "the same name space");
}

#[test]
fn s13_n_2() {
    // Path resolution and permissions are the kernel's: a search-denied directory hides its
    // entries (EACCES) for an unprivileged caller, whatever the library.
    if root() {
        return;
    }
    let d = Dir::new("n2");
    std::fs::create_dir(d.0.join("closed")).unwrap();
    std::fs::write(d.0.join("closed/f"), b"").unwrap();
    std::fs::set_permissions(d.0.join("closed"), std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let mut st = Stat::default();
    errno::set_raw(0);
    assert!(err(x::stat(d.at("closed/f").as_ptr(), Some(&mut st)), Errno::EACCES));
    std::fs::set_permissions(d.0.join("closed"), std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
}

#[test]
fn s13_n_3() {
    // The *at form resolves against its directory descriptor, even after the directory moved:
    // the race the path form cannot avoid.
    let d = Dir::new("n3");
    std::fs::create_dir(d.0.join("a")).unwrap();
    std::fs::write(d.0.join("a/f"), b"x").unwrap();
    let dir = io::liella_open3(d.at("a").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    std::fs::rename(d.0.join("a"), d.0.join("b")).unwrap();
    let mut st = Stat::default();
    assert_eq!(x::fstatat(dir, c"f".as_ptr(), Some(&mut st), 0), 0, "still found through the descriptor");
    errno::set_raw(0);
    assert!(err(x::stat(d.at("a/f").as_ptr(), Some(&mut st)), Errno::ENOENT), "the path no longer names it");
    io::close(dir);
}

// ------------------------------------------------------------------------------ phase 12: §13.1–13.3, §13.8–13.9
//
// Process-wide state (the working directory, the root, the environment, the umask) is changed
// only in a child: the test re-executed alone (`p12::in_child`). Scratch directories live
// under Cargo's temporary directory for integration tests (never /tmp) and are removed by the
// parent; the deep tree (25 components of 200 bytes, paths over 5,000 bytes) is created and
// removed through descriptors.

use p12::*;

mod p12 {
    use core::ffi::c_void;
    use std::cell::{Cell, RefCell};
    use std::ffi::CString;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

    use liella_abi::ctypes::{c_char, c_int};
    use liella_abi::layout::struct_FTW;
    use liella_boundary::posix::s04_strings as st;
    use liella_boundary::sys::CSys;
    use liella_kernel::fd::Fd;
    use liella_kernel::path::KStr;
    use liella_kernel::sys::Sys;
    use liella_kernel::uapi::{Stat, AT_REMOVEDIR, O_CLOEXEC, O_CREAT, O_DIRECTORY, O_RDONLY, O_WRONLY, S_IFMT};

    use super::SEQ;

    pub(crate) const CHILD: &str = "LIELLA_C13_CHILD";
    const CHILD_DIR: &str = "LIELLA_C13_DIR";
    /// Components of the deep tree.
    pub(crate) const DEPTH: usize = 25;

    /// A scratch directory (see the section comment). In a child it is the parent's.
    pub(crate) struct Scratch {
        pub(crate) path: PathBuf,
        owned: bool,
    }

    impl Scratch {
        pub(crate) fn new(tag: &str) -> Scratch {
            if std::env::var(CHILD).is_ok() {
                if let Ok(p) = std::env::var(CHILD_DIR) {
                    return Scratch { path: PathBuf::from(p), owned: false };
                }
            }
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13-{}-{tag}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Scratch { path: std::fs::canonicalize(&p).unwrap(), owned: true }
        }
        pub(crate) fn at(&self, rel: &str) -> PathBuf {
            if rel.is_empty() { self.path.clone() } else { self.path.join(rel) }
        }
        pub(crate) fn c(&self, rel: &str) -> CString {
            cpath(&self.at(rel))
        }
        pub(crate) fn dir(&self, rel: &str) {
            std::fs::create_dir_all(self.at(rel)).unwrap();
        }
        pub(crate) fn file(&self, rel: &str) {
            std::fs::write(self.at(rel), b"").unwrap();
        }
        pub(crate) fn link(&self, target: &str, rel: &str) {
            std::os::unix::fs::symlink(target, self.at(rel)).unwrap();
        }
        pub(crate) fn mode(&self, rel: &str, m: u32) {
            std::fs::set_permissions(self.at(rel), std::fs::Permissions::from_mode(m)).unwrap();
        }
        /// The walk fixture `t` (and the loop `lp`): `t/a/{f1, sub/x, s_up -> ..}`, `t/b`, `t/e/`,
        /// `t/s_file -> b`, `t/s_dir -> a`, `t/s_dangling -> nowhere`, and for a non-root user
        /// `t/noread/` (0333) and `t/nosearch/` (0600), each holding one file.
        pub(crate) fn tree(&self) {
            if !self.owned {
                return;
            }
            self.dir("t/a/sub");
            self.file("t/a/f1");
            self.file("t/a/sub/x");
            self.link("..", "t/a/s_up");
            self.file("t/b");
            self.dir("t/e");
            self.link("b", "t/s_file");
            self.link("a", "t/s_dir");
            self.link("nowhere", "t/s_dangling");
            self.dir("lp");
            self.link("s_loop", "lp/s_loop");
            self.file("lp/f");
            if !super::root() {
                self.dir("t/noread");
                self.file("t/noread/nr_f");
                self.dir("t/nosearch");
                self.file("t/nosearch/ns_f");
                self.mode("t/noread", 0o333);
                self.mode("t/nosearch", 0o600);
            }
        }
    }

    fn unlock(p: &Path) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let q = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) && e.file_name() != "deep" {
                let _ = std::fs::set_permissions(&q, std::fs::Permissions::from_mode(0o755));
                unlock(&q);
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if self.owned {
                let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o755));
                unlock(&self.path);
                if self.path.join("deep").exists() {
                    remove_deep(&self.path);
                }
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }

    pub(crate) fn cpath(p: &Path) -> CString {
        CString::new(p.as_os_str().as_encoded_bytes()).unwrap()
    }

    pub(crate) fn z(b: &[u8]) -> Vec<u8> {
        let mut v = b.to_vec();
        v.push(0);
        v
    }

    pub(crate) fn kz(b: &[u8]) -> KStr<'_> {
        KStr::from_nul_terminated(b).unwrap()
    }

    /// The C text at `p` (copied with the library's own `strlen` and `memcpy`: no `unsafe`).
    pub(crate) fn ctext(p: *const c_char) -> Vec<u8> {
        if p.is_null() {
            return Vec::new();
        }
        let n = st::strlen(p);
        let mut v = vec![0u8; n];
        st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
        v
    }

    /// `n` bytes at `p`.
    pub(crate) fn cbytes(p: *const c_void, n: usize) -> Vec<u8> {
        let mut v = vec![0u8; n];
        st::memcpy(v.as_mut_ptr().cast(), p, n);
        v
    }

    pub(crate) fn word(b: &[u8], at: usize) -> u64 {
        u64::from_ne_bytes(b[at..at + 8].try_into().unwrap())
    }

    /// The working directory as the kernel names it.
    pub(crate) fn kernel_cwd() -> Vec<u8> {
        let mut b = vec![0u8; 65536];
        let n = CSys.getcwd(&mut b).unwrap();
        b.truncate(n - 1);
        b
    }

    pub(crate) fn lstat(p: &Path) -> Stat {
        CSys.fstatat(Fd::CWD, kz(&z(p.as_os_str().as_encoded_bytes())), 0x100).unwrap()
    }

    /// Run `body` in a child: this test re-executed alone, with the scratch directory `d`. The
    /// child must report success; its output is shown otherwise.
    pub(crate) fn in_child(name: &str, d: &Scratch, body: impl FnOnce(&Path)) {
        run_child(name, d, false, body);
    }

    /// As [`in_child`], the child in a new user and mount namespace where the system allows one
    /// (`unshare -U -r -m`, so that `chroot` and `mount` are permitted without privileges; the
    /// test's thread cannot enter one itself, the kernel requires a single-threaded process).
    /// The body asks [`private_namespace`] whether it got one.
    pub(crate) fn in_child_ns(name: &str, d: &Scratch, body: impl FnOnce(&Path)) {
        run_child(name, d, true, body);
    }

    const NS: &str = "LIELLA_C13_NS";

    fn run_child(name: &str, d: &Scratch, ns: bool, body: impl FnOnce(&Path)) {
        if std::env::var(CHILD).is_ok_and(|v| v == name) {
            body(&d.path);
            println!("child {name}: done");
            CSys.exit_group(0);
        }
        let unshare = ["-U", "-r", "-m", "--propagation", "private"];
        let ns = ns && std::process::Command::new("unshare").args(unshare).arg("true").output().is_ok_and(|o| o.status.success());
        let exe = std::env::current_exe().unwrap();
        let mut cmd = if ns {
            let mut c = std::process::Command::new("unshare");
            c.args(unshare).arg("--").arg(&exe).env(NS, "1");
            c
        } else {
            std::process::Command::new(&exe)
        };
        let o = cmd.args([name, "--exact", "--test-threads=1", "--nocapture"]).env(CHILD, name).env(CHILD_DIR, &d.path).output().unwrap();
        let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success() && out.contains(&format!("child {name}: done")), "{name} failed in its child ({:?}):\n{out}\n{err}", o.status);
    }

    /// Whether this child runs in its own user and mount namespace ([`in_child_ns`]).
    pub(crate) fn private_namespace() -> bool {
        std::env::var(NS).is_ok()
    }

    /// Build `<dir>/deep` + [`DEPTH`] components of 200 bytes + the file `bottom`, through
    /// descriptors (no path longer than one component is used).
    pub(crate) fn build_deep(dir: &Path) {
        let name = z(&[b'd'; 200]);
        if dir.join("deep").exists() {
            return;
        }
        std::fs::create_dir(dir.join("deep")).unwrap();
        let mut fd = CSys.openat(Fd::CWD, kz(&z(dir.join("deep").as_os_str().as_encoded_bytes())), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).unwrap();
        for _ in 0..DEPTH {
            CSys.mkdirat(fd, kz(&name), 0o755).unwrap();
            let n = CSys.openat(fd, kz(&name), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).unwrap();
            CSys.close(fd).unwrap();
            fd = n;
        }
        let f = CSys.openat(fd, kz(b"bottom\0"), O_WRONLY | O_CREAT | O_CLOEXEC, 0o644).unwrap();
        CSys.close(f).unwrap();
        CSys.close(fd).unwrap();
    }

    /// The descriptor of `<dir>/deep` plus `levels` components.
    pub(crate) fn open_deep(dir: &Path, levels: usize) -> Fd {
        let name = z(&[b'd'; 200]);
        let mut fd = CSys.openat(Fd::CWD, kz(&z(dir.join("deep").as_os_str().as_encoded_bytes())), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).unwrap();
        for _ in 0..levels {
            let n = CSys.openat(fd, kz(&name), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).unwrap();
            CSys.close(fd).unwrap();
            fd = n;
        }
        fd
    }

    /// Remove the deep tree through descriptors.
    pub(crate) fn remove_deep(dir: &Path) {
        let name = z(&[b'd'; 200]);
        let Ok(top) = CSys.openat(Fd::CWD, kz(&z(dir.join("deep").as_os_str().as_encoded_bytes())), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0) else { return };
        let mut fds = vec![top];
        while fds.len() <= DEPTH {
            match CSys.openat(*fds.last().unwrap(), kz(&name), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0) {
                Ok(f) => fds.push(f),
                Err(_) => break,
            }
        }
        let _ = CSys.fchmodat(*fds.last().unwrap(), kz(b".\0"), 0o755);
        let _ = CSys.unlinkat(*fds.last().unwrap(), kz(b"bottom\0"), 0);
        while fds.len() > 1 {
            let f = fds.pop().unwrap();
            let _ = CSys.close(f);
            let _ = CSys.unlinkat(*fds.last().unwrap(), kz(&name), AT_REMOVEDIR);
        }
        let _ = CSys.close(top);
        let _ = std::fs::remove_dir(dir.join("deep"));
    }

    /// One `nftw`/`ftw` callback as recorded.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct Rec {
        pub(crate) flag: i32,
        pub(crate) level: i32,
        pub(crate) base: i32,
        pub(crate) path: Vec<u8>,
        pub(crate) kind: u32,
        pub(crate) cwd: Option<Vec<u8>>,
    }

    thread_local! {
        static RECS: RefCell<Vec<Rec>> = const { RefCell::new(Vec::new()) };
        /// The callback's answer: at this path (and type code, -1 any), return this value.
        pub(crate) static ACT: RefCell<Option<(Vec<u8>, i32, i32)>> = const { RefCell::new(None) };
        pub(crate) static WITH_CWD: Cell<bool> = const { Cell::new(false) };
        pub(crate) static QUIET: Cell<bool> = const { Cell::new(false) };
    }

    fn answer(path: &[u8], flag: i32) -> c_int {
        ACT.with(|a| match &*a.borrow() {
            Some((p, f, v)) if p == path && (*f < 0 || *f == flag) => *v,
            _ => 0,
        })
    }

    pub(crate) extern "C" fn record(p: *const c_char, s: *const c_void, flag: c_int, ftw: *mut c_void) -> c_int {
        let path = ctext(p);
        let w = cbytes(ftw.cast_const(), size_of::<struct_FTW>());
        let (base, level) = (i32::from_ne_bytes(w[0..4].try_into().unwrap()), i32::from_ne_bytes(w[4..8].try_into().unwrap()));
        let sb = cbytes(s, size_of::<Stat>());
        let kind = u32::from_ne_bytes(sb[24..28].try_into().unwrap()) & S_IFMT;
        let cwd = WITH_CWD.get().then(kernel_cwd);
        if !QUIET.get() {
            RECS.with(|r| r.borrow_mut().push(Rec { flag, level, base, path: path.clone(), kind, cwd }));
        }
        answer(&path, flag)
    }

    pub(crate) extern "C" fn record_ftw(p: *const c_char, s: *const c_void, flag: c_int) -> c_int {
        let path = ctext(p);
        let sb = cbytes(s, size_of::<Stat>());
        let kind = u32::from_ne_bytes(sb[24..28].try_into().unwrap()) & S_IFMT;
        RECS.with(|r| r.borrow_mut().push(Rec { flag, level: -1, base: -1, path: path.clone(), kind, cwd: None }));
        answer(&path, flag)
    }

    /// Take the recorded callbacks.
    pub(crate) fn take() -> Vec<Rec> {
        RECS.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }

    /// The paths of `recs` relative to `dir`, each with its type code.
    pub(crate) fn names(recs: &[Rec], dir: &Path) -> Vec<(String, i32)> {
        let pre = dir.as_os_str().as_encoded_bytes();
        recs.iter()
            .map(|r| {
                let p = r.path.strip_prefix(pre).map(|q| q.strip_prefix(b"/").unwrap_or(q)).unwrap_or(&r.path);
                (String::from_utf8_lossy(p).into_owned(), r.flag)
            })
            .collect()
    }

    /// An `FTSENT` as the C caller reads it.
    #[derive(Clone, Debug)]
    pub(crate) struct Ent {
        pub(crate) addr: usize,
        pub(crate) info: u16,
        pub(crate) level: i16,
        pub(crate) errno: i32,
        pub(crate) name: String,
        pub(crate) path: String,
        pub(crate) acc: String,
        /// The `fts_path` and `fts_accpath` pointers themselves.
        pub(crate) path_at: usize,
        pub(crate) acc_at: usize,
        pub(crate) cycle: usize,
        pub(crate) parent: usize,
        pub(crate) link: usize,
        pub(crate) number: i64,
        pub(crate) pointer: usize,
        pub(crate) kind: u32,
        pub(crate) namelen: u16,
        pub(crate) pathlen: u16,
    }

    pub(crate) fn ent(p: *mut c_void) -> Option<Ent> {
        if p.is_null() {
            return None;
        }
        let b = cbytes(p, 120);
        let at = |a: u64| ctext(core::ptr::with_exposed_provenance(usize::try_from(a).unwrap()));
        let statp = word(&b, 104);
        let kind = if statp == 0 { 0 } else { u32::from_ne_bytes(cbytes(core::ptr::with_exposed_provenance(usize::try_from(statp).unwrap()), 144)[24..28].try_into().unwrap()) & S_IFMT };
        Some(Ent {
            addr: p.expose_provenance(),
            info: u16::from_ne_bytes(b[98..100].try_into().unwrap()),
            level: i16::from_ne_bytes(b[96..98].try_into().unwrap()),
            errno: i32::from_ne_bytes(b[56..60].try_into().unwrap()),
            name: String::from_utf8_lossy(&ctext(p.cast::<u8>().wrapping_add(112).cast())).into_owned(),
            path: String::from_utf8_lossy(&at(word(&b, 48))).into_owned(),
            acc: String::from_utf8_lossy(&at(word(&b, 40))).into_owned(),
            path_at: usize::try_from(word(&b, 48)).unwrap(),
            acc_at: usize::try_from(word(&b, 40)).unwrap(),
            cycle: usize::try_from(word(&b, 0)).unwrap(),
            parent: usize::try_from(word(&b, 8)).unwrap(),
            link: usize::try_from(word(&b, 16)).unwrap(),
            number: i64::from_ne_bytes(b[24..32].try_into().unwrap()),
            pointer: usize::try_from(word(&b, 32)).unwrap(),
            kind,
            namelen: u16::from_ne_bytes(b[66..68].try_into().unwrap()),
            pathlen: u16::from_ne_bytes(b[64..66].try_into().unwrap()),
        })
    }

    /// `fts_open`'s comparison by `fts_name`.
    pub(crate) extern "C" fn by_name(a: *const *const c_void, b: *const *const c_void) -> c_int {
        let name = |pp: *const *const c_void| {
            let p = usize::try_from(word(&cbytes(pp.cast(), 8), 0)).unwrap();
            ctext(core::ptr::with_exposed_provenance::<u8>(p).wrapping_add(112).cast())
        };
        match name(a).cmp(&name(b)) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }

    /// A NULL-terminated `char *` array over `args`.
    pub(crate) fn argv(args: &[CString]) -> Vec<*mut c_char> {
        let mut v: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
        v.push(core::ptr::null_mut());
        v
    }
}

const ENOENT_: i32 = 2;
const EBADF_: i32 = 9;
const EINTR_: i32 = 4;
const FTW_F: i32 = 0;
const FTW_D: i32 = 1;
const FTW_DNR: i32 = 2;
const FTW_NS: i32 = 3;
const FTW_SL: i32 = 4;
const FTW_DP: i32 = 5;
const FTW_SLN: i32 = 6;
const FTW_PHYS: c_int = 1;
const FTW_MOUNT: c_int = 2;
const FTW_CHDIR: c_int = 4;
const FTW_DEPTH: c_int = 8;
const FTW_ACTIONRETVAL: c_int = 16;
const S_IFDIR_: u32 = 0o040000;
const S_IFREG_: u32 = 0o100000;
const S_IFLNK_: u32 = 0o120000;

fn walk(path: &CString, flags: c_int) -> (c_int, i32, Vec<Rec>) {
    take();
    errno::set_raw(0);
    let r = x::nftw(path.as_ptr(), Some(record), 10, flags);
    let e = errno::get();
    (r, e, take())
}

fn act(path: Option<&std::path::Path>, flag: i32, v: i32) {
    ACT.with(|a| *a.borrow_mut() = path.map(|p| (p.as_os_str().as_encoded_bytes().to_vec(), flag, v)));
}

#[test]
fn s13_1_c_1() {
    // getcwd stores the absolute path of the working directory (the kernel's answer) and
    // returns the caller's buffer.
    let k = kernel_cwd();
    assert_eq!(k.first(), Some(&b'/'), "absolute");
    let mut buf = vec![b'Z' as i8; k.len() + 1];
    let r = x::getcwd(buf.as_mut_ptr(), buf.len());
    assert_eq!(r, buf.as_mut_ptr(), "the buffer itself");
    assert_eq!(ctext(r), k);
}

#[test]
fn s13_1_c_1_1() {
    // A NULL buffer is allocated: `size` bytes when given (a smaller path), else exactly enough;
    // a path that does not fit `size` is ERANGE.
    let k = kernel_cwd();
    let n = k.len();
    assert_eq!(ctext(x::getcwd(null_mut(), 0)), k, "size 0: the exact size");
    assert_eq!(ctext(x::getcwd(null_mut(), n + 1)), k, "size len+1");
    assert_eq!(ctext(x::getcwd(null_mut(), 100_000)), k, "a larger size");
    for size in [n, 1] {
        errno::set_raw(0);
        assert!(x::getcwd(null_mut(), size).is_null() && errno::get() == Errno::ERANGE.get(), "size {size}: ERANGE");
    }
    // A working directory longer than a page (the kernel cannot name it): found by walking up.
    let d = Scratch::new("c111");
    in_child("s13_1_c_1_1", &d, |dir| {
        build_deep(dir);
        let fd = open_deep(dir, DEPTH);
        assert_eq!(x::fchdir(fd.raw()), 0);
        let want = dir.as_os_str().len() + "/deep".len() + DEPTH * 201;
        let p = ctext(x::getcwd(null_mut(), 0));
        assert_eq!(p.len(), want, "the whole path, {} bytes", want);
        assert!(p.starts_with(dir.as_os_str().as_encoded_bytes()));
        errno::set_raw(0);
        assert!(x::getcwd(null_mut(), 4096).is_null() && errno::get() == Errno::ERANGE.get(), "PATH_MAX bytes are too few: ERANGE");
        let mut big = vec![0i8; want + 1];
        assert!(!x::getcwd(big.as_mut_ptr(), want + 1).is_null(), "exactly len+1 bytes");
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
    });
}

#[test]
fn s13_1_c_1_2() {
    // An unreachable working directory (outside the root after chroot) is ENOENT; the caller's
    // buffer holds the kernel's "(unreachable)" text.
    let d = Scratch::new("c112");
    in_child_ns("s13_1_c_1_2", &d, |dir| {
        std::fs::create_dir_all(dir.join("c/in")).unwrap();
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        if !private_namespace() {
            println!("no user namespace here: the chroot case is covered by fstree_products' root phase");
            return;
        }
        assert_eq!(x::chroot(cpath(&dir.join("c")).as_ptr()), 0);
        let mut buf = vec![0i8; 4096];
        errno::set_raw(0);
        assert!(x::getcwd(buf.as_mut_ptr(), buf.len()).is_null());
        assert_eq!(errno::get(), Errno::ENOENT.get());
        assert!(ctext(buf.as_ptr()).starts_with(b"(unreachable)"), "the kernel's text is left in the buffer");
        errno::set_raw(0);
        assert!(x::getcwd(null_mut(), 0).is_null() && errno::get() == Errno::ENOENT.get());
        errno::set_raw(0);
        assert!(x::get_current_dir_name().is_null() && errno::get() == Errno::ENOENT.get());
        let mut rb = vec![b'Q' as i8; 4096];
        errno::set_raw(0);
        assert!(x::realpath(c".".as_ptr(), rb.as_mut_ptr()).is_null() && errno::get() == Errno::ENOENT.get());
        assert_eq!(ctext(rb.as_ptr()), b"", "the resolved prefix is empty");
        assert_eq!(ctext(x::realpath(c"/in/..".as_ptr(), null_mut())), b"/", "absolute paths resolve in the new root");
    });
}

#[test]
fn s13_1_c_2() {
    // get_current_dir_name: $PWD when it names the working directory (symbolic links kept),
    // the physical path otherwise.
    let d = Scratch::new("c12");
    in_child("s13_1_c_2", &d, |dir| {
        std::fs::create_dir_all(dir.join("c/sub")).unwrap();
        std::os::unix::fs::symlink("c/sub", dir.join("c_link")).unwrap();
        assert_eq!(x::chdir(cpath(&dir.join("c/sub")).as_ptr()), 0);
        let phys = dir.join("c/sub").as_os_str().as_encoded_bytes().to_vec();
        let pwd = |v: Option<&std::path::Path>| match v {
            Some(p) => assert_eq!(liella_boundary::posix::s01_startup::setenv(c"PWD".as_ptr(), cpath(p).as_ptr(), 1), 0),
            None => assert_eq!(liella_boundary::posix::s01_startup::unsetenv(c"PWD".as_ptr()), 0),
        };
        pwd(None);
        assert_eq!(ctext(x::get_current_dir_name()), phys, "PWD unset");
        pwd(Some(&dir.join("c_link")));
        assert_eq!(ctext(x::get_current_dir_name()), dir.join("c_link").as_os_str().as_encoded_bytes(), "a symbolic link to the directory is kept");
        pwd(Some(&dir.join("c")));
        assert_eq!(ctext(x::get_current_dir_name()), phys, "another directory: the physical path");
        pwd(Some(&dir.join("missing")));
        assert_eq!(ctext(x::get_current_dir_name()), phys, "a missing path: the physical path");
        // Owner ruling 2026-10-03 (D-13.14): only an absolute $PWD without `.` or `..`
        // components is accepted, even when it names the working directory.
        pwd(Some(&dir.join("c/./sub")));
        assert_eq!(ctext(x::get_current_dir_name()), phys, "a `.` component: the physical path");
        pwd(Some(&dir.join("c/sub/../sub")));
        assert_eq!(ctext(x::get_current_dir_name()), phys, "a `..` component: the physical path");
        pwd(Some(std::path::Path::new(".")));
        assert_eq!(ctext(x::get_current_dir_name()), phys, "a relative $PWD: the physical path");
        let mut doubled = b"/".to_vec();
        doubled.extend_from_slice(dir.join("c_link").as_os_str().as_encoded_bytes());
        pwd(Some(std::path::Path::new(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(&doubled))));
        assert_eq!(ctext(x::get_current_dir_name()), doubled, "repeated slashes are not dot components");
        pwd(None);
    });
}

#[test]
fn s13_1_c_3() {
    // getwd writes into a PATH_MAX buffer and returns it; a longer working directory cannot be
    // written (ENAMETOOLONG, the buffer untouched).
    let mut buf = vec![0i8; 4096];
    let r = x::getwd(buf.as_mut_ptr());
    assert_eq!(r, buf.as_mut_ptr());
    assert_eq!(ctext(r), kernel_cwd());
    let d = Scratch::new("c13");
    in_child("s13_1_c_3", &d, |dir| {
        build_deep(dir);
        assert_eq!(x::fchdir(open_deep(dir, DEPTH).raw()), 0);
        let mut b = vec![b'Z' as i8; 4096];
        errno::set_raw(0);
        assert!(x::getwd(b.as_mut_ptr()).is_null());
        assert_eq!(errno::get(), Errno::ENAMETOOLONG.get());
        assert!(b.iter().all(|&c| c == b'Z' as i8), "untouched");
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
    });
}

#[test]
fn s13_1_c_4() {
    // chdir and fchdir change the working directory of the whole process: another thread sees
    // the change.
    let d = Scratch::new("c14");
    in_child("s13_1_c_4", &d, |dir| {
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("f"), b"").unwrap();
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        assert_eq!(kernel_cwd(), dir.as_os_str().as_encoded_bytes());
        let seen = std::thread::spawn(kernel_cwd).join().unwrap();
        assert_eq!(seen, dir.as_os_str().as_encoded_bytes(), "process-wide");
        let fd = io::liella_open3(cpath(&dir.join("sub")).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
        assert_eq!(x::fchdir(fd), 0);
        assert_eq!(kernel_cwd(), dir.join("sub").as_os_str().as_encoded_bytes());
        assert_eq!(std::thread::spawn(kernel_cwd).join().unwrap(), dir.join("sub").as_os_str().as_encoded_bytes());
        io::close(fd);
        let f = io::liella_open3(cpath(&dir.join("f")).as_ptr(), O_RDONLY, 0);
        errno::set_raw(0);
        assert!(err(x::fchdir(f), Errno::ENOTDIR), "a file");
        io::close(f);
        assert!(err(x::fchdir(-1), Errno::EBADF));
        assert!(err(x::chdir(cpath(&dir.join("f")).as_ptr()), Errno::ENOTDIR));
        assert!(err(x::chdir(cpath(&dir.join("missing")).as_ptr()), Errno::ENOENT));
        assert!(err(x::chdir(c"".as_ptr()), Errno::ENOENT));
    });
}

#[test]
fn s13_1_c_5() {
    // chroot needs CAP_SYS_CHROOT (EPERM without it) and leaves the working directory alone.
    let d = Scratch::new("c15");
    if !root() {
        errno::set_raw(0);
        assert!(err(x::chroot(d.c("").as_ptr()), Errno::EPERM), "unprivileged");
    }
    in_child_ns("s13_1_c_5", &d, |dir| {
        std::fs::create_dir_all(dir.join("c/in")).unwrap();
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        let before = lstat(dir);
        let newroot = lstat(&dir.join("c"));
        if !private_namespace() {
            println!("no user namespace here: covered by fstree_products' root phase");
            return;
        }
        assert_eq!(x::chroot(cpath(&dir.join("c")).as_ptr()), 0);
        let dot = CSys.fstatat(Fd::CWD, kz(b".\0"), 0).unwrap();
        assert_eq!((dot.dev, dot.ino), (before.dev, before.ino), "the working directory is unchanged");
        let slash = CSys.fstatat(Fd::CWD, kz(b"/\0"), 0).unwrap();
        assert_eq!((slash.dev, slash.ino), (newroot.dev, newroot.ino), "/ is the new root");
        errno::set_raw(0);
        assert!(err(x::chroot(c"/missing".as_ptr()), Errno::ENOENT));
    });
}

#[test]
fn s13_1_f_0() {
    // ERANGE for a buffer too small; ENOENT for a removed working directory; EACCES when an
    // ancestor of a long working directory cannot be read; ENAMETOOLONG for paths and names
    // over the kernel's limits.
    let mut b = [0i8; 2];
    errno::set_raw(0);
    assert!(x::getcwd(b.as_mut_ptr(), 2).is_null() && errno::get() == Errno::ERANGE.get());
    errno::set_raw(0);
    assert!(x::getcwd(b.as_mut_ptr(), 0).is_null() && errno::get() == Errno::EINVAL.get(), "size 0 with a buffer");
    let long = vec![b'n'; 256];
    assert!(err(x::chdir(CString::new(long).unwrap().as_ptr()), Errno::ENAMETOOLONG), "a 256-byte component");
    let mut dots = b"./".repeat(2100);
    dots.push(b'.');
    assert!(err(x::chdir(CString::new(dots).unwrap().as_ptr()), Errno::ENAMETOOLONG), "a path over PATH_MAX");
    let d = Scratch::new("c1f0");
    in_child("s13_1_f_0", &d, |dir| {
        std::fs::create_dir_all(dir.join("gone")).unwrap();
        assert_eq!(x::chdir(cpath(&dir.join("gone")).as_ptr()), 0);
        std::fs::remove_dir(dir.join("gone")).unwrap();
        let mut b = vec![0i8; 4096];
        errno::set_raw(0);
        assert!(x::getcwd(b.as_mut_ptr(), 4096).is_null() && errno::get() == Errno::ENOENT.get(), "removed: getcwd");
        errno::set_raw(0);
        assert!(x::getwd(b.as_mut_ptr()).is_null() && errno::get() == Errno::ENOENT.get(), "removed: getwd");
        errno::set_raw(0);
        assert!(x::get_current_dir_name().is_null() && errno::get() == Errno::ENOENT.get(), "removed: get_current_dir_name");
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        if !root() {
            build_deep(dir);
            assert_eq!(x::fchdir(open_deep(dir, DEPTH).raw()), 0);
            let up = vec!["..".to_string(); DEPTH - 10].join("/");
            let upc = CString::new(up).unwrap();
            assert_eq!(x::chmod(upc.as_ptr(), 0o111), 0, "level 10: search only");
            errno::set_raw(0);
            assert!(x::getcwd(null_mut(), 0).is_null() && errno::get() == Errno::EACCES.get(), "an unreadable ancestor");
            errno::set_raw(0);
            assert!(x::realpath(c".".as_ptr(), null_mut()).is_null() && errno::get() == Errno::EACCES.get());
            assert_eq!(x::chmod(upc.as_ptr(), 0o755), 0);
            assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        }
    });
}

#[test]
fn s13_2_c_1() {
    // opendir and fdopendir open streams; fdopendir takes the descriptor (closedir closes it),
    // sets close-on-exec, keeps its position, and refuses what is not a readable directory
    // without taking it.
    use liella_kernel::ops::FcntlInt;
    let d = Scratch::new("c21");
    d.dir("r/d");
    d.file("r/f");
    let dp = x::opendir(d.c("r").as_ptr());
    assert!(!dp.is_null());
    let fd = x::dirfd(dp);
    assert!(fd >= 0);
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(1), "close-on-exec");
    assert_eq!(x::closedir(dp), 0);
    let fd = io::liella_open3(d.c("r").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let dp = x::fdopendir(fd);
    assert!(!dp.is_null());
    assert_eq!(x::dirfd(dp), fd, "the same descriptor");
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(1));
    let mut n = 0;
    while !x::readdir(dp).is_null() {
        n += 1;
    }
    assert_eq!(n, 4);
    assert_eq!(x::closedir(dp), 0);
    // Descriptor numbers are process-wide: a closed one is checked alone, in a child, so that
    // no concurrently running test reuses the number meanwhile.
    in_child("s13_2_c_1", &d, |dir| {
        let fd = io::liella_open3(cpath(&dir.join("r")).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
        assert_eq!(x::closedir(x::fdopendir(fd)), 0);
        assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Err(Errno::EBADF), "closedir closed the descriptor it was given");
    });
    // At the end already: no implicit rewind.
    let fd = io::liella_open3(d.c("r").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut buf = vec![0u8; 4096];
    while CSys.getdents64(Fd::new(fd), &mut buf).unwrap() > 0 {}
    let dp = x::fdopendir(fd);
    assert!(x::readdir(dp).is_null(), "the descriptor's position is kept");
    x::rewinddir(dp);
    assert!(!x::readdir(dp).is_null());
    x::closedir(dp);
    let f = io::liella_open3(d.c("r/f").as_ptr(), O_RDONLY, 0);
    errno::set_raw(0);
    assert!(x::fdopendir(f).is_null() && errno::get() == Errno::ENOTDIR.get());
    assert!(CSys.fcntl_int(Fd::new(f), FcntlInt::GetFd, 0).is_ok(), "not taken on failure");
    io::close(f);
    let p = io::liella_open3(d.c("r").as_ptr(), 0o10000000 | O_DIRECTORY, 0);
    errno::set_raw(0);
    assert!(x::fdopendir(p).is_null() && errno::get() == Errno::EBADF.get(), "an O_PATH descriptor cannot be read");
    io::close(p);
    errno::set_raw(0);
    assert!(x::fdopendir(-1).is_null() && errno::get() == Errno::EBADF.get());
    errno::set_raw(0);
    assert!(x::opendir(d.c("r/f").as_ptr()).is_null() && errno::get() == Errno::ENOTDIR.get());
    errno::set_raw(0);
    assert!(x::opendir(d.c("missing").as_ptr()).is_null() && errno::get() == Errno::ENOENT.get());
}

/// The fields of the `struct dirent` at `p`: ino, off, reclen, type, name.
fn dirent_at(p: *mut liella_abi::layout::struct_dirent) -> (u64, i64, u16, u8, Vec<u8>) {
    let h = cbytes(p.cast_const().cast(), 19);
    let name = ctext(p.cast::<u8>().wrapping_add(19).cast());
    (word(&h, 0), word(&h, 8) as i64, u16::from_ne_bytes(h[16..18].try_into().unwrap()), h[18], name)
}

#[test]
fn s13_2_c_2() {
    // readdir returns each entry with d_ino, d_off (the position after it), d_reclen, d_type and
    // d_name, then NULL with errno untouched.
    let d = Scratch::new("c22");
    d.dir("r/d");
    d.file("r/f");
    d.link("f", "r/l");
    let n255 = "n".repeat(255);
    d.file(&format!("r/{n255}"));
    let dp = x::opendir(d.c("r").as_ptr());
    let mut seen = Vec::new();
    loop {
        let e = x::readdir(dp);
        if e.is_null() {
            break;
        }
        let (ino, off, reclen, ty, name) = dirent_at(e);
        assert_eq!(off, x::telldir(dp), "d_off is the position after the entry");
        assert_eq!(usize::from(reclen), (19 + name.len() + 1).next_multiple_of(8), "the record's length");
        let full = d.at("r").join(String::from_utf8_lossy(&name).as_ref());
        let st = lstat(&full);
        assert_eq!(ino, st.ino, "{}", String::from_utf8_lossy(&name));
        let want = match st.mode & S_IFMT {
            0o040000 => 4,
            0o120000 => 10,
            _ => 8,
        };
        assert_eq!(ty, want, "d_type of {}", String::from_utf8_lossy(&name));
        seen.push(String::from_utf8_lossy(&name).into_owned());
    }
    seen.sort();
    let mut want = vec![".".to_string(), "..".to_string(), "d".to_string(), "f".to_string(), "l".to_string(), n255];
    want.sort();
    assert_eq!(seen, want);
    for preset in [0, EINTR_] {
        errno::set_raw(preset);
        assert!(x::readdir(dp).is_null());
        assert_eq!(errno::get(), preset, "the end leaves errno as it was");
    }
    assert_eq!(x::closedir(dp), 0);
}

#[test]
fn s13_2_c_2_1() {
    // Each stream has its own storage: two streams on one directory, read alternately or from
    // two threads, each return every entry; an entry stays intact while the other stream reads.
    let d = Scratch::new("c221");
    for i in 0..40 {
        d.file(&format!("e{i:02}"));
    }
    let (a, b) = (x::opendir(d.c("").as_ptr()), x::opendir(d.c("").as_ptr()));
    let first = x::readdir(a);
    let name = dirent_at(first).4;
    let mut nb = 0;
    while !x::readdir(b).is_null() {
        nb += 1;
    }
    assert_eq!(dirent_at(first).4, name, "untouched by the other stream");
    let mut na = 1;
    while !x::readdir(a).is_null() {
        na += 1;
    }
    assert_eq!((na, nb), (42, 42));
    x::closedir(a);
    x::closedir(b);
    let path = d.c("");
    let count = |p: CString| {
        let s = x::opendir(p.as_ptr());
        let mut n = 0;
        while !x::readdir(s).is_null() {
            n += 1;
        }
        x::closedir(s);
        n
    };
    let (p1, p2) = (path.clone(), path.clone());
    let t1 = std::thread::spawn(move || (0..20).map(|_| count(p1.clone())).collect::<Vec<_>>());
    let t2 = std::thread::spawn(move || (0..20).map(|_| count(p2.clone())).collect::<Vec<_>>());
    assert!(t1.join().unwrap().iter().chain(t2.join().unwrap().iter()).all(|&n| n == 42), "concurrent streams");
}

#[test]
fn s13_2_c_2_2() {
    // d_type is the entry's type when the file system knows it; DT_UNKNOWN only tells the caller
    // to stat. Every known type here agrees with lstat.
    let d = Scratch::new("c222");
    d.dir("x/d");
    d.file("x/f");
    d.link("f", "x/l");
    let dp = x::opendir(d.c("x").as_ptr());
    loop {
        let e = x::readdir(dp);
        if e.is_null() {
            break;
        }
        let (_, _, _, ty, name) = dirent_at(e);
        let st = lstat(&d.at("x").join(String::from_utf8_lossy(&name).as_ref()));
        if ty != 0 {
            assert_eq!(u32::from(ty) << 12, st.mode & S_IFMT, "{}", String::from_utf8_lossy(&name));
        }
    }
    x::closedir(dp);
}

#[test]
fn s13_2_c_3() {
    // readdir_r copies the entry into the caller's buffer and points *result at it; at the end
    // it returns 0 with *result NULL; errors are returned (errno is not the channel).
    use liella_abi::layout::{struct_dirent, struct_dirent64};
    let d = Scratch::new("c23");
    d.file("f");
    let dp = x::opendir(d.c("").as_ptr());
    let mut e = struct_dirent { d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256] };
    let mut res: *mut struct_dirent = null_mut();
    let mut names = Vec::new();
    loop {
        assert_eq!(x::readdir_r(dp, Some(&mut e), Some(&mut res)), 0);
        if res.is_null() {
            break;
        }
        assert_eq!(res, core::ptr::from_mut(&mut e), "the caller's buffer");
        names.push(ctext(e.d_name.as_ptr()));
        assert_eq!(e.d_ino, lstat(&d.at(&String::from_utf8_lossy(names.last().unwrap()))).ino);
    }
    names.sort();
    assert_eq!(names, vec![b".".to_vec(), b"..".to_vec(), b"f".to_vec()]);
    assert_eq!(x::readdir_r(dp, Some(&mut e), Some(&mut res)), 0, "after the end");
    assert!(res.is_null());
    x::closedir(dp);
    // A descriptor closed underneath, checked alone (descriptor numbers are process-wide).
    in_child("s13_2_c_3", &d, |dir| {
        let dp = x::opendir(cpath(dir).as_ptr());
        io::close(x::dirfd(dp));
        let mut e = struct_dirent { d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256] };
        let mut res: *mut struct_dirent = core::ptr::without_provenance_mut(8);
        assert_eq!(x::readdir_r(dp, Some(&mut e), Some(&mut res)), EBADF_, "the error is returned");
        assert!(res.is_null());
    });
    let dp64 = x::opendir(d.c("").as_ptr());
    let mut e64 = struct_dirent64 { d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256] };
    let mut r64: *mut struct_dirent64 = null_mut();
    let mut n = 0;
    while x::readdir64_r(dp64, Some(&mut e64), Some(&mut r64)) == 0 && !r64.is_null() {
        n += 1;
    }
    assert_eq!(n, 3);
    x::closedir(dp64);
}

#[test]
fn s13_2_c_4() {
    // telldir positions are the file system's cookies: seekdir to one continues with the entry
    // read there, in this or another stream on the directory; the end position gives NULL.
    let d = Scratch::new("c24");
    for i in 0..58 {
        d.file(&format!("e{i:02}"));
    }
    let dp = x::opendir(d.c("").as_ptr());
    assert_eq!(x::telldir(dp), 0, "the start");
    let mut pos = Vec::new();
    loop {
        let t = x::telldir(dp);
        let e = x::readdir(dp);
        if e.is_null() {
            break;
        }
        pos.push((t, dirent_at(e).4));
    }
    let end = x::telldir(dp);
    assert_eq!(pos.len(), 60);
    let mut cookies: Vec<i64> = pos.iter().map(|p| p.0).collect();
    cookies.sort_unstable();
    cookies.dedup();
    assert_eq!(cookies.len(), 60, "distinct");
    assert!(!cookies.contains(&end));
    for &j in &[0usize, 1, 17, 30, 59, 3] {
        x::seekdir(dp, pos[j].0);
        assert_eq!(x::telldir(dp), pos[j].0);
        assert_eq!(dirent_at(x::readdir(dp)).4, pos[j].1, "entry {j}");
    }
    x::seekdir(dp, end);
    assert!(x::readdir(dp).is_null());
    let other = x::opendir(d.c("").as_ptr());
    x::seekdir(other, pos[30].0);
    assert_eq!(dirent_at(x::readdir(other)).4, pos[30].1, "a position works in a second stream");
    x::seekdir(dp, pos[10].0);
    for p in &pos[10..] {
        assert_eq!(dirent_at(x::readdir(dp)).4, p.1, "the rest repeats in order");
    }
    x::closedir(other);
    x::closedir(dp);
}

#[test]
fn s13_2_c_4_across_buffer_growth() {
    // S13.2.c.4 on a directory larger than a stream's first fill and its full buffer (the buffer
    // grows in place, FAULTS-TARGZ-2, doubling 4 -> 8 -> 16 -> 32 KiB, TAR-HEAP): every entry once, from opendir and fdopendir, and
    // telldir positions on both sides of the growth still resume at their entry.
    let d = Scratch::new("c4g");
    let name = |i: usize| format!("grow-{i:04}-padding-to-make-the-records-longer");
    for i in 0..700 {
        d.file(&name(i));
    }
    let mut want: Vec<Vec<u8>> = (0..700).map(|i| name(i).into_bytes()).collect();
    want.push(b".".to_vec());
    want.push(b"..".to_vec());
    want.sort();
    for from_fd in [false, true] {
        let dp = if from_fd {
            x::fdopendir(io::liella_open3(d.c("").as_ptr(), O_RDONLY | O_DIRECTORY, 0))
        } else {
            x::opendir(d.c("").as_ptr())
        };
        assert!(!dp.is_null());
        let mut pos = Vec::new();
        loop {
            let t = x::telldir(dp);
            let e = x::readdir(dp);
            if e.is_null() {
                break;
            }
            pos.push((t, dirent_at(e).4));
        }
        let mut got: Vec<Vec<u8>> = pos.iter().map(|p| p.1.clone()).collect();
        got.sort();
        assert_eq!(got, want, "every entry once (fdopendir: {from_fd})");
        for &j in &[650usize, 3, 60, 300, 701] {
            x::seekdir(dp, pos[j].0);
            assert_eq!(dirent_at(x::readdir(dp)).4, pos[j].1, "entry {j} (fdopendir: {from_fd})");
        }
        assert_eq!(x::closedir(dp), 0);
    }
}

#[test]
fn s13_2_c_5() {
    // rewinddir restarts the stream and sees the directory as it is now; closedir closes the
    // stream and its descriptor; dirfd is the descriptor.
    use liella_kernel::ops::FcntlInt;
    let d = Scratch::new("c25");
    d.file("a");
    d.file("b");
    let dp = x::opendir(d.c("").as_ptr());
    while !x::readdir(dp).is_null() {}
    d.file("c");
    std::fs::remove_file(d.at("a")).unwrap();
    x::rewinddir(dp);
    let mut names = Vec::new();
    loop {
        let e = x::readdir(dp);
        if e.is_null() {
            break;
        }
        names.push(dirent_at(e).4);
    }
    names.sort();
    assert_eq!(names, vec![b".".to_vec(), b"..".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    let fd = x::dirfd(dp);
    assert!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).is_ok());
    assert_eq!(x::closedir(dp), 0);
    // That the descriptor is closed is checked alone (descriptor numbers are process-wide).
    in_child("s13_2_c_5", &d, |dir| {
        let dp = x::opendir(cpath(dir).as_ptr());
        let fd = x::dirfd(dp);
        assert_eq!(x::closedir(dp), 0);
        assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Err(Errno::EBADF));
    });
}

#[test]
fn s13_2_c_6() {
    // getdents64 is the system call: records of the same names; getdirentries also stores the
    // position before the read.
    let d = Scratch::new("c26");
    d.file("f");
    d.dir("g");
    let fd = io::liella_open3(d.c("").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut buf = vec![0u8; 4096];
    errno::set_raw(0);
    assert_eq!(x::getdents64(fd, buf.as_mut_ptr().cast(), 1), -1);
    assert_eq!(errno::get(), Errno::EINVAL.get(), "a buffer smaller than a record");
    let n = x::getdents64(fd, buf.as_mut_ptr().cast(), buf.len());
    let mut names = Vec::new();
    let mut off = 0;
    while off < n as usize {
        let reclen = usize::from(u16::from_ne_bytes(buf[off + 16..off + 18].try_into().unwrap()));
        let name: Vec<u8> = buf[off + 19..off + reclen].iter().take_while(|&&c| c != 0).copied().collect();
        names.push(name);
        off += reclen;
    }
    names.sort();
    assert_eq!(names, vec![b".".to_vec(), b"..".to_vec(), b"f".to_vec(), b"g".to_vec()]);
    assert_eq!(x::getdents64(fd, buf.as_mut_ptr().cast(), buf.len()), 0, "the end");
    io::close(fd);
    let fd = io::liella_open3(d.c("").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut base = 12345i64;
    assert!(x::getdirentries(fd, buf.as_mut_ptr().cast(), buf.len(), Some(&mut base)) > 0);
    assert_eq!(base, 0, "the position before the first read");
    io::close(fd);
}

#[test]
fn s13_2_g_0() {
    // Exactly once: 1,500 entries that exist throughout are each returned once while 600 others
    // are created and 300 removed during the scan (every 50 entries read, 15 created and 8
    // removed); no name is returned twice; . and .. once each.
    let d = Scratch::new("c2g0");
    let (ns, nv, nn) = (1500usize, 300usize, 600usize);
    for i in 0..ns {
        d.file(&format!("stable-{i:04}-padding-to-make-the-records-longer"));
    }
    for i in 0..nv {
        d.file(&format!("victim-{i:04}-padding-to-make-the-records-longer"));
    }
    let dp = x::opendir(d.c("").as_ptr());
    let mut counts = std::collections::HashMap::<Vec<u8>, u32>::new();
    let (mut reads, mut created, mut removed) = (0, 0, 0);
    errno::set_raw(0);
    loop {
        let e = x::readdir(dp);
        if e.is_null() {
            break;
        }
        *counts.entry(dirent_at(e).4).or_default() += 1;
        reads += 1;
        if reads % 50 == 0 {
            for _ in 0..15 {
                if created < nn {
                    d.file(&format!("new-{created:04}-padding-to-make-the-records-longer"));
                    created += 1;
                }
            }
            for _ in 0..8 {
                if removed < nv {
                    std::fs::remove_file(d.at(&format!("victim-{removed:04}-padding-to-make-the-records-longer"))).unwrap();
                    removed += 1;
                }
            }
        }
    }
    assert_eq!(errno::get(), 0, "the scan ended normally");
    x::closedir(dp);
    for i in 0..ns {
        assert_eq!(counts.get(format!("stable-{i:04}-padding-to-make-the-records-longer").as_bytes()), Some(&1), "stable entry {i}");
    }
    assert_eq!((counts.get(b".".as_slice()), counts.get(b"..".as_slice())), (Some(&1), Some(&1)));
    assert!(counts.values().all(|&c| c == 1), "no entry twice");
    for i in removed..nv {
        assert_eq!(counts.get(format!("victim-{i:04}-padding-to-make-the-records-longer").as_bytes()), Some(&1), "victim {i} was never removed");
    }
    assert!(removed > 100 && created > 300, "the directory changed during the scan ({removed} removed, {created} created)");
}

#[test]
fn s13_2_f_0() {
    // NULL means the end (errno untouched) or an error: EBADF when the descriptor is closed
    // underneath; ENOENT once a directory removed while open has no buffered entries left
    // (D-13.2, owner ruling 2026-10-03), on every further call. EOVERFLOW cannot occur
    // (ino_t is 64 bits).
    let d = Scratch::new("c2f0");
    // A descriptor closed underneath, checked alone (descriptor numbers are process-wide).
    in_child("s13_2_f_0", &d, |dir| {
        let dp = x::opendir(cpath(dir).as_ptr());
        io::close(x::dirfd(dp));
        errno::set_raw(0);
        assert!(x::readdir(dp).is_null() && errno::get() == EBADF_);
        errno::set_raw(0);
        assert!(err(x::closedir(dp), Errno::EBADF), "closedir reports the descriptor already gone");
    });
    let mut sf = Statfs::default();
    assert_eq!(x::statfs(d.c("").as_ptr(), Some(&mut sf)), 0);
    eprintln!("scratch file system f_type {:#x} (ext4 is 0xef53)", sf.ftype);
    // Removed before the first read.
    d.dir("rm");
    let dp = x::opendir(d.c("rm").as_ptr());
    std::fs::remove_dir(d.at("rm")).unwrap();
    for _ in 0..2 {
        errno::set_raw(0);
        assert!(x::readdir(dp).is_null());
        assert_eq!(errno::get(), ENOENT_, "the removed directory");
    }
    x::rewinddir(dp);
    errno::set_raw(0);
    assert!(x::readdir(dp).is_null() && errno::get() == ENOENT_, "after rewinddir too");
    x::closedir(dp);
    // Removed after part of a large directory was read: the buffered entries come first.
    d.dir("big");
    for i in 0..2000 {
        d.file(&format!("big/entry-{i:04}-padding-to-make-the-records-longer"));
    }
    let dp = x::opendir(d.c("big").as_ptr());
    for _ in 0..50 {
        assert!(!x::readdir(dp).is_null());
    }
    for i in 0..2000 {
        std::fs::remove_file(d.at(&format!("big/entry-{i:04}-padding-to-make-the-records-longer"))).unwrap();
    }
    std::fs::remove_dir(d.at("big")).unwrap();
    let mut more = 0;
    errno::set_raw(0);
    while !x::readdir(dp).is_null() {
        more += 1;
    }
    assert!(more > 0 && more < 1952, "some buffered entries ({more}), not the rest");
    assert_eq!(errno::get(), ENOENT_);
    let mut e = liella_abi::layout::struct_dirent { d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256] };
    let mut res = null_mut();
    assert_eq!(x::readdir_r(dp, Some(&mut e), Some(&mut res)), ENOENT_, "readdir_r returns it");
    assert!(res.is_null());
    x::closedir(dp);
}

/// The names `scandir` listed (the array read through the library's own memcpy).
fn listed(list: *mut *mut liella_abi::layout::struct_dirent, n: c_int) -> Vec<String> {
    let words = cbytes(list.cast_const().cast(), usize::try_from(n).unwrap() * 8);
    (0..usize::try_from(n).unwrap()).map(|i| String::from_utf8_lossy(&dirent_at(core::ptr::with_exposed_provenance_mut(word(&words, i * 8) as usize)).4).into_owned()).collect()
}

thread_local! {
    static FILTERED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `alphasort` and `versionsort` as callback values (the exports are `extern "C"`; the callback
/// type is `extern "C-unwind"`).
extern "C" fn alpha(a: *const *const core::ffi::c_void, b: *const *const core::ffi::c_void) -> c_int {
    x::alphasort(a, b)
}

extern "C" fn version(a: *const *const core::ffi::c_void, b: *const *const core::ffi::c_void) -> c_int {
    x::versionsort(a, b)
}

extern "C" fn no_dots(e: *const core::ffi::c_void) -> c_int {
    FILTERED.set(FILTERED.get() + 1);
    let name = ctext(e.cast::<u8>().wrapping_add(19).cast());
    c_int::from(name.first() != Some(&b'.'))
}

#[test]
fn s13_3_c_1() {
    // scandir: every entry offered to the filter (. and .. too), the kept ones sorted by the
    // comparison; alphasort is strcoll (byte order in the C locale), versionsort strverscmp.
    let d = Scratch::new("c31");
    for n in ["a", "B", "b", "A", "_c", "a10", "a2", "a1"] {
        d.file(n);
    }
    let mut list = null_mut();
    let n = x::scandir(d.c("").as_ptr(), Some(&mut list), None, Some(alpha));
    assert_eq!(listed(list, n), [".", "..", "A", "B", "_c", "a", "a1", "a10", "a2", "b"]);
    let n = x::scandir(d.c("").as_ptr(), Some(&mut list), None, Some(version));
    assert_eq!(listed(list, n), [".", "..", "A", "B", "_c", "a", "a1", "a2", "a10", "b"]);
    FILTERED.set(0);
    let n = x::scandir(d.c("").as_ptr(), Some(&mut list), Some(no_dots), Some(alpha));
    assert_eq!(n, 8);
    assert_eq!(FILTERED.get(), 10, "the filter sees every entry");
    let n = x::scandir(d.c("").as_ptr(), Some(&mut list), None, None);
    let mut all = listed(list, n);
    all.sort();
    assert_eq!(all.len(), 10, "no comparison: the directory's order, every entry");
    let sentinel = core::ptr::without_provenance_mut(0x1000);
    let mut l2 = sentinel;
    errno::set_raw(0);
    assert_eq!(x::scandir(d.c("missing").as_ptr(), Some(&mut l2), None, Some(alpha)), -1);
    assert_eq!(errno::get(), ENOENT_);
    assert_eq!(l2, sentinel, "the list is written only on success");
}

#[test]
fn s13_3_c_2() {
    // nftw reports each entry with its path (the root as given, then names below it), its
    // status, a type code, the level below the root and the offset of the last component.
    let d = Scratch::new("c32");
    d.tree();
    let (r, _, recs) = walk(&d.c("t/a"), FTW_PHYS);
    assert_eq!(r, 0);
    let top = d.at("t/a").as_os_str().len() as i32;
    let want = [("t/a", FTW_D, 0, top - 1, S_IFDIR_), ("t/a/f1", FTW_F, 1, top + 1, S_IFREG_), ("t/a/sub", FTW_D, 1, top + 1, S_IFDIR_), ("t/a/sub/x", FTW_F, 2, top + 5, S_IFREG_), ("t/a/s_up", FTW_SL, 1, top + 1, S_IFLNK_)];
    let mut got: Vec<(String, i32, i32, i32, u32)> = names(&recs, &d.path).into_iter().zip(&recs).map(|((p, f), r)| (p, f, r.level, r.base, r.kind)).collect();
    got.sort();
    let mut want: Vec<(String, i32, i32, i32, u32)> = want.iter().map(|(p, f, l, b, k)| (p.to_string(), *f, *l, *b, *k)).collect();
    want.sort();
    assert_eq!(got, want);
    let pre: Vec<String> = names(&recs, &d.path).into_iter().map(|p| p.0).collect();
    assert_eq!(pre[0], "t/a", "pre-order: the directory before its entries");
}

#[test]
fn s13_3_c_2_1() {
    // The type codes: F, D, DNR (cannot be read), DP (post-order), NS (cannot be examined),
    // SL (a link, physical walk), SLN (a link naming nothing, links followed).
    let d = Scratch::new("c321");
    d.tree();
    let (_, _, recs) = walk(&d.c("t"), FTW_PHYS);
    let n = names(&recs, &d.path);
    let has = |p: &str, f: i32| n.iter().any(|(q, g)| q == p && *g == f);
    assert!(has("t/b", FTW_F) && has("t/e", FTW_D) && has("t/s_dir", FTW_SL) && has("t/s_dangling", FTW_SL));
    if !root() {
        assert!(has("t/noread", FTW_DNR), "a directory that cannot be read");
        assert!(!n.iter().any(|(q, _)| q.starts_with("t/noread/")), "nothing inside it");
        assert!(has("t/nosearch", FTW_D) && has("t/nosearch/ns_f", FTW_NS), "an entry that cannot be examined");
    }
    let (_, _, recs) = walk(&d.c("t"), 0);
    let n = names(&recs, &d.path);
    let has = |p: &str, f: i32| n.iter().any(|(q, g)| q == p && *g == f);
    assert!(has("t/s_dangling", FTW_SLN) && has("t/s_file", FTW_F), "links followed");
    let (_, _, recs) = walk(&d.c("t"), FTW_PHYS | FTW_DEPTH);
    let n = names(&recs, &d.path);
    assert!(n.iter().any(|(q, g)| q == "t" && *g == FTW_DP) && n.last().unwrap().0 == "t", "post-order: the root last");
}

#[test]
fn s13_3_c_2_2() {
    // The flags: FTW_PHYS (links not followed), FTW_DEPTH (directories after their contents),
    // FTW_CHDIR (the callback runs in the entry's directory), FTW_MOUNT (another file system is
    // not entered), FTW_ACTIONRETVAL (the callback skips subtrees and siblings).
    let d = Scratch::new("c322");
    d.tree();
    let (_, _, phys) = walk(&d.c("t"), FTW_PHYS);
    assert!(!names(&phys, &d.path).iter().any(|(p, _)| p.starts_with("t/s_dir/")), "FTW_PHYS: a link is not entered");
    let (_, _, follow) = walk(&d.c("t"), 0);
    let fol = names(&follow, &d.path);
    let dirs: Vec<&String> = fol.iter().filter(|(_, f)| *f == FTW_D).map(|(p, _)| p).collect();
    // t/s_dir links to t/a, so the walk enters that directory once, through whichever of the two
    // directory order (per file system, D-T.1) lists first; its entries come under that path.
    let via = if dirs.contains(&&"t/s_dir".to_string()) { "t/s_dir/f1" } else { "t/a/f1" };
    assert!(fol.iter().any(|(p, _)| p == via), "links followed: the target's entries under the path that reached it ({via})");
    assert!(!(dirs.contains(&&"t/a".to_string()) && dirs.contains(&&"t/s_dir".to_string())), "each directory once");
    let (_, _, depth) = walk(&d.c("t/a"), FTW_PHYS | FTW_DEPTH);
    let dn = names(&depth, &d.path);
    let pos = |p: &str| dn.iter().position(|(q, _)| q == p).unwrap();
    assert!(pos("t/a/sub/x") < pos("t/a/sub") && pos("t/a/sub") < pos("t/a"), "FTW_DEPTH");
    act(Some(&d.at("t/a")), FTW_D, 2);
    let (r, _, skip) = walk(&d.c("t"), FTW_PHYS | FTW_ACTIONRETVAL);
    act(None, -1, 0);
    assert_eq!(r, 0);
    assert!(!names(&skip, &d.path).iter().any(|(p, _)| p.starts_with("t/a/")), "FTW_SKIP_SUBTREE");
    act(Some(&d.at("t/a/f1")), -1, 3);
    let (_, _, sib) = walk(&d.c("t/a"), FTW_PHYS | FTW_ACTIONRETVAL);
    act(None, -1, 0);
    assert_eq!(names(&sib, &d.path).last().unwrap().0, "t/a/f1", "FTW_SKIP_SIBLINGS ends the directory");
    act(Some(&d.at("t/a/f1")), -1, 1);
    let (r, _, _) = walk(&d.c("t/a"), FTW_PHYS | FTW_ACTIONRETVAL);
    act(None, -1, 0);
    assert_eq!(r, 1, "FTW_STOP");
    // FTW_ACTIONRETVAL's other answers (the reference's, fstree.glibc.txt): FTW_SKIP_SUBTREE at a
    // file is FTW_CONTINUE; a value outside 0..3 ends the walk and is returned; either skip at
    // the root ends the walk with 0.
    act(Some(&d.at("t/b")), -1, 2);
    let (r, _, all) = walk(&d.c("t"), FTW_PHYS | FTW_ACTIONRETVAL);
    act(None, -1, 0);
    assert_eq!((r, all.len()), (0, phys.len()), "FTW_SKIP_SUBTREE at a file: nothing skipped");
    act(Some(&d.at("t/b")), -1, 99);
    let (r, _, cut) = walk(&d.c("t"), FTW_PHYS | FTW_ACTIONRETVAL);
    act(None, -1, 0);
    assert_eq!((r, names(&cut, &d.path).last().unwrap().0.as_str()), (99, "t/b"), "another value: returned, the walk ended");
    for v in [2, 3] {
        act(Some(&d.at("t")), FTW_D, v);
        let (r, _, recs) = walk(&d.c("t"), FTW_PHYS | FTW_ACTIONRETVAL);
        act(None, -1, 0);
        assert_eq!((r, recs.len()), (0, 1), "skip {v} at the root: only the root, 0");
    }
    in_child_ns("s13_3_c_2_2", &d, |dir| {
        let u = dir.join("u");
        std::fs::create_dir_all(u.join("a/sub")).unwrap();
        std::fs::write(u.join("a/sub/x"), b"").unwrap();
        std::fs::write(u.join("b"), b"").unwrap();
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        WITH_CWD.set(true);
        let (r, _, recs) = walk(&cpath(&u), FTW_PHYS | FTW_CHDIR);
        WITH_CWD.set(false);
        assert_eq!(r, 0);
        for rec in &recs {
            let p = std::path::PathBuf::from(String::from_utf8(rec.path.clone()).unwrap());
            let want = if rec.level == 0 { dir.to_path_buf() } else { p.parent().unwrap().to_path_buf() };
            assert_eq!(rec.cwd.as_deref(), Some(want.as_os_str().as_encoded_bytes()), "FTW_CHDIR: {}", p.display());
        }
        assert_eq!(kernel_cwd(), dir.as_os_str().as_encoded_bytes(), "the caller's directory afterwards");
        if private_namespace() {
            std::fs::create_dir_all(u.join("mnt")).unwrap();
            if x::mount(c"tmpfs".as_ptr(), cpath(&u.join("mnt")).as_ptr(), c"tmpfs".as_ptr(), 0, core::ptr::null()) == 0 {
                std::fs::write(u.join("mnt/inner"), b"").unwrap();
                let (_, _, all) = walk(&cpath(&u), FTW_PHYS);
                assert!(names(&all, dir).iter().any(|(p, _)| p == "u/mnt/inner"));
                let (_, _, one) = walk(&cpath(&u), FTW_PHYS | FTW_MOUNT);
                assert!(!names(&one, dir).iter().any(|(p, _)| p.starts_with("u/mnt")), "FTW_MOUNT: neither the mount point nor its contents");
                x::umount2(cpath(&u.join("mnt")).as_ptr(), 0);
            }
        }
    });
}

#[test]
fn s13_3_c_2_3() {
    // A non-zero return ends the walk at once and is returned (without FTW_ACTIONRETVAL any
    // value, FTW_STOP included).
    let d = Scratch::new("c323");
    d.tree();
    for v in [42, -7, 1] {
        act(Some(&d.at("t/a/sub/x")), -1, v);
        let (r, _, recs) = walk(&d.c("t/a"), FTW_PHYS);
        act(None, -1, 0);
        assert_eq!(r, v);
        assert_eq!(names(&recs, &d.path).last().unwrap().0, "t/a/sub/x", "nothing after it");
    }
}

#[test]
fn s13_3_c_3() {
    // ftw: links always followed (no SL/SLN codes): a link to a directory is walked under the
    // link's path, a dangling link is FTW_NS.
    let d = Scratch::new("c33");
    d.tree();
    take();
    assert_eq!(x::ftw(d.c("t").as_ptr(), Some(record_ftw), 10), 0);
    let recs = take();
    let n = names(&recs, &d.path);
    assert!(n.iter().all(|(_, f)| *f != FTW_SL && *f != FTW_SLN));
    assert!(n.iter().any(|(p, f)| p == "t/s_dangling" && *f == FTW_NS));
    assert!(n.iter().any(|(p, f)| p == "t/s_file" && *f == FTW_F));
    act(Some(&d.at("t/b")), -1, 5);
    take();
    assert_eq!(x::ftw(d.c("t").as_ptr(), Some(record_ftw), 10), 5, "a non-zero return is returned");
    act(None, -1, 0);
    take();
}

#[test]
fn s13_3_c_4() {
    // fts: entries in pre-order and post-order (sorted by the comparison), fts_set's SKIP,
    // AGAIN and FOLLOW, fts_children, and FTS_DC with fts_cycle naming the ancestor repeated.
    let d = Scratch::new("c34");
    d.tree();
    // An entry's fts_instr (bytes 102-103).
    let instr_of = |addr: usize| {
        let b = cbytes(core::ptr::with_exposed_provenance(addr), 104);
        u16::from_ne_bytes([b[102], b[103]])
    };
    let read_all = |f: *mut core::ffi::c_void, acts: &[(&str, u16, c_int)]| {
        let mut out = Vec::new();
        while let Some(e) = ent(x::fts_read(f)) {
            // fts_cur, the FTS structure's first word, names the entry just returned.
            assert_eq!(word(&cbytes(f, 8), 0), u64::try_from(e.addr).unwrap(), "fts_cur after fts_read");
            // FTS_NOINSTR in a new entry and again once fts_read has acted on an instruction
            // (probe: glibc 2.39).
            assert_eq!(instr_of(e.addr), 3, "fts_instr of {} (info {}) after fts_read", e.path, e.info);
            out.push((e.path.clone(), e.info));
            for (p, info, instr) in acts {
                if e.path.ends_with(p) && e.info == *info && out.iter().filter(|(q, i)| q == &e.path && i == info).count() == 1 {
                    assert_eq!(x::fts_set(f, core::ptr::with_exposed_provenance_mut(e.addr), *instr), 0);
                    // fts_set writes the instruction into the entry's fts_instr (probe: glibc 2.39).
                    let b = cbytes(core::ptr::with_exposed_provenance(e.addr), 104);
                    assert_eq!(c_int::from(u16::from_ne_bytes([b[102], b[103]])), *instr, "fts_instr after fts_set");
                }
            }
        }
        out
    };
    let args = [d.c("t/a")];
    let av = argv(&args);
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    let all = read_all(f, &[]);
    let rel: Vec<(String, u16)> = all.iter().map(|(p, i)| (p.strip_prefix(&d.path.display().to_string()).unwrap_or(p).to_string(), *i)).collect();
    assert_eq!(rel, [("/t/a".into(), 1), ("/t/a/f1".into(), 8), ("/t/a/s_up".into(), 12), ("/t/a/sub".into(), 1), ("/t/a/sub/x".into(), 8), ("/t/a/sub".into(), 6), ("/t/a".into(), 6)]);
    assert_eq!(x::fts_close(f), 0);
    // The FTSENT fields: lengths, the access path (the path, without chdir), the parent, the
    // caller's fts_number kept from pre-order to post-order, the status.
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    let mut root = None;
    while let Some(e) = ent(x::fts_read(f)) {
        assert_eq!((usize::from(e.namelen), usize::from(e.pathlen)), (e.name.len(), e.path.len()));
        assert_eq!(e.acc, e.path, "FTS_NOCHDIR: the access path is the path");
        if e.level == 0 && e.info == 1 {
            assert_eq!((e.name.as_str(), e.kind, e.number), ("a", S_IFDIR_, 0));
            root = Some(e.addr);
            // The caller's field: the library never writes it after creating the entry.
            let mut seven = 7i64.to_ne_bytes();
            liella_boundary::posix::s04_strings::memcpy(core::ptr::with_exposed_provenance_mut::<u8>(e.addr + 24).cast(), seven.as_mut_ptr().cast_const().cast(), 8);
        } else if e.level == 0 {
            assert_eq!((e.addr, e.number), (root.unwrap(), 7), "the post-order entry is the pre-order one, fts_number kept");
        } else if e.level == 1 {
            assert_eq!(e.parent, root.unwrap(), "fts_parent");
        }
    }
    x::fts_close(f);
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    let skipped = read_all(f, &[("/t/a/sub", 1, 4)]);
    assert!(!skipped.iter().any(|(p, _)| p.ends_with("sub/x")) && skipped.iter().any(|(p, i)| p.ends_with("/t/a/sub") && *i == 6), "FTS_SKIP: no entries, the post-order entry still");
    x::fts_close(f);
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    let again = read_all(f, &[("/t/a/f1", 8, 1)]);
    assert_eq!(again.iter().filter(|(p, _)| p.ends_with("/f1")).count(), 2, "FTS_AGAIN returns the entry again");
    x::fts_close(f);
    let targs = [d.c("t")];
    let tav = argv(&targs);
    let f = x::fts_open(tav.as_ptr(), 16 | 4, Some(by_name));
    let followed = read_all(f, &[("/t/s_dir", 12, 2)]);
    assert!(followed.iter().any(|(p, i)| p.ends_with("/t/s_dir") && *i == 1), "FTS_FOLLOW: the link returned again, as the directory it names");
    assert!(followed.iter().any(|(p, i)| p.ends_with("/t/s_dir/f1") && *i == 8), "and walked under the link's path");
    x::fts_close(f);
    // fts_children lists the current directory's entries without moving.
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    assert!(ent(x::fts_read(f)).is_some());
    let mut kids = Vec::new();
    let mut c = ent(x::fts_children(f, 0));
    while let Some(e) = c {
        assert_eq!(instr_of(e.addr), 3, "fts_instr of a listed entry: FTS_NOINSTR (probe: glibc 2.39)");
        kids.push(e.name.clone());
        c = ent(core::ptr::with_exposed_provenance_mut(e.link));
    }
    assert_eq!(kids, ["f1", "s_up", "sub"]);
    x::fts_close(f);
    // FTS_SKIP on listed entries (probe: glibc 2.39): a directory is never returned (neither
    // order); a file is returned, its fts_instr still FTS_SKIP.
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    assert!(ent(x::fts_read(f)).is_some());
    let mut c = ent(x::fts_children(f, 0));
    while let Some(e) = c {
        if e.name == "sub" || e.name == "f1" {
            assert_eq!(x::fts_set(f, core::ptr::with_exposed_provenance_mut(e.addr), 4), 0);
        }
        c = ent(core::ptr::with_exposed_provenance_mut(e.link));
    }
    let mut rest = Vec::new();
    while let Some(e) = ent(x::fts_read(f)) {
        rest.push((e.name.clone(), e.info, instr_of(e.addr)));
    }
    assert_eq!(rest, [("f1".into(), 8, 4), ("s_up".into(), 12, 3), ("a".into(), 6, 3)], "listed and skipped");
    x::fts_close(f);
    // fts_instr written directly, without fts_set, is acted on as fts_set's (probe: glibc 2.39,
    // FTS-INSTR-DIRECT), on listed entries and on the entry fts_read returned.
    let put_instr = |addr: usize, v: u16| {
        let mut b = v.to_ne_bytes();
        liella_boundary::posix::s04_strings::memcpy(core::ptr::with_exposed_provenance_mut::<u8>(addr + 102).cast(), b.as_mut_ptr().cast_const().cast(), 2);
    };
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    assert!(ent(x::fts_read(f)).is_some());
    let mut c = ent(x::fts_children(f, 0));
    while let Some(e) = c {
        if e.name == "sub" || e.name == "f1" {
            put_instr(e.addr, 4);
        }
        c = ent(core::ptr::with_exposed_provenance_mut(e.link));
    }
    let mut rest = Vec::new();
    while let Some(e) = ent(x::fts_read(f)) {
        rest.push((e.name.clone(), e.info, instr_of(e.addr)));
    }
    assert_eq!(rest, [("f1".into(), 8, 4), ("s_up".into(), 12, 3), ("a".into(), 6, 3)], "listed and marked FTS_SKIP directly");
    x::fts_close(f);
    // FTS_FOLLOW on a listed link, through fts_set and written directly: when reached, the link
    // is returned as the directory it names (FTS_NOINSTR again) and walked under its own path.
    for direct in [false, true] {
        let f = x::fts_open(tav.as_ptr(), 16 | 4, Some(by_name));
        assert!(ent(x::fts_read(f)).is_some());
        let mut c = ent(x::fts_children(f, 0));
        while let Some(e) = c {
            if e.name == "s_dir" {
                if direct {
                    put_instr(e.addr, 2);
                } else {
                    assert_eq!(x::fts_set(f, core::ptr::with_exposed_provenance_mut(e.addr), 2), 0);
                }
            }
            c = ent(core::ptr::with_exposed_provenance_mut(e.link));
        }
        let mut got = Vec::new();
        while let Some(e) = ent(x::fts_read(f)) {
            got.push((e.path.clone(), e.info, instr_of(e.addr)));
        }
        assert!(got.iter().any(|(p, i, n)| p.ends_with("/t/s_dir") && *i == 1 && *n == 3), "listed FTS_FOLLOW (direct {direct}): the directory the link names");
        assert!(got.iter().any(|(p, i, _)| p.ends_with("/t/s_dir/f1") && *i == 8), "listed FTS_FOLLOW (direct {direct}): walked under the link");
        assert!(!got.iter().any(|(p, i, _)| p.ends_with("/t/s_dir") && *i == 12), "listed FTS_FOLLOW (direct {direct}): not returned as the link");
        x::fts_close(f);
    }
    // On the entry fts_read returned: FTS_AGAIN returns it again, FTS_SKIP drops a directory's
    // entries, and a value that is no instruction is ignored and reset to FTS_NOINSTR.
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    let (mut seen, mut again, mut marked) = (Vec::new(), false, None);
    while let Some(e) = ent(x::fts_read(f)) {
        if let Some(a) = marked.take() {
            assert_eq!(instr_of(a), 3, "fts_read reset a value that is no instruction");
        }
        seen.push((e.name.clone(), e.info));
        if e.name == "f1" && !again {
            again = true;
            put_instr(e.addr, 1);
        } else if e.name == "s_up" {
            put_instr(e.addr, 7);
            marked = Some(e.addr);
        } else if e.name == "sub" && e.info == 1 {
            put_instr(e.addr, 4);
        }
    }
    let want: Vec<(String, u16)> = [("a", 1), ("f1", 8), ("f1", 8), ("s_up", 12), ("sub", 1), ("sub", 6), ("a", 6)].iter().map(|(n, i)| (n.to_string(), *i)).collect();
    assert_eq!(seen, want, "instructions written directly on fts_read's entries");
    x::fts_close(f);
    // fts_set takes FTS_NOINSTR (3) and 0 too, and stores them.
    let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
    let r = x::fts_read(f);
    for v in [4, 3, 0] {
        assert_eq!(x::fts_set(f, r, v), 0, "fts_set {v}");
        assert_eq!(c_int::from(instr_of(r.addr())), v, "fts_instr after fts_set {v}");
    }
    // The 0 left above, used by the next fts_read (which rewrites nothing else in the root).
    assert!(ent(x::fts_read(f)).is_some());
    assert_eq!(instr_of(r.addr()), 3, "fts_read used the instruction up: FTS_NOINSTR in the entry again");
    x::fts_close(f);
    // FTS_LOGICAL: t/a/s_up repeats t.
    let args = [d.c("t")];
    let av = argv(&args);
    let f = x::fts_open(av.as_ptr(), 2, Some(by_name));
    let mut root_addr = 0;
    let mut dc = None;
    while let Some(e) = ent(x::fts_read(f)) {
        if e.level == 0 && e.info == 1 {
            root_addr = e.addr;
        }
        if e.info == 2 && e.path.ends_with("/t/a/s_up") {
            dc = Some(e);
        }
    }
    let dc = dc.unwrap();
    assert_eq!(dc.cycle, root_addr, "fts_cycle is the ancestor t");
    x::fts_close(f);
    // FTS_SEEDOT: each directory's . and .. as FTS_DOT entries (after its pre-order entry);
    // without it, none.
    let all_of = |paths: &[CString], opts: c_int| {
        let av = argv(paths);
        let f = x::fts_open(av.as_ptr(), opts, Some(by_name));
        let mut out = Vec::new();
        while let Some(e) = ent(x::fts_read(f)) {
            out.push(e);
        }
        assert_eq!(x::fts_close(f), 0);
        out
    };
    let rel = |e: &Ent| e.path.strip_prefix(&format!("{}/", d.path.display())).unwrap_or(&e.path).to_string();
    let seen = all_of(&[d.c("t/a")], 16 | 32 | 4);
    let dots: Vec<String> = seen.iter().filter(|e| e.info == 5).map(rel).collect();
    assert_eq!(dots, ["t/a/.", "t/a/..", "t/a/sub/.", "t/a/sub/.."], "FTS_SEEDOT");
    assert!(seen.iter().filter(|e| e.info == 5).all(|e| e.level >= 1 && (e.name == "." || e.name == "..")));
    assert!(!all_of(&[d.c("t/a")], 16 | 4).iter().any(|e| e.info == 5), "no FTS_DOT without FTS_SEEDOT");
    // FTS_COMFOLLOW: a root that is a symbolic link is followed in a physical walk (only the
    // root: links below it stay FTS_SL); without it the root is FTS_SL.
    let follow = all_of(&[d.c("t/s_dir")], 16 | 1 | 4);
    assert_eq!((follow[0].info, follow[0].level), (1, 0), "FTS_COMFOLLOW: the root as the directory it names");
    assert!(follow.iter().any(|e| rel(e) == "t/s_dir/sub/x" && e.info == 8));
    assert!(follow.iter().any(|e| rel(e) == "t/s_dir/s_up" && e.info == 12), "a link below the root stays FTS_SL");
    let plain = all_of(&[d.c("t/s_dir")], 16 | 4);
    assert_eq!((plain.len(), plain[0].info), (1, 12), "without FTS_COMFOLLOW: the root link itself");
    let dang = all_of(&[d.c("t/s_dangling")], 16 | 1 | 4);
    assert_eq!(dang[0].info, 13, "FTS_COMFOLLOW on a dangling root: FTS_SLNONE");
    // chdir mode (no FTS_NOCHDIR) and FTS_XDEV change the process's working directory and mount
    // table: in a child.
    in_child_ns("s13_3_c_4", &d, |dir| {
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        let args = [CString::new("t/a").unwrap()];
        let av = argv(&args);
        let f = x::fts_open(av.as_ptr(), 16, Some(by_name));
        let mut n = 0;
        while let Some(e) = ent(x::fts_read(f)) {
            n += 1;
            let cwd = kernel_cwd();
            if e.level == 0 {
                assert_eq!((e.acc.as_str(), cwd.as_slice()), ("t/a", dir.as_os_str().as_encoded_bytes()), "the root: its path, from the caller's directory");
            } else if e.info != 6 {
                let parent = dir.join(std::path::Path::new(&e.path).parent().unwrap());
                assert_eq!((e.acc.as_str(), cwd.as_slice()), (e.name.as_str(), parent.as_os_str().as_encoded_bytes()), "chdir mode: fts_accpath is the name, in the parent ({})", e.path);
            }
        }
        assert!(n >= 7);
        assert_eq!(x::fts_close(f), 0);
        assert_eq!(kernel_cwd(), dir.as_os_str().as_encoded_bytes(), "fts_close returns to the caller's directory");
        if private_namespace() {
            let m = dir.join("m");
            std::fs::create_dir_all(m.join("mnt")).unwrap();
            std::fs::write(m.join("f"), b"").unwrap();
            if x::mount(c"tmpfs".as_ptr(), cpath(&m.join("mnt")).as_ptr(), c"tmpfs".as_ptr(), 0, core::ptr::null()) == 0 {
                std::fs::write(m.join("mnt/inner"), b"").unwrap();
                for opts in [16 | 64 | 4, 16 | 64] {
                    let args = [CString::new("m").unwrap()];
                    let av = argv(&args);
                    let f = x::fts_open(av.as_ptr(), opts, Some(by_name));
                    let mut got = Vec::new();
                    while let Some(e) = ent(x::fts_read(f)) {
                        got.push((e.path.clone(), e.info));
                    }
                    x::fts_close(f);
                    let want: Vec<(String, u16)> = [("m", 1), ("m/f", 8), ("m/mnt", 1), ("m/mnt", 6), ("m", 6)].iter().map(|(p, i)| (p.to_string(), *i)).collect();
                    assert_eq!(got, want, "FTS_XDEV (options {opts}): the mount point is returned, its contents are not");
                }
                let args = [CString::new("m").unwrap()];
                let av = argv(&args);
                let f = x::fts_open(av.as_ptr(), 16 | 4, Some(by_name));
                let mut inner = false;
                while let Some(e) = ent(x::fts_read(f)) {
                    inner |= e.path == "m/mnt/inner";
                }
                x::fts_close(f);
                assert!(inner, "without FTS_XDEV the other file system is entered");
                x::umount2(cpath(&m.join("mnt")).as_ptr(), 0);
            }
        }
    });
}

#[test]
fn s13_3_c_4_deep_paths() {
    // FTS-SHARED-PATH (ruling #615): one path buffer per stream, as fts(3) allows. Pin its
    // rules in a tree 40 levels of 32-byte names down, under a root given with a trailing
    // slash, in FTS_NOCHDIR and chdir mode:
    // * the entry fts_read returned is NUL-terminated at fts_pathlen, and its path is its
    //   parent's, one separator and its name;
    // * every ancestor (valid while the walk is inside it) shares the buffer and its first
    //   fts_pathlen bytes name it, also right after the buffer grew (the walk's paths reach
    //   ~1.4 KiB, past three doublings of the first 256 bytes, so the buffer moves several
    //   times and every ancestor must have been re-pointed);
    // * fts_accpath is fts_path with FTS_NOCHDIR, the entry's own name in chdir mode (roots:
    //   their path);
    // * fts_children's entries share the buffer, their fts_pathlen is their parent's, one and
    //   their name's (their bytes need not be there: fts(3) lets fts_read overwrite them);
    // * fts(3)'s initial fts_number 0 and fts_pointer NULL on every fresh entry.
    const LEVELS: usize = 40;
    let d = Scratch::new("c34deep");
    let mut rel = String::from("top");
    for i in 0..LEVELS {
        rel.push_str(&format!("/d{i:02}{}", "x".repeat(29)));
        d.dir(&rel);
        d.file(&format!("{rel}/f{i:02}"));
    }
    let args = [CString::new(format!("{}/top/", d.path.display())).unwrap()];
    let av = argv(&args);
    for opts in [16 | 4, 16] {
        let nochdir = opts & 4 != 0;
        let f = x::fts_open(av.as_ptr(), opts, Some(by_name));
        assert!(!f.is_null());
        let (mut pre, mut files, mut post, mut deepest, mut moves) = (0, 0, 0, 0usize, 0);
        let mut last_buf = 0usize;
        while let Some(e) = ent(x::fts_read(f)) {
            assert_eq!((usize::from(e.namelen), usize::from(e.pathlen)), (e.name.len(), e.path.len()), "{}", e.path);
            assert!(matches!(e.info, 1 | 6 | 8), "{}: fts_info {}", e.path, e.info);
            assert_eq!((e.number, e.pointer), (0, 0), "{}: fts_number and fts_pointer start at 0", e.path);
            if last_buf != 0 && e.path_at != last_buf {
                moves += 1;
            }
            last_buf = e.path_at;
            let acc_ok = if nochdir || e.level == 0 { e.acc_at == e.path_at } else { e.acc_at == e.addr + 112 };
            assert!(acc_ok, "options {opts}: {}: fts_accpath", e.path);
            match e.info {
                1 => pre += 1,
                6 => post += 1,
                _ => files += 1,
            }
            deepest = deepest.max(e.path.len());
            if e.level == 0 {
                assert_eq!(e.path, args[0].to_str().unwrap(), "the root's path is its argument");
            } else {
                let p = ent(core::ptr::with_exposed_provenance_mut(e.parent)).unwrap();
                let pp = &e.path[..usize::from(p.pathlen)];
                let sep = if pp.ends_with('/') { "" } else { "/" };
                assert_eq!(e.path, format!("{pp}{sep}{}", e.name), "options {opts}: one separator, then the name");
                let mut a = p;
                loop {
                    assert_eq!(a.path_at, e.path_at, "options {opts}: ancestor at level {} of {} shares the buffer", a.level, e.path);
                    let a_acc = if nochdir || a.level == 0 { a.path_at } else { a.addr + 112 };
                    assert_eq!(a.acc_at, a_acc, "options {opts}: ancestor's fts_accpath");
                    let own = &e.path[..usize::from(a.pathlen)];
                    assert!(own.ends_with(&a.name) || a.level == 0, "options {opts}: ancestor {own} of {}", e.path);
                    if a.level == 0 {
                        assert_eq!(own, args[0].to_str().unwrap());
                        break;
                    }
                    a = ent(core::ptr::with_exposed_provenance_mut(a.parent)).unwrap();
                }
            }
            if e.info == 1 && e.level % 7 == 0 {
                let mut c = ent(x::fts_children(f, 0));
                let mut n = 0;
                while let Some(k) = c {
                    assert_eq!(k.path_at, e.path_at, "options {opts}: fts_children entry {} shares the buffer", k.name);
                    let sep = usize::from(!e.path.ends_with('/'));
                    assert_eq!(usize::from(k.pathlen), e.path.len() + sep + k.name.len(), "options {opts}: fts_children entry {}", k.name);
                    n += 1;
                    c = ent(core::ptr::with_exposed_provenance_mut(k.link));
                }
                assert!(n >= 1, "options {opts}: {} has entries", e.path);
            }
        }
        assert_eq!((pre, files, post), (LEVELS + 1, LEVELS, LEVELS + 1), "options {opts}");
        assert_eq!(deepest, args[0].as_bytes().len() + LEVELS * 33 + 3, "options {opts}: the deepest file's path");
        assert!(moves >= 3, "options {opts}: the path buffer moved {moves} times (expected at least 3 growths)");
        assert_eq!(x::fts_close(f), 0);
    }
}

#[test]
fn s13_3_i_0() {
    // nopenfd does not bound the walk (D-13.4): a tree deeper than PATH_MAX is walked whole
    // with nopenfd 1, every callback's path naming its entry; the same with FTW_CHDIR (D-13.13)
    // and with fts in every mode.
    let d = Scratch::new("c3i0");
    build_deep(&d.path);
    let deep = d.c("deep");
    let check = |recs: &[Rec]| {
        assert_eq!(recs.len(), DEPTH + 2, "the root, {DEPTH} directories and the file");
        let pre = d.at("deep").as_os_str().len();
        for r in recs {
            let level = usize::try_from(r.level).unwrap();
            let want = if level <= DEPTH { pre + 201 * level } else { pre + 201 * DEPTH + 7 };
            assert_eq!(r.path.len(), want, "level {level}");
            assert_eq!(&r.path[usize::try_from(r.base).unwrap()..], if level == 0 { &b"deep"[..] } else if level <= DEPTH { &[b'd'; 200][..] } else { &b"bottom"[..] });
        }
        assert!(recs.iter().any(|r| r.path.len() > 4096), "paths over PATH_MAX");
    };
    take();
    errno::set_raw(0);
    assert_eq!(x::nftw(deep.as_ptr(), Some(record), 1, FTW_PHYS), 0);
    check(&take());
    // Out of descriptors part-way down (EMFILE from the kernel, injected): the walk closes the
    // shallowest levels' descriptors, reopens them component by component when it climbs back,
    // and still reports every entry.
    {
        use liella::fs::walk::{self, Action, Event, Options};
        let k = Faulty::new(CSys);
        assert!(k.fail("openat", Errno::EMFILE, 8, 3));
        let (mut n, mut longest) = (0usize, 0usize);
        let r = walk::walk(&k, d.at("deep").as_os_str().as_encoded_bytes(), Options { physical: true, ..Options::default() }, &mut |ev| {
            if let Event::Visit(v) = ev {
                n += 1;
                longest = longest.max(v.path.len());
            }
            Ok(Action::Continue)
        });
        assert_eq!(r, Ok(0));
        assert_eq!(k.injected(), 3, "the descriptor limit was hit");
        assert_eq!(n, DEPTH + 2, "every entry");
        assert!(longest > 4096);
    }
    in_child("s13_3_i_0", &d, |dir| {
        assert_eq!(x::chdir(cpath(dir).as_ptr()), 0);
        take();
        assert_eq!(x::nftw(c"deep".as_ptr(), Some(record), 20, FTW_PHYS | FTW_CHDIR), 0, "FTW_CHDIR over a deep tree (D-13.13)");
        assert_eq!(take().len(), DEPTH + 2);
        assert_eq!(kernel_cwd(), dir.as_os_str().as_encoded_bytes());
    });
    for opts in [16, 16 | 4, 2, 2 | 4] {
        let args = [deep.clone()];
        let av = argv(&args);
        let f = x::fts_open(av.as_ptr(), opts, None);
        let (mut n, mut maxlen, mut errs) = (0, 0, 0);
        while let Some(e) = ent(x::fts_read(f)) {
            n += 1;
            maxlen = maxlen.max(e.path.len());
            errs += i32::from(e.errno != 0);
        }
        assert_eq!((n, errs), (2 * DEPTH + 3, 0), "fts options {opts}: every entry, pre- and post-order");
        assert!(maxlen > 4096);
        x::fts_close(f);
    }
}

#[test]
fn s13_3_f_0() {
    // Failures are -1 with errno and no callback (missing root, a loop when links are
    // followed); entries that cannot be examined are FTW_NS / FTS_NS. The walkers never abort
    // (D-13.13): fts_open without paths is NULL with EINVAL, and fts_children after the end is
    // NULL with errno 0.
    let d = Scratch::new("c3f0");
    d.tree();
    let (r, e, recs) = walk(&d.c("missing"), 0);
    assert_eq!((r, e, recs.len()), (-1, ENOENT_, 0));
    let (r, e, _) = walk(&d.c("lp"), 0);
    assert_eq!((r, e), (-1, Errno::ELOOP.get()), "a self loop when links are followed");
    if !root() {
        let (_, _, recs) = walk(&d.c("t/nosearch"), 0);
        assert!(names(&recs, &d.path).iter().any(|(p, f)| p == "t/nosearch/ns_f" && *f == FTW_NS));
        let args = [d.c("t/nosearch")];
        let av = argv(&args);
        let f = x::fts_open(av.as_ptr(), 16 | 4, None);
        let mut ns = false;
        while let Some(e) = ent(x::fts_read(f)) {
            ns |= e.info == 10 && e.errno == Errno::EACCES.get();
        }
        assert!(ns, "FTS_NS with fts_errno");
        x::fts_close(f);
    }
    let none: [*mut i8; 1] = [null_mut()];
    errno::set_raw(0);
    assert!(x::fts_open(none.as_ptr(), 16, None).is_null());
    assert_eq!(errno::get(), Errno::EINVAL.get(), "no paths: EINVAL (D-13.13)");
    let args = [d.c("t/e")];
    let av = argv(&args);
    let f = x::fts_open(av.as_ptr(), 16 | 4, None);
    while !x::fts_read(f).is_null() {}
    errno::set_raw(Errno::EIO.get());
    assert!(x::fts_children(f, 0).is_null());
    assert_eq!(errno::get(), 0, "after the end: NULL with errno 0 (D-13.13)");
    errno::set_raw(0);
    assert_eq!(x::fts_set(f, null_mut(), 7), -1, "an invalid instruction");
    assert_eq!(errno::get(), Errno::EINVAL.get());
    x::fts_close(f);
}
#[test]
fn s13_4_c_1() {
    let d = Dir::new("c41");
    let a = d.file("a", b"data");
    assert_eq!(x::link(a.as_ptr(), d.at("b").as_ptr()), 0);
    assert_eq!(stat(&a).ino, stat(&d.at("b")).ino, "the same inode");
    assert_eq!(stat(&a).nlink, 2);
    std::fs::write(d.0.join("b"), b"changed").unwrap();
    assert_eq!(std::fs::read(d.0.join("a")).unwrap(), b"changed", "one file, two names");
}

#[test]
fn s13_4_c_2() {
    let d = Dir::new("c42");
    let l = d.at("l");
    assert_eq!(x::symlink(c"later".as_ptr(), l.as_ptr()), 0);
    let mut st = Stat::default();
    errno::set_raw(0);
    assert!(err(x::stat(l.as_ptr(), Some(&mut st)), Errno::ENOENT), "dangling: not resolved at creation");
    d.file("later", b"");
    assert_eq!(x::stat(l.as_ptr(), Some(&mut st)), 0, "resolved when used");
    assert_eq!(stat(&l).mode & S_IFMT, 0o120000);
}

#[test]
fn s13_4_c_3() {
    let d = Dir::new("c43");
    let l = d.at("l");
    std::os::unix::fs::symlink("abcdef", l.to_str().unwrap()).unwrap();
    let mut b = [b'*'; 6];
    assert_eq!(x::readlink(l.as_ptr(), b.as_mut_ptr().cast(), 6), 6, "fits exactly: no room for a terminator, none written");
    assert_eq!(&b, b"abcdef");
    let mut s = [b'*'; 4];
    assert_eq!(x::readlink(l.as_ptr(), s.as_mut_ptr().cast(), 3), 3, "truncated silently: the result equals n");
    assert_eq!(&s, b"abc*");
}

#[test]
fn s13_4_g_0() {
    // An observer never sees `new` missing while rename replaces it.
    let d = Dir::new("g40");
    let new = d.file("new", b"0");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let (stop, new) = (stop.clone(), new.clone());
        std::thread::spawn(move || {
            let mut missing = 0;
            let mut st = Stat::default();
            while !stop.load(Ordering::Relaxed) {
                if x::stat(new.as_ptr(), Some(&mut st)) != 0 {
                    missing += 1;
                }
            }
            missing
        })
    };
    for i in 0..2000 {
        let tmp = d.file("tmp", format!("{i}").as_bytes());
        assert_eq!(x::rename(tmp.as_ptr(), new.as_ptr()), 0);
    }
    stop.store(true, Ordering::Relaxed);
    assert_eq!(watcher.join().unwrap(), 0, "never neither");
}

#[test]
fn s13_4_f_0() {
    let d = Dir::new("f40");
    let a = d.file("a", b"");
    errno::set_raw(0);
    assert!(err(x::link(a.as_ptr(), a.as_ptr()), Errno::EEXIST), "EEXIST");
    std::fs::create_dir_all(d.0.join("full/x")).unwrap();
    std::fs::create_dir(d.0.join("empty")).unwrap();
    assert!(err(x::rename(d.at("empty").as_ptr(), d.at("full").as_ptr()), Errno::ENOTEMPTY) || errno::get() == Errno::EEXIST.get(), "ENOTEMPTY");
    assert!(err(x::rename(a.as_ptr(), d.at("empty").as_ptr()), Errno::EISDIR), "EISDIR");
    std::os::unix::fs::symlink("loop", d.0.join("loop")).unwrap();
    assert!(err(x::unlink(d.at("loop/x").as_ptr()), Errno::ELOOP), "ELOOP");
    // EXDEV: /proc is another file system.
    assert!(err(x::link(c"/proc/self/status".as_ptr(), d.at("x").as_ptr()), Errno::EXDEV) || errno::get() == Errno::EPERM.get(), "EXDEV");
    assert!(err(x::rename(c"/proc/self/status".as_ptr(), d.at("y").as_ptr()), Errno::EXDEV) || errno::get() == Errno::EACCES.get() || errno::get() == Errno::EPERM.get());
}

#[test]
fn s13_5_c_0() {
    alone("s13_5_c_0", || {
        x::umask(0o027);
        let d = Dir::new("c50");
        let ok_dir = x::mkdir(d.at("dir").as_ptr(), 0o777) == 0 && stat(&d.at("dir")).mode & 0o7777 == 0o750;
        let ok_fifo = x::mkfifo(d.at("fifo").as_ptr(), 0o666) == 0 && stat(&d.at("fifo")).mode & (S_IFMT | 0o777) == 0o010640;
        let ok_reg = x::mknod(d.at("reg").as_ptr(), 0o100666, 0) == 0 && stat(&d.at("reg")).mode & (S_IFMT | 0o777) == 0o100640;
        let ok_sock = x::mknod(d.at("sock").as_ptr(), 0o140600, 0) == 0 && stat(&d.at("sock")).mode & S_IFMT == 0o140000;
        let dev = x::gnu_dev_makedev(259, 70000);
        let ok_dev = (x::gnu_dev_major(dev), x::gnu_dev_minor(dev)) == (259, 70000);
        ok_dir && ok_fifo && ok_reg && ok_sock && ok_dev
    });
}

#[test]
fn s13_6_c_0() {
    let d = Dir::new("c60");
    let f = d.file("f", &[7u8; 5000]);
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let (mut a, mut b) = (Stat::default(), Stat::default());
    assert_eq!(x::stat(f.as_ptr(), Some(&mut a)), 0);
    assert_eq!(x::fstat(fd, Some(&mut b)), 0);
    assert_eq!(a, b, "the same inode's metadata by path and by descriptor");
    io::close(fd);
}

#[test]
fn s13_6_c_1() {
    let d = Dir::new("c61");
    let f = d.file("f", &[7u8; 5000]);
    let st = stat(&f);
    let m = std::fs::metadata(f.to_str().unwrap()).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!((st.mode, st.ino, st.dev, st.nlink), (m.mode(), m.ino(), m.dev(), m.nlink()));
    assert_eq!((st.uid, st.gid, st.rdev, st.size), (m.uid(), m.gid(), m.rdev(), 5000));
    assert_eq!((st.blksize, st.blocks), (m.blksize() as i64, m.blocks() as i64));
    let mut null = Stat::default();
    x::stat(c"/dev/null".as_ptr(), Some(&mut null));
    assert_eq!((null.mode & S_IFMT, x::gnu_dev_major(null.rdev), x::gnu_dev_minor(null.rdev)), (0o020000, 1, 3), "st_rdev of a device");
}

#[test]
fn s13_6_c_2() {
    let d = Dir::new("c62");
    let f = d.file("f", b"");
    let ts = [Timespec { sec: 100, nsec: 1 }, Timespec { sec: 200, nsec: 999_999_999 }];
    x::utimensat(AT_FDCWD, f.as_ptr(), Some(&ts), 0);
    let st = stat(&f);
    assert_eq!((st.atime, st.mtime), (ts[0], ts[1]), "nanosecond timespecs");
    assert!(st.ctime.sec > 200);
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    let mut s = Stat::default();
    assert_eq!(x::fstatat(dir, c"l".as_ptr(), Some(&mut s), AT_SYMLINK_NOFOLLOW), 0);
    assert_eq!(s.mode & S_IFMT, 0o120000, "AT_SYMLINK_NOFOLLOW");
    assert_eq!(x::fstatat(dir, c"".as_ptr(), Some(&mut s), AT_EMPTY_PATH), 0);
    assert_eq!(s.mode & S_IFMT, 0o040000, "AT_EMPTY_PATH");
    assert_eq!(x::fstatat(dir, c"f".as_ptr(), Some(&mut s), 0x800), 0, "AT_NO_AUTOMOUNT");
    io::close(dir);
}

#[test]
fn s13_6_f_0() {
    let d = Dir::new("f60");
    let f = d.file("f", b"");
    let mut st = Stat::default();
    errno::set_raw(0);
    assert!(err(x::stat(d.at("none").as_ptr(), Some(&mut st)), Errno::ENOENT));
    assert!(err(x::stat(d.at("f/x").as_ptr(), Some(&mut st)), Errno::ENOTDIR));
    std::os::unix::fs::symlink("loop", d.0.join("loop")).unwrap();
    assert!(err(x::stat(d.at("loop").as_ptr(), Some(&mut st)), Errno::ELOOP));
    // EOVERFLOW concerns 32-bit structures; x86-64's are 64-bit throughout, so a large size
    // is reported, not refused.
    let fd = io::liella_open3(f.as_ptr(), O_RDWR, 0);
    assert_eq!(x::ftruncate(fd, 1 << 40), 0);
    assert_eq!(x::fstat(fd, Some(&mut st)), 0);
    assert_eq!(st.size, 1 << 40);
    io::close(fd);
}

#[test]
fn s13_7_c_1() {
    let d = Dir::new("c71");
    let f = d.file("f", b"");
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    assert_eq!(x::chmod(d.at("l").as_ptr(), 0o640), 0, "chmod follows");
    assert_eq!(stat(&f).mode & 0o777, 0o640);
    errno::set_raw(0);
    assert!(err(x::lchmod(d.at("l").as_ptr(), 0o600), Errno::EOPNOTSUPP), "a symlink has no mode");
    assert_eq!(x::lchmod(f.as_ptr(), 0o604), 0, "lchmod of a non-link changes it");
    assert_eq!(stat(&f).mode & 0o777, 0o604);
    // Both paths: fchmodat2 where the kernel has it (injected success), else the O_PATH +
    // /proc/self/fd fallback (injected ENOSYS, as on kernels before 6.6).
    let k = Faulty::new(CSys);
    assert!(k.fail("fchmodat2", Errno::ENOSYS, 0, 2));
    fs_compat::fchmodat(&k, Fd::CWD, k_(&f), 0o611, 0x100).unwrap();
    assert_eq!(stat(&f).mode & 0o777, 0o611, "the fallback changed the file");
    assert_eq!(fs_compat::fchmodat(&k, Fd::CWD, k_(&d.at("l")), 0o600, 0x100), Err(Errno::EOPNOTSUPP), "and refused the link");
    assert_eq!(k.injected(), 2);
}

#[test]
fn s13_7_c_2() {
    let d = Dir::new("c72");
    let f = d.file("f", b"");
    let st = stat(&f);
    assert_eq!(x::chown(f.as_ptr(), u32::MAX, st.gid), 0, "-1: uid unchanged");
    assert_eq!(x::chown(f.as_ptr(), st.uid, u32::MAX), 0, "-1: gid unchanged");
    let after = stat(&f);
    assert_eq!((after.uid, after.gid), (st.uid, st.gid));
    // A supplementary group the caller is in: allowed without privilege.
    let mut groups = [0u32; 64];
    let n = CSys.getgroups(&mut groups).unwrap();
    if let Some(&g) = groups[..n].iter().find(|&&g| g != st.gid) {
        assert_eq!(x::chown(f.as_ptr(), u32::MAX, g), 0);
        assert_eq!(stat(&f).gid, g);
    }
}

#[test]
fn s13_7_c_3() {
    alone("s13_7_c_3", || {
        let first = x::umask(0o022);
        let a = x::umask(0o077);
        let b = x::umask(first);
        // Reading it without changing it: set twice (racy) or /proc/self/status.
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let shown = status.lines().find(|l| l.starts_with("Umask:")).map(|l| u32::from_str_radix(l[6..].trim(), 8).unwrap());
        a == 0o022 && b == 0o077 && shown == Some(first)
    });
}

#[test]
fn s13_7_c_4() {
    // Real versus effective IDs: the same for this process, so access and euidaccess agree;
    // the emulation (no faccessat2) computes the same answer from the mode bits.
    let d = Dir::new("c74");
    let f = d.file("f", b"");
    std::fs::set_permissions(f.to_str().unwrap(), std::os::unix::fs::PermissionsExt::from_mode(0o540)).unwrap();
    for mode in [0, 1, 2, 4, 5, 6, 7] {
        let a = x::access(f.as_ptr(), mode);
        let e = x::euidaccess(f.as_ptr(), mode);
        let at = x::faccessat(AT_FDCWD, f.as_ptr(), mode, AT_EACCESS);
        assert_eq!((a, e), (at, at), "mode {mode}");
        let k = Faulty::new(CSys);
        k.fail("faccessat2", Errno::ENOSYS, 0, 1);
        let emulated = fs_compat::faccessat(&k, Fd::CWD, k_(&f), mode as u32, 0x200);
        assert_eq!(emulated.is_ok(), at == 0, "the emulation agrees for mode {mode}");
        assert_eq!(k.injected(), 1);
    }
}

/// The `realpath` fixture: `rp/a/f`, `rp/l_rel -> a`, `rp/l_abs -> <abs>/rp/a`, `rp/l_file ->
/// a/f`, `rp/l_dots -> a/../a/./f`, `rp/l_up -> ..`, `rp/l_dang -> missing`, `rp/l_loop ->
/// l_loop`, `rp/l_ping <-> rp/l_pong`, and for a non-root user `rp/ns/` (0600) holding `f`.
fn rp_fixture(d: &Scratch) {
    d.dir("rp/a");
    d.file("rp/a/f");
    d.link("a", "rp/l_rel");
    d.link(&d.at("rp/a").display().to_string(), "rp/l_abs");
    d.link("a/f", "rp/l_file");
    d.link("a/../a/./f", "rp/l_dots");
    d.link("..", "rp/l_up");
    d.link("missing", "rp/l_dang");
    d.link("l_loop", "rp/l_loop");
    d.link("l_pong", "rp/l_ping");
    d.link("l_ping", "rp/l_pong");
    d.dir("rp/ns/d");
    d.file("rp/ns/f");
    d.mode("rp/ns", 0o600);
}

/// `realpath(path, buf)` with a pre-filled PATH_MAX buffer: the result (or `Err((errno,
/// buffer text or None if untouched))`).
fn rp(path: &CString) -> Result<Vec<u8>, (i32, Option<Vec<u8>>)> {
    let mut b = vec![b'Q' as i8; 4096];
    b[4095] = 0;
    errno::set_raw(0);
    let r = x::realpath(path.as_ptr(), b.as_mut_ptr());
    if r.is_null() {
        let e = errno::get();
        return Err((e, (b[0] != b'Q' as i8).then(|| ctext(b.as_ptr()))));
    }
    assert_eq!(r, b.as_mut_ptr(), "the caller's buffer");
    Ok(ctext(r))
}

#[test]
fn s13_8_c_0() {
    // The result is absolute, has no ".", ".." or symbolic-link components, and names the same
    // file.
    let d = Scratch::new("c80");
    rp_fixture(&d);
    let f = d.at("rp/a/f").as_os_str().as_encoded_bytes().to_vec();
    let a = d.at("rp/a").as_os_str().as_encoded_bytes().to_vec();
    for (input, want) in [("rp/./a//f", &f), ("rp/a/../a/f", &f), ("rp/l_rel/f", &f), ("rp/l_abs/f", &f), ("rp/l_file", &f), ("rp/l_dots", &f), ("rp/l_up/rp/a", &a), ("rp/a/", &a), ("rp/a/.", &a)] {
        assert_eq!(rp(&d.c(input)), Ok(want.clone()), "{input}");
    }
    for root in ["/", "//", "///", "/..", "/../..", "//."] {
        assert_eq!(rp(&CString::new(root).unwrap()), Ok(b"/".to_vec()), "{root}");
    }
    assert_eq!(rp(&CString::new(".").unwrap()), Ok(kernel_cwd()), "relative to the working directory");
    let r = rp(&d.c("rp/l_dots")).unwrap();
    let mut prefix = std::path::PathBuf::from("/");
    for c in std::path::Path::new(&String::from_utf8(r.clone()).unwrap()).components().skip(1) {
        prefix.push(c);
        assert_ne!(lstat(&prefix).mode & S_IFMT, S_IFLNK_, "no link in {}", prefix.display());
    }
    assert_eq!(lstat(std::path::Path::new(&String::from_utf8(r).unwrap())).ino, lstat(&d.at("rp/a/f")).ino, "the same file");
    // Deep trees (REALPATH-DEPTH, the bench's `deep` profile): 64 levels of 32-byte names (over
    // 2 KiB, every lookup name ever longer), then `.`, `..`, doubled slashes and a relative link.
    let mut deep = String::from("rp/a");
    for k in 0..64 {
        deep.push_str(&format!("/stress-deep-level-{k:03}-abcdefghij"));
    }
    d.dir(&format!("{deep}/sub"));
    d.file(&format!("{deep}/f"));
    d.link("../f", &format!("{deep}/sub/l"));
    let want = d.at(&format!("{deep}/f")).as_os_str().as_encoded_bytes().to_vec();
    assert!(want.len() > 2048);
    for input in [format!("{deep}/f"), format!("{deep}/sub/l"), format!("{deep}/./sub//../sub/l"), format!("rp/l_rel/{}/f", &deep[5..])] {
        assert_eq!(rp(&d.c(&input)), Ok(want.clone()), "deep {}", &input[input.len() - 20..]);
    }
    // The Rust entry checks the path for a NUL once (the C forms cannot pass one): EINVAL, no prefix.
    for bad in [&b"/\0tmp"[..], b"/tmp/\0", b"rp\0/a"] {
        assert!(matches!(liella::fs::canon::canonicalize(&CSys, bad), Err(e) if e.errno == Errno::EINVAL && e.prefix.is_none()), "{bad:?}");
    }
}

#[test]
fn s13_8_c_1() {
    // realpath writes the caller's PATH_MAX buffer and returns it, or allocates for NULL; the
    // allocated form has no length limit (D-13.5) while a buffer cannot hold more than PATH_MAX.
    let d = Scratch::new("c81");
    rp_fixture(&d);
    let want = d.at("rp/a/f").as_os_str().as_encoded_bytes().to_vec();
    assert_eq!(rp(&d.c("rp/l_rel/f")), Ok(want.clone()));
    assert_eq!(ctext(x::realpath(d.c("rp/l_rel/f").as_ptr(), null_mut())), want, "allocated");
    let mut chk = vec![0i8; 4096];
    assert_eq!(ctext(x::__realpath_chk(d.c("rp/l_rel/f").as_ptr(), chk.as_mut_ptr(), 4096)), want, "the fortified form");
    build_deep(&d.path);
    let mut long = d.at("deep").as_os_str().as_encoded_bytes().to_vec();
    for _ in 0..DEPTH {
        long.push(b'/');
        long.extend_from_slice(&[b'd'; 200]);
    }
    long.extend_from_slice(b"/bottom");
    let long = CString::new(long).unwrap();
    assert_eq!(rp(&long), Err((Errno::ENAMETOOLONG.get(), None)), "the result does not fit the buffer: untouched");
    let full = ctext(x::realpath(long.as_ptr(), null_mut()));
    assert_eq!(full, long.as_bytes(), "allocated: any length (D-13.5)");
    assert!(full.len() > 4096);
}

#[test]
fn s13_8_c_2() {
    // canonicalize_file_name always allocates: realpath(path, NULL); a NULL path is EINVAL.
    let d = Scratch::new("c82");
    rp_fixture(&d);
    for p in ["rp/l_dots", "rp/a", "rp/l_up"] {
        assert_eq!(ctext(x::canonicalize_file_name(d.c(p).as_ptr())), ctext(x::realpath(d.c(p).as_ptr(), null_mut())), "{p}");
    }
    errno::set_raw(0);
    assert!(x::canonicalize_file_name(core::ptr::null()).is_null() && errno::get() == Errno::EINVAL.get());
    errno::set_raw(0);
    assert!(x::realpath(core::ptr::null(), null_mut()).is_null() && errno::get() == Errno::EINVAL.get());
    errno::set_raw(0);
    assert!(x::canonicalize_file_name(d.c("rp/missing").as_ptr()).is_null() && errno::get() == ENOENT_);
}

#[test]
fn s13_8_f_0() {
    // ENOENT and EACCES leave the prefix resolved so far, including the component that failed;
    // ELOOP, ENAMETOOLONG and ENOTDIR leave the buffer untouched. Up to 40 links resolve in one
    // call (the kernel's limit); 41 are ELOOP.
    let d = Scratch::new("c8f0");
    rp_fixture(&d);
    let at = |p: &str| d.at(p).as_os_str().as_encoded_bytes().to_vec();
    let enoent = |p: &str, pre: &str| assert_eq!(rp(&d.c(p)), Err((ENOENT_, Some(at(pre)))), "{p}");
    enoent("rp/missing", "rp/missing");
    enoent("rp/missing/x", "rp/missing");
    enoent("rp/missing/../a", "rp/missing");
    enoent("rp/a/missing", "rp/a/missing");
    enoent("rp/l_dang", "rp/missing");
    for p in ["rp/l_loop", "rp/l_loop/x", "rp/l_ping"] {
        assert_eq!(rp(&d.c(p)), Err((Errno::ELOOP.get(), None)), "{p}");
    }
    for p in ["rp/a/f/", "rp/a/f/.", "rp/a/f/..", "rp/a/f/x", "rp/l_file/"] {
        assert_eq!(rp(&d.c(p)), Err((Errno::ENOTDIR.get(), None)), "{p}");
    }
    assert_eq!(rp(&d.c(&format!("rp/{}", "n".repeat(256)))), Err((Errno::ENAMETOOLONG.get(), None)));
    assert_eq!(rp(&d.c(&format!("rp/{}", "n".repeat(255)))), Err((ENOENT_, Some(at(&format!("rp/{}", "n".repeat(255)))))));
    assert_eq!(rp(&CString::new("").unwrap()), Err((ENOENT_, None)), "the empty path");
    if !root() {
        assert_eq!(rp(&d.c("rp/ns/f")), Err((Errno::EACCES.get(), Some(at("rp/ns/f")))));
        assert_eq!(rp(&d.c("rp/ns/d/..")), Err((Errno::EACCES.get(), Some(at("rp/ns/d")))));
        assert_eq!(rp(&d.c("rp/ns/..")), Ok(at("rp")), ".. after an unsearchable directory is not examined");
    }
    d.dir("rp/ch");
    d.link("../a/f", "rp/ch/l0");
    for k in 1..=41 {
        d.link(&format!("l{}", k - 1), &format!("rp/ch/l{k}"));
    }
    assert_eq!(rp(&d.c("rp/ch/l39")), Ok(at("rp/a/f")), "40 links");
    assert_eq!(rp(&d.c("rp/ch/l40")), Err((Errno::ELOOP.get(), None)), "41 links");
    d.dir("rp/hop");
    d.link(".", "rp/hop/h");
    assert_eq!(rp(&d.c(&format!("rp/hop{}", "/h".repeat(40)))), Ok(at("rp/hop")), "40 separate links in one path");
    assert_eq!(rp(&d.c(&format!("rp/hop{}", "/h".repeat(41)))), Err((Errno::ELOOP.get(), None)), "the count is per call");
}

/// Run a temporary-name call on a fresh copy of `template`: the result and the template after.
fn tmpl(template: &[u8], f: impl FnOnce(*mut i8) -> c_int) -> (c_int, i32, Vec<u8>) {
    let mut t: Vec<i8> = template.iter().map(|&b| b as i8).collect();
    t.push(0);
    errno::set_raw(0);
    let r = f(t.as_mut_ptr());
    (r, errno::get(), ctext(t.as_ptr()))
}

/// The positions where `after` differs from `before` all hold letters or digits, and there
/// are `n` of them, at the end of the text before `suffix` bytes.
fn replaced(before: &[u8], after: &[u8], suffix: usize) -> bool {
    let n = before.len();
    after.len() == n
        && after[..n - suffix - 6] == before[..n - suffix - 6]
        && after[n - suffix..] == before[n - suffix..]
        && after[n - suffix - 6..n - suffix].iter().all(|c| c.is_ascii_alphanumeric())
}

#[test]
fn s13_9_c_1() {
    // mkstemp replaces the trailing XXXXXX with letters and digits, creates the file
    // exclusively with mode 0600 (masked by the umask), open for reading and writing, and
    // returns its descriptor; repeated calls give distinct files.
    use liella_kernel::ops::FcntlInt;
    let d = Scratch::new("c91");
    let t = d.at("aXXXXXX").as_os_str().as_encoded_bytes().to_vec();
    let (fd, _, after) = tmpl(&t, |p| x::mkstemp(p));
    assert!(fd >= 0);
    assert!(replaced(&t, &after, 0), "{}", String::from_utf8_lossy(&after));
    let st = lstat(std::path::Path::new(&String::from_utf8(after.clone()).unwrap()));
    let fst = CSys.fstat(Fd::new(fd)).unwrap();
    assert_eq!((st.ino, st.mode & S_IFMT, st.size), (fst.ino, S_IFREG_, 0), "the name is the descriptor's new, empty file");
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap() & 3, 2, "O_RDWR");
    io::close(fd);
    let mut names = std::collections::HashSet::new();
    for _ in 0..20 {
        let (fd, _, after) = tmpl(d.at("qXXXXXX").as_os_str().as_encoded_bytes(), |p| x::mkstemp(p));
        assert!(fd >= 0);
        io::close(fd);
        assert!(names.insert(after), "distinct");
    }
    // O_EXCL: a name the kernel reports as existing (EEXIST, which only an exclusive create
    // gives) is never opened; another name is tried, and only the last one is created.
    {
        use liella::fs::temp::{self, Create, Made};
        let k = Faulty::new(CSys);
        assert!(k.fail("openat", Errno::EEXIST, 0, 3));
        let before = std::fs::read_dir(&d.path).unwrap().count();
        let mut t = z(d.at("xXXXXXX").as_os_str().as_encoded_bytes());
        let made = temp::make(&k, Fd::CWD, &mut t, 0, Create::File(0));
        assert!(matches!(made, Ok(Made::File(_))), "created after three names were taken: {made:?}");
        assert_eq!(k.injected(), 3);
        if let Ok(Made::File(fd)) = made {
            CSys.close(fd).unwrap();
        }
        assert_eq!(std::fs::read_dir(&d.path).unwrap().count(), before + 1, "one file: no name reported as existing was created");
    }
    in_child("s13_9_c_1", &d, |dir| {
        for (mask, mode) in [(0o000, 0o600), (0o077, 0o600), (0o277, 0o400)] {
            x::umask(mask);
            let (fd, _, after) = tmpl(dir.join("uXXXXXX").as_os_str().as_encoded_bytes(), |p| x::mkstemp(p));
            assert!(fd >= 0);
            assert_eq!(lstat(std::path::Path::new(&String::from_utf8(after).unwrap())).mode & 0o7777, mode, "umask {mask:o}");
            io::close(fd);
        }
    });
}

#[test]
fn s13_9_c_2() {
    // The o forms add open flags (O_CLOEXEC, O_APPEND, O_SYNC); the file is read-write whatever
    // access mode is passed.
    use liella_kernel::ops::FcntlInt;
    let d = Scratch::new("c92");
    let t = d.at("oXXXXXX").as_os_str().as_encoded_bytes().to_vec();
    let (fd, _, _) = tmpl(&t, |p| x::mkostemp(p, 0o2000000 | 0o2000 | 0o4010000));
    assert!(fd >= 0);
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(1), "O_CLOEXEC");
    let fl = CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap();
    assert!(fl & 0o2000 != 0 && fl & 0o4010000 == 0o4010000, "O_APPEND and O_SYNC");
    io::close(fd);
    let (fd, _, _) = tmpl(&t, |p| x::mkostemp(p, 1));
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap() & 3, 2, "O_WRONLY asked, O_RDWR given");
    io::close(fd);
    let (fd, _, after) = tmpl(d.at("pXXXXXX.s").as_os_str().as_encoded_bytes(), |p| x::mkostemps(p, 2, 0o2000000));
    assert!(fd >= 0 && after.ends_with(b".s"));
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(1));
    io::close(fd);
    // Flags that cannot make a new regular file (as the reference, fstree.glibc.txt): O_DIRECTORY
    // and O_TMPFILE are EINVAL, O_PATH ENOENT, each after a name was tried; O_TRUNC,
    // O_CREAT|O_EXCL and an unknown bit change nothing visible. The flags pass to the kernel
    // (⚑ KERN): before Linux 6.4, O_CREAT|O_EXCL|O_DIRECTORY creates the regular file and then
    // fails with ENOTDIR (Linux 6.4 made it EINVAL with nothing created).
    let before = std::fs::read_dir(&d.path).unwrap().count();
    let directory = if kernel::before(6, 4) {
        kernel::note("EINVAL for O_CREAT|O_DIRECTORY", "6.4");
        Errno::ENOTDIR
    } else {
        Errno::EINVAL
    };
    for (flags, e) in [(0o200000, directory), (0o20200000, Errno::EINVAL), (0o10000000, Errno::ENOENT)] {
        let (r, en, after) = tmpl(&t, |p| x::mkostemp(p, flags));
        assert_eq!((r, en), (-1, e.get()), "flags {flags:o}");
        assert!(replaced(&t, &after, 0), "flags {flags:o}: the template holds the name tried");
    }
    let left = if kernel::before(6, 4) { 1 } else { 0 };
    assert_eq!(std::fs::read_dir(&d.path).unwrap().count(), before + left, "the failures created nothing (but the old kernel's file)");
    for flags in [0o1000, 0o300, 0x4000_0000] {
        let (fd, _, after) = tmpl(&t, |p| x::mkostemp(p, flags));
        assert!(fd >= 0, "flags {flags:o}");
        let st = CSys.fstat(Fd::new(fd)).unwrap();
        assert_eq!((st.mode & 0o077, st.mode & S_IFMT, st.size), (0, S_IFREG_, 0), "flags {flags:o}");
        assert_eq!(lstat(std::path::Path::new(&String::from_utf8(after).unwrap())).ino, st.ino);
        io::close(fd);
    }
}

#[test]
fn s13_9_c_3() {
    // The s forms keep a suffix of the given length after the XXXXXX; any other length is
    // EINVAL with the template unchanged.
    let d = Scratch::new("c93");
    let t = d.at("bXXXXXX.txt").as_os_str().as_encoded_bytes().to_vec();
    let (fd, _, after) = tmpl(&t, |p| x::mkstemps(p, 4));
    assert!(fd >= 0);
    assert!(replaced(&t, &after, 4));
    io::close(fd);
    for s in [5, 3, -1, 100, 0] {
        assert_eq!(tmpl(&t, |p| x::mkstemps(p, s)), (-1, Errno::EINVAL.get(), t.clone()), "suffix length {s}");
    }
    let t2 = d.at("bXXXXXXXX").as_os_str().as_encoded_bytes().to_vec();
    let (fd, _, after) = tmpl(&t2, |p| x::mkstemps(p, 2));
    assert!(fd >= 0 && after.ends_with(b"XX") && replaced(&t2, &after, 2), "a suffix of X is kept");
    io::close(fd);
}

#[test]
fn s13_9_c_4() {
    // mkdtemp creates a directory with mode 0700 (masked by the umask) and returns its template.
    let d = Scratch::new("c94");
    let t = d.at("dXXXXXX").as_os_str().as_encoded_bytes().to_vec();
    let mut buf: Vec<i8> = t.iter().map(|&b| b as i8).collect();
    buf.push(0);
    let r = x::mkdtemp(buf.as_mut_ptr());
    assert_eq!(r, buf.as_mut_ptr());
    let name = ctext(r);
    assert!(replaced(&t, &name, 0));
    assert_eq!(lstat(std::path::Path::new(&String::from_utf8(name).unwrap())).mode & S_IFMT, S_IFDIR_);
    in_child("s13_9_c_4", &d, |dir| {
        for (mask, mode) in [(0o000, 0o700), (0o277, 0o500)] {
            x::umask(mask);
            let mut b: Vec<i8> = dir.join("vXXXXXX").as_os_str().as_encoded_bytes().iter().map(|&c| c as i8).collect();
            b.push(0);
            let r = x::mkdtemp(b.as_mut_ptr());
            assert!(!r.is_null());
            assert_eq!(lstat(std::path::Path::new(&String::from_utf8(ctext(r)).unwrap())).mode & 0o7777, mode, "umask {mask:o}");
        }
    });
}

#[test]
fn s13_9_c_5() {
    // mktemp only names: the name does not exist and nothing is created; it returns its
    // template, emptied (with errno) when no name can be made.
    let d = Scratch::new("c95");
    d.file("file");
    let t = d.at("gXXXXXX").as_os_str().as_encoded_bytes().to_vec();
    let mut buf: Vec<i8> = t.iter().map(|&b| b as i8).collect();
    buf.push(0);
    let r = x::mktemp(buf.as_mut_ptr());
    assert_eq!(r, buf.as_mut_ptr());
    let name = ctext(r);
    assert!(replaced(&t, &name, 0));
    assert!(!std::path::Path::new(&String::from_utf8(name).unwrap()).exists(), "nothing created");
    assert_eq!(std::fs::read_dir(&d.path).unwrap().count(), 1, "only the fixture's file");
    for (bad, e) in [(d.at("gXXXXX"), Errno::EINVAL), (d.at("file/gXXXXXX"), Errno::ENOTDIR)] {
        let mut b: Vec<i8> = bad.as_os_str().as_encoded_bytes().iter().map(|&c| c as i8).collect();
        b.push(0);
        errno::set_raw(0);
        assert_eq!(x::mktemp(b.as_mut_ptr()), b.as_mut_ptr());
        assert_eq!((b[0], errno::get()), (0, e.get()), "{}: the template is emptied", bad.display());
    }
}

#[test]
fn s13_9_i_0() {
    // Exactly six X before the suffix: fewer, none, lowercase, a character after them, a
    // trailing slash or an empty template are EINVAL (template unchanged); with more than six
    // only the last six are replaced.
    let d = Scratch::new("c9i0");
    for bad in ["aXXXXX", "a", "axxxxxx", "aXXXXXXb", "aXXXXXX/"] {
        let t = d.at(bad).as_os_str().as_encoded_bytes().to_vec();
        assert_eq!(tmpl(&t, |p| x::mkstemp(p)), (-1, Errno::EINVAL.get(), t.clone()), "{bad}");
        assert_eq!(tmpl(&t, |p| c_int::from(x::mkdtemp(p).is_null()).wrapping_neg()), (-1, Errno::EINVAL.get(), t.clone()), "mkdtemp {bad}");
    }
    assert_eq!(tmpl(b"", |p| x::mkstemp(p)), (-1, Errno::EINVAL.get(), Vec::new()));
    let t = d.at("aXXXXXXXX").as_os_str().as_encoded_bytes().to_vec();
    let (fd, _, after) = tmpl(&t, |p| x::mkstemp(p));
    assert!(fd >= 0);
    assert!(replaced(&t, &after, 0), "the last six of eight replaced, the two before kept");
    io::close(fd);
}

#[test]
fn s13_9_f_0() {
    // EINVAL for a bad template; the open (mkdir) errors, the template then holding the name
    // tried; EEXIST once every attempt found its name taken: ATTEMPTS (62^3 = 238,328) attempts,
    // provoked with a kernel that says EEXIST (Faulty), after which nothing was created.
    use liella::fs::temp::{self, Create, Made};
    let d = Scratch::new("c9f0");
    d.file("file");
    for (p, e) in [("missing/aXXXXXX", Errno::ENOENT), ("file/aXXXXXX", Errno::ENOTDIR)] {
        let t = d.at(p).as_os_str().as_encoded_bytes().to_vec();
        let (r, en, after) = tmpl(&t, |p| x::mkstemp(p));
        assert_eq!((r, en), (-1, e.get()), "{p}");
        assert!(replaced(&t, &after, 0), "the template holds the name tried");
    }
    assert_eq!(temp::ATTEMPTS, 238_328);
    for (method, how) in [("openat", Create::File(0)), ("mkdirat", Create::Dir)] {
        let k = Faulty::new(CSys);
        assert!(k.fail(method, Errno::EEXIST, 0, temp::ATTEMPTS));
        let mut t = z(d.at("eXXXXXX").as_os_str().as_encoded_bytes());
        let before = t.clone();
        assert_eq!(temp::make(&k, Fd::CWD, &mut t, 0, how), Err(Errno::EEXIST), "{method}: exhausted");
        assert_eq!(k.injected(), temp::ATTEMPTS, "{method}: every attempt made");
        assert!(replaced(&before[..before.len() - 1], &t[..t.len() - 1], 0), "the last name tried is left in the template");
        assert_eq!(std::fs::read_dir(&d.path).unwrap().count(), 1, "nothing created");
        let mut fresh = before.clone();
        let r = temp::make(&k, Fd::CWD, &mut fresh, 0, how);
        assert!(r.is_ok(), "{method}: after the faults the next call succeeds: {r:?}");
        let made = std::path::PathBuf::from(String::from_utf8(fresh[..fresh.len() - 1].to_vec()).unwrap());
        if let Ok(Made::File(fd)) = r {
            CSys.close(fd).unwrap();
            std::fs::remove_file(made).unwrap();
        } else {
            std::fs::remove_dir(made).unwrap();
        }
    }
}
#[test]
fn s13_10_c_1() {
    let mut b = Statfs::default();
    assert_eq!(x::statfs(c"/tmp".as_ptr(), Some(&mut b)), 0);
    let mine = std::process::Command::new("stat").args(["-f", "-c", "%t %S %b %f %a %c %d %l %i", "/tmp"]).output();
    if let Ok(o) = mine {
        let v: Vec<String> = String::from_utf8_lossy(&o.stdout).split_whitespace().map(String::from).collect();
        if v.len() == 9 {
            assert_eq!(i64::from_str_radix(&v[0], 16).unwrap(), b.ftype, "type magic");
            assert_eq!(v[1].parse::<i64>().unwrap(), b.frsize, "fragment size");
            assert_eq!(v[2].parse::<u64>().unwrap(), b.blocks, "blocks");
            assert_eq!(v[7].parse::<i64>().unwrap(), b.namelen, "name length");
        }
    }
    assert!(b.bfree >= b.bavail && b.files >= b.ffree, "free and available blocks, files and free files");
    assert_ne!(b.flags & 0x20, 0, "mount flags present (ST_VALID)");
}

#[test]
fn s13_10_c_2() {
    let mut f = Statfs::default();
    x::statfs(c"/proc".as_ptr(), Some(&mut f));
    let v = fs_compat::statvfs(&f);
    assert_eq!(v.f_flag, (f.flags & !0x20) as u64, "f_flag from statfs.f_flags");
    assert_ne!(v.f_flag & (0x2 | 0x4 | 0x8), 0, "/proc: nosuid, nodev, noexec");
    assert_eq!(u64::from(v.f_type), f.ftype as u64, "f_type (2.39)");
    // Derived without a /proc/mounts scan (D-13.8): the same answer from an injected statfs.
    let mut fake = Statfs::default();
    fake.flags = 0x20 | 0x1;
    fake.fsid = [-2, 7];
    let v = fs_compat::statvfs(&fake);
    assert_eq!(v.f_flag, 1, "ST_RDONLY");
    assert_eq!(v.f_fsid, 0xffff_ffff_ffff_fffe | (7 << 32), "glibc's packing: the low half sign-extended");
}

#[test]
fn s13_11_c_0() {
    let d = Dir::new("c110");
    let f = d.file("f", b"");
    let v = [0u8, 1, 2, 255];
    assert_eq!(x::setxattr(f.as_ptr(), c"user.bin".as_ptr(), v.as_ptr().cast(), 4, 0), 0, "binary values");
    let mut b = [9u8; 4];
    assert_eq!(x::getxattr(f.as_ptr(), c"user.bin".as_ptr(), b.as_mut_ptr().cast(), 4), 4);
    assert_eq!(b, v);
    assert_eq!(x::removexattr(f.as_ptr(), c"user.bin".as_ptr()), 0);
    errno::set_raw(0);
    let r = x::setxattr(f.as_ptr(), c"trusted.k".as_ptr(), v.as_ptr().cast(), 1, 0);
    assert!(r == 0 || errno::get() == Errno::EPERM.get(), "trusted.: privileged");
    assert!(err(x::setxattr(f.as_ptr(), c"other.k".as_ptr(), v.as_ptr().cast(), 1, 0), Errno::EOPNOTSUPP), "only the four namespaces");
}

#[test]
fn s13_11_c_1() {
    let d = Dir::new("c111");
    let f = d.file("f", b"");
    errno::set_raw(0);
    assert!(err(x::setxattr(f.as_ptr(), c"user.k".as_ptr(), b"1".as_ptr().cast(), 1, 2), Errno::ENODATA), "XATTR_REPLACE needs it to exist");
    assert_eq!(x::setxattr(f.as_ptr(), c"user.k".as_ptr(), b"1".as_ptr().cast(), 1, 1), 0, "XATTR_CREATE");
    assert!(err(x::setxattr(f.as_ptr(), c"user.k".as_ptr(), b"2".as_ptr().cast(), 1, 1), Errno::EEXIST), "XATTR_CREATE needs it absent");
    assert_eq!(x::setxattr(f.as_ptr(), c"user.k".as_ptr(), b"2".as_ptr().cast(), 1, 2), 0, "XATTR_REPLACE");
}

#[test]
fn s13_11_c_2() {
    let d = Dir::new("c112");
    let f = d.file("f", b"");
    x::setxattr(f.as_ptr(), c"user.k".as_ptr(), [1u8; 300].as_ptr().cast(), 300, 0);
    assert_eq!(x::getxattr(f.as_ptr(), c"user.k".as_ptr(), null_mut(), 0), 300);
    let n = x::listxattr(f.as_ptr(), null_mut(), 0);
    assert!(n >= 7);
    let mut b = vec![0u8; n as usize];
    assert_eq!(x::listxattr(f.as_ptr(), b.as_mut_ptr().cast(), b.len()), n, "the size asked for is enough");
}

#[test]
fn s13_11_c_3() {
    let d = Dir::new("c113");
    let f = d.file("f", b"");
    for name in [c"user.one", c"user.two", c"user.three"] {
        x::setxattr(f.as_ptr(), name.as_ptr(), b"v".as_ptr().cast(), 1, 0);
    }
    let mut b = [0u8; 256];
    let n = x::listxattr(f.as_ptr(), b.as_mut_ptr().cast(), 256) as usize;
    assert_eq!(b[n - 1], 0, "each name NUL-terminated, the last too");
    let mut names: Vec<&[u8]> = b[..n - 1].split(|&c| c == 0).filter(|s| s.starts_with(b"user.")).collect();
    names.sort();
    assert_eq!(names, [&b"user.one"[..], b"user.three", b"user.two"]);
}

#[test]
fn s13_11_f_0() {
    let d = Dir::new("f110");
    let f = d.file("f", b"");
    let mut b = [0u8; 1];
    errno::set_raw(0);
    assert!(err(x::getxattr(f.as_ptr(), c"user.none".as_ptr(), b.as_mut_ptr().cast(), 1) as c_int, Errno::ENODATA), "ENODATA");
    x::setxattr(f.as_ptr(), c"user.k".as_ptr(), b"12".as_ptr().cast(), 2, 0);
    assert!(err(x::getxattr(f.as_ptr(), c"user.k".as_ptr(), b.as_mut_ptr().cast(), 1) as c_int, Errno::ERANGE), "ERANGE");
    assert!(err(x::setxattr(c"/proc/self/status".as_ptr(), c"user.k".as_ptr(), b"1".as_ptr().cast(), 1, 0), Errno::EOPNOTSUPP)
        || errno::get() == Errno::EPERM.get() || errno::get() == Errno::EACCES.get(), "ENOTSUP where the file system has none");
    let big = vec![0u8; 65537];
    assert!(err(x::setxattr(f.as_ptr(), c"user.big".as_ptr(), big.as_ptr().cast(), big.len(), 0), Errno::E2BIG), "E2BIG beyond XATTR_SIZE_MAX");
}

#[test]
fn s13_12_c_1() {
    // Unprivileged: the kernel refuses each kind of mount request, after the library passed
    // the optional arguments (NULL source, type, data) through.
    if root() {
        return;
    }
    let d = Dir::new("c121");
    errno::set_raw(0);
    for (src, ty, flags, data) in [
        (c"none".as_ptr(), c"tmpfs".as_ptr(), 0u64, c"size=1m".as_ptr()),
        (core::ptr::null(), core::ptr::null(), 0x20 | 0x1, core::ptr::null()),
        (d.at("").as_ptr(), core::ptr::null(), 0x1000, core::ptr::null()),
        (core::ptr::null(), core::ptr::null(), 1 << 18, core::ptr::null()),
    ] {
        assert!(err(x::mount(src, d.at("").as_ptr(), ty, flags, data.cast()), Errno::EPERM), "attach, remount, bind, propagation");
    }
}

#[test]
fn s13_12_c_2() {
    if root() {
        return;
    }
    for flags in [1, 2, 4, 8] {
        errno::set_raw(0);
        let r = x::umount2(c"/".as_ptr(), flags);
        assert!(r == -1 && [Errno::EPERM.get(), Errno::EINVAL.get()].contains(&errno::get()), "flags {flags}: MNT_FORCE, MNT_DETACH, MNT_EXPIRE, UMOUNT_NOFOLLOW");
    }
}

#[test]
fn s13_12_c_3() {
    // The descriptor API: open_tree works unprivileged; the rest reach the kernel typed.
    let d = Dir::new("c123");
    let t = x::open_tree(AT_FDCWD, d.at("").as_ptr(), 0x80000);
    assert!(t >= 0, "OPEN_TREE_CLOEXEC");
    assert_eq!(io::liella_fcntl3(t, 1, 0), 1);
    io::close(t);
    errno::set_raw(0);
    let r = x::fsopen(c"tmpfs".as_ptr(), 1);
    if r >= 0 {
        assert_eq!(x::fsconfig(r, 1, c"size".as_ptr(), c"1m".as_ptr().cast(), 0), 0, "a typed option, no data string");
        io::close(r);
    } else {
        assert_eq!(errno::get(), Errno::EPERM.get());
    }
}

// §13.13 (phase 12; D-13.15): the mount-table streams and the fstab enumeration. Fixtures live in
// scratch directories (`Scratch`); getmntent's result block is the calling thread's, so each test
// thread has its own; the fstab cursor is process-wide, so the fsent cases run in a child, with a
// fixture bind-mounted over /etc/fstab when the system gives the child its own user and mount
// namespace.

use p13::*;

mod p13 {
    use std::ffi::CString;

    pub(crate) use liella_abi::ctypes::c_char;
    pub(crate) use liella_abi::layout::struct_mntent;
    use liella_abi::layout::{struct_fstab, FILE};
    use liella_boundary::posix::s12_stdio as io12;
    use liella_boundary::posix::s13_filesystem as x;

    use super::p12::{cbytes, ctext, word};

    pub(crate) fn cs(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    fn text_at(a: u64) -> Vec<u8> {
        ctext(core::ptr::with_exposed_provenance::<c_char>(a as usize))
    }

    /// An entry as text: `fsname dir type opts freq passno` (strings in brackets).
    pub(crate) fn show(m: *const struct_mntent) -> String {
        if m.is_null() {
            return "NULL".into();
        }
        let b = cbytes(m.cast(), 40);
        let s = |i: usize| String::from_utf8_lossy(&text_at(word(&b, i * 8))).into_owned();
        let n = |at: usize| i32::from_ne_bytes(b[at..at + 4].try_into().unwrap());
        format!("[{}] [{}] [{}] [{}] {} {}", s(0), s(1), s(2), s(3), n(32), n(36))
    }

    /// A `struct fstab` as text: `spec file vfstype mntops type freq passno`.
    pub(crate) fn show_fs(f: *const struct_fstab) -> String {
        if f.is_null() {
            return "NULL".into();
        }
        let b = cbytes(f.cast(), 48);
        let s = |i: usize| String::from_utf8_lossy(&text_at(word(&b, i * 8))).into_owned();
        let n = |at: usize| i32::from_ne_bytes(b[at..at + 4].try_into().unwrap());
        format!("[{}] [{}] [{}] [{}] {} {} {}", s(0), s(1), s(2), s(3), s(4), n(40), n(44))
    }

    /// Every entry of the stream, shown.
    pub(crate) fn all(f: *mut FILE) -> Vec<String> {
        let mut v = Vec::new();
        loop {
            let m = x::getmntent(f);
            if m.is_null() {
                return v;
            }
            v.push(show(m));
        }
    }

    pub(crate) fn open(path: &CString, mode: &str) -> *mut FILE {
        let f = x::setmntent(path.as_ptr(), cs(mode).as_ptr());
        assert!(!f.is_null(), "setmntent {path:?} {mode}");
        f
    }

    pub(crate) fn entry(fs: &str, dir: &str, ty: &str, opts: &str, freq: i32, passno: i32) -> String {
        format!("[{fs}] [{dir}] [{ty}] [{opts}] {freq} {passno}")
    }

    pub(crate) fn ftell(f: *mut FILE) -> i64 {
        io12::ftell(f)
    }

    /// A `struct mntent` naming C strings the caller keeps alive.
    pub(crate) fn ment(s: &[&CString; 4], freq: i32, passno: i32) -> struct_mntent {
        let a = |c: &CString| c.as_ptr().expose_provenance();
        struct_mntent { mnt_fsname: a(s[0]), mnt_dir: a(s[1]), mnt_type: a(s[2]), mnt_opts: a(s[3]), mnt_freq: freq, mnt_passno: passno }
    }
}

#[test]
fn s13_13_c_1() {
    // setmntent opens a table as fopen does with the same mode, adding close-on-exec; the
    // kernel's table (/proc/self/mounts) reads as one with a "/" entry; errors are the open's.
    use liella_kernel::ops::FcntlInt;
    let d = Scratch::new("c1331");
    let t = d.c("t");
    for mode in ["r", "r+", "w", "w+", "a", "a+", "re", "rb"] {
        std::fs::write(d.at("t"), b"a /a t o 0 0\n").unwrap();
        let f = open(&t, mode);
        let fd = liella_boundary::posix::s12_stdio::fileno(f);
        assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(1), "close-on-exec ({mode})");
        let acc = CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap() & 3;
        assert_eq!(acc, if mode.contains('+') { 2 } else if mode.starts_with('r') { 0 } else { 1 }, "access mode ({mode})");
        assert_eq!(x::endmntent(f), 1);
    }
    std::fs::write(d.at("t"), b"a /a t o 0 0\n").unwrap();
    let f = open(&t, "r");
    assert_eq!(all(f), [entry("a", "/a", "t", "o", 0, 0)]);
    assert_eq!(x::endmntent(f), 1);
    for (path, mode, e) in [(d.c("none"), "r", Errno::ENOENT), (d.c("t/x"), "r", Errno::ENOTDIR), (t.clone(), "z", Errno::EINVAL), (t.clone(), "", Errno::EINVAL), (t.clone(), "+r", Errno::EINVAL)] {
        errno::set_raw(0);
        assert!(x::setmntent(path.as_ptr(), cs(mode).as_ptr()).is_null() && errno::get() == e.get(), "{path:?} {mode:?}");
    }
    errno::set_raw(0);
    assert!(x::setmntent(core::ptr::null(), c"r".as_ptr()).is_null() && errno::get() == Errno::EFAULT.get(), "a NULL file");
    assert!(x::setmntent(t.as_ptr(), core::ptr::null()).is_null() && errno::get() == Errno::EFAULT.get(), "a NULL mode");
    // The kernel's table: four non-empty strings per entry, the numbers 0, and the root.
    let f = open(&cs("/proc/self/mounts"), "r");
    let mut root_seen = false;
    loop {
        let m = x::getmntent(f);
        if m.is_null() {
            break;
        }
        let s = show(m);
        assert!(!s.contains("[]") && s.ends_with(" 0 0"), "{s}");
        root_seen |= s.split("] [").nth(1) == Some("/");
    }
    assert!(root_seen, "the / entry");
    assert_eq!(x::endmntent(f), 1);
    assert_eq!(x::endmntent(null_mut()), 1, "NULL is a no-op returning 1");
}

#[test]
fn s13_13_c_2() {
    // getmntent splits each line at runs of blanks into the six fields, decodes \040 \011 \012
    // \134 (and \\) in the four strings, skips comments, blank lines and lines that are not
    // entries (a NUL byte, a number outside int), keeps lines of any length, and returns the
    // calling thread's block, which the next call overwrites. getmntent_r fills the caller's
    // structure and buffer; a buffer too small is ERANGE and the line is returned by a retry.
    let d = Scratch::new("c1332");
    let long = "L".repeat(10_000);
    let mut text = b"# comment\n\n  \t\n /dev/a\\040b\t/m\\011x\\012y\\134z  ext4 rw,noatime 1 2 extra\n".to_vec();
    text.extend_from_slice(b"short\nnul\0x /n t o 0 0\nbig /b t o 2147483648 0\n");
    text.extend_from_slice(format!("{long} /l t o -1 +2\n").as_bytes());
    text.extend_from_slice(b"\\101\\\\ /c t\\ o 0x1 7\nlast /z t o 3");
    std::fs::write(d.at("t"), &text).unwrap();
    let f = open(&d.c("t"), "r");
    let a = x::getmntent(f);
    assert_eq!(show(a), entry("/dev/a b", "/m\tx\ny\\z", "ext4", "rw,noatime", 1, 2));
    let b = x::getmntent(f);
    assert_eq!(a, b, "the same block on each call");
    assert_eq!(show(a), entry("short", "", "", "", 0, 0), "the first result is overwritten");
    assert_eq!(show(x::getmntent(f)), entry(&long, "/l", "t", "o", -1, 2), "no length limit; the NUL line and the out-of-range line are skipped");
    assert_eq!(show(x::getmntent(f)), entry("\\101\\", "/c", "t\\", "o", 0, 7), "only the escapes; each number from its own field");
    assert_eq!(show(x::getmntent(f)), entry("last", "/z", "t", "o", 3, 0), "an unterminated last line");
    errno::set_raw(Errno::EINTR.get());
    assert!(x::getmntent(f).is_null() && errno::get() == Errno::EINTR.get(), "the end leaves errno alone");
    assert_eq!(x::endmntent(f), 1);
    // getmntent_r.
    std::fs::write(d.at("r"), b"first /first t1 o1 1 1\nsecond /s t o 0 0\n").unwrap();
    let f = open(&d.c("r"), "r");
    let mut me = struct_mntent { mnt_fsname: 0, mnt_dir: 0, mnt_type: 0, mnt_opts: 0, mnt_freq: 0, mnt_passno: 0 };
    let mut small = [b'@' as c_char; 16];
    errno::set_raw(0);
    assert!(x::getmntent_r(f, Some(&mut me), small.as_mut_ptr(), 16).is_null() && errno::get() == Errno::ERANGE.get(), "16 bytes cannot hold the strings (19 needed)");
    assert!(small.iter().all(|&c| c == b'@' as c_char), "nothing written");
    let mut buf = [0 as c_char; 19];
    let r = x::getmntent_r(f, Some(&mut me), buf.as_mut_ptr(), 19);
    assert_eq!(r, core::ptr::from_mut(&mut me), "its argument");
    assert_eq!(show(r), entry("first", "/first", "t1", "o1", 1, 1), "the retry returns the line");
    assert_eq!(me.mnt_fsname, buf.as_ptr().expose_provenance(), "the strings are in the buffer");
    assert_eq!(me.mnt_opts, buf.as_ptr().expose_provenance() + 16);
    errno::set_raw(0);
    assert!(x::getmntent_r(f, Some(&mut me), buf.as_mut_ptr(), -1).is_null() && errno::get() == Errno::EINVAL.get(), "a negative size");
    assert!(x::getmntent_r(f, None, buf.as_mut_ptr(), 19).is_null() && errno::get() == Errno::EFAULT.get(), "a NULL mntbuf");
    assert!(x::getmntent_r(f, Some(&mut me), core::ptr::null_mut(), 19).is_null() && errno::get() == Errno::EFAULT.get(), "a NULL buffer");
    assert!(x::getmntent_r(null_mut(), Some(&mut me), buf.as_mut_ptr(), 19).is_null() && errno::get() == Errno::EINVAL.get(), "a NULL stream");
    assert!(x::getmntent(null_mut()).is_null() && errno::get() == Errno::EINVAL.get());
    assert_eq!(show(x::getmntent_r(f, Some(&mut me), buf.as_mut_ptr(), 19)), entry("second", "/s", "t", "o", 0, 0), "the failed calls read nothing");
    errno::set_raw(0);
    assert!(x::getmntent_r(f, Some(&mut me), buf.as_mut_ptr(), 19).is_null() && errno::get() == 0, "the end");
    assert_eq!(x::endmntent(f), 1);
}

#[test]
fn s13_13_c_3() {
    // addmntent writes the entry's line at the end of the file whatever the stream's position,
    // escaping space, tab, newline and backslash, and flushes it; an entry that would not read
    // back is EINVAL. hasmntopt finds an option by name (or exactly, with =) and returns its
    // address inside mnt_opts.
    let d = Scratch::new("c1333");
    std::fs::write(d.at("t"), b"one /1 t o 0 0\ntwo /2 t o 0 0\n").unwrap();
    let f = open(&d.c("t"), "r+");
    assert_eq!(show(x::getmntent(f)), entry("one", "/1", "t", "o", 0, 0));
    let (s0, s1, s2, s3) = (cs("a b"), cs("/m\tp"), cs("t\ny"), cs("o\\p"));
    let m = ment(&[&s0, &s1, &s2, &s3], -1, 7);
    assert_eq!(x::addmntent(f, Some(&m)), 0);
    assert_eq!(std::fs::read(d.at("t")).unwrap(), b"one /1 t o 0 0\ntwo /2 t o 0 0\na\\040b /m\\011p t\\012y o\\134p -1 7\n", "appended and flushed");
    assert_eq!(ftell(f), 64, "the stream is at the end");
    assert!(x::getmntent(f).is_null());
    for (strs, why) in [(["", "/d", "t", "o"], "an empty source"), (["s", "/d", "t", ""], "empty options"), (["#s", "/d", "t", "o"], "a source read back as a comment")] {
        let c: Vec<CString> = strs.iter().map(|s| cs(s)).collect();
        errno::set_raw(0);
        assert_eq!(x::addmntent(f, Some(&ment(&[&c[0], &c[1], &c[2], &c[3]], 0, 0))), 1, "{why}");
        assert_eq!(errno::get(), Errno::EINVAL.get(), "{why}");
    }
    let mut nul = m;
    nul.mnt_dir = 0;
    assert_eq!(x::addmntent(f, Some(&nul)), 1, "a NULL field");
    assert_eq!(x::addmntent(f, None), 1, "a NULL entry");
    assert_eq!(x::endmntent(f), 1);
    let f = open(&d.c("t"), "r");
    assert_eq!(all(f)[2], entry("a b", "/m\tp", "t\ny", "o\\p", -1, 7), "reads back; nothing else was written");
    errno::set_raw(0);
    assert_eq!(x::addmntent(f, Some(&m)), 1, "a read-only stream");
    assert_eq!(errno::get(), Errno::EBADF.get());
    assert_eq!(x::endmntent(f), 1);
    // hasmntopt.
    for (opts, q, at) in [
        ("rw,noro,ro", "ro", Some(8)),
        ("relatime", "atime", None),
        ("size=10,ro", "size", Some(0)),
        ("size=10,ro", "size=10", Some(0)),
        ("size=10,ro", "size=1", None),
        ("rw=1", "rw=", None),
        ("rw,ro", "rw,ro", None),
        ("a,,b", "", Some(2)),
        ("RW", "rw", None),
    ] {
        let (o, qc) = (cs(opts), cs(q));
        let e = ment(&[&s0, &s1, &s2, &o], 0, 0);
        let r = x::hasmntopt(Some(&e), qc.as_ptr());
        assert_eq!(r, at.map_or(null_mut(), |a| o.as_ptr().cast_mut().wrapping_add(a)), "{opts} / {q}");
    }
    errno::set_raw(0);
    assert!(x::hasmntopt(Some(&m), core::ptr::null()).is_null() && errno::get() == Errno::EINVAL.get(), "a NULL query");
    errno::set_raw(0);
    assert!(x::hasmntopt(Some(&nul), c"ro".as_ptr()).is_null() && errno::get() == 0, "mnt_dir is not read");
    let mut no_opts = m;
    no_opts.mnt_opts = 0;
    assert!(x::hasmntopt(Some(&no_opts), c"ro".as_ptr()).is_null() && errno::get() == Errno::EINVAL.get(), "a NULL mnt_opts");
    assert!(x::hasmntopt(None, c"ro".as_ptr()).is_null() && errno::get() == Errno::EINVAL.get(), "a NULL entry");
}

#[test]
fn s13_13_c_4() {
    // The fsent functions read /etc/fstab into struct fstab: the six fields as getmntent reads
    // them, and fs_type, the last of rw, rq, ro, sw, xx among the options ("??" without one).
    // getfsent opens the file on first use; getfsspec and getfsfile find the first entry with
    // that field, through their own descriptor (the enumeration does not move). The cursor is
    // process-wide: the cases run in a child, on a fixture bind-mounted over /etc/fstab when the
    // child has its own namespaces, else on the system's file.
    const FIXTURE: &[u8] = b"# fixture\n/dev/a / ext4 defaults 0 1\n\nUUID=b /boot vfat rw,ro 0 2\n/dev/c none swap sw,xx 0 0\n/dev/sp\\040ace /mnt/x ext4 noauto,ro=1 1\n/dev/a /dup ext4 rq 0 0";
    let d = Scratch::new("c1334");
    std::fs::write(d.at("fstab"), FIXTURE).unwrap();
    in_child_ns("s13_13_c_4", &d, |dir| {
        let fixture = private_namespace();
        if fixture {
            let r = x::mount(cpath(&dir.join("fstab")).as_ptr(), c"/etc/fstab".as_ptr(), core::ptr::null(), 0x1000, core::ptr::null());
            assert_eq!(r, 0, "MS_BIND of the fixture over /etc/fstab, errno {}", errno::get());
        }
        let want: Vec<String> = if fixture {
            vec![
                "[/dev/a] [/] [ext4] [defaults] ?? 0 1".into(),
                "[UUID=b] [/boot] [vfat] [rw,ro] ro 0 2".into(),
                "[/dev/c] [none] [swap] [sw,xx] xx 0 0".into(),
                "[/dev/sp ace] [/mnt/x] [ext4] [noauto,ro=1] ro 1 0".into(),
                "[/dev/a] [/dup] [ext4] [rq] rq 0 0".into(),
            ]
        } else if std::path::Path::new("/etc/fstab").exists() {
            // The system's file: the same entries as getmntent reads (fs_type left out).
            let f = open(&cs("/etc/fstab"), "r");
            let v = all(f);
            x::endmntent(f);
            v
        } else {
            errno::set_raw(0);
            assert_eq!(x::setfsent(), 0);
            assert_eq!(errno::get(), Errno::ENOENT.get(), "no /etc/fstab");
            assert!(x::getfsent().is_null());
            return;
        };
        let strip_type = |s: String| if fixture { s } else { let p: Vec<&str> = s.rsplitn(4, ' ').collect(); format!("{} {} {}", p[3], p[1], p[0]) };
        let first = x::getfsent();
        assert_eq!(first.is_null(), want.is_empty(), "getfsent opens the file itself");
        assert_eq!(x::setfsent(), 1);
        let mut got = Vec::new();
        loop {
            errno::set_raw(Errno::EINTR.get());
            let e = x::getfsent();
            if e.is_null() {
                assert_eq!(errno::get(), Errno::EINTR.get(), "the end leaves errno alone");
                break;
            }
            got.push(strip_type(show_fs(e)));
        }
        assert_eq!(got, want);
        if want.is_empty() {
            x::endfsent();
            return;
        }
        assert_eq!(x::setfsent(), 1, "already open: back to the first line");
        let a = x::getfsent();
        assert_eq!(strip_type(show_fs(a)), want[0]);
        if fixture {
            let s = x::getfsspec(c"/dev/a".as_ptr());
            assert_eq!(show_fs(s), want[0], "the first of two");
            assert_eq!(s, a, "getfsspec and getfsent share the block");
            assert_eq!(show_fs(x::getfsfile(c"/dup".as_ptr())), want[4]);
            assert_eq!(show_fs(x::getfsspec(c"/dev/sp ace".as_ptr())), want[3], "the decoded field");
            for key in [c"/dev/sp\\040ace", c"/dev", c"", c"/nowhere"] {
                errno::set_raw(0);
                assert!(x::getfsspec(key.as_ptr()).is_null() && errno::get() == 0, "{key:?}");
            }
            assert_eq!(show_fs(x::getfsent()), want[1], "the lookups did not move the enumeration");
            errno::set_raw(0);
            assert!(x::getfsfile(core::ptr::null()).is_null() && errno::get() == Errno::EFAULT.get(), "a NULL key");
        }
        x::endfsent();
        x::endfsent();
        assert_eq!(strip_type(show_fs(x::getfsent())), want[0], "after endfsent, from the start");
        x::endfsent();
    });
}
#[test]
fn s13_14_c_0() {
    if root() {
        return;
    }
    let d = Dir::new("c140");
    let f = d.file("swap", &[0u8; 4096]);
    errno::set_raw(0);
    assert!(err(x::swapon(f.as_ptr(), 0x8000 | 5), Errno::EPERM), "SWAP_FLAG_PREFER with a priority, refused unprivileged");
    assert!(err(x::swapoff(f.as_ptr()), Errno::EPERM));
}

#[test]
fn s13_15_c_0() {
    errno::set_raw(0);
    assert!(err(x::chflags(c"/tmp".as_ptr(), 0), Errno::ENOSYS));
    assert!(err(x::fchflags(0, 0x1), Errno::ENOSYS));
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s13_4_c_4() {
    // unlink removes a non-directory, rmdir an empty directory, remove either.
    let d = Dir::new("c44");
    let f = d.file("f", b"x");
    std::fs::create_dir(d.0.join("e")).unwrap();
    let e = d.at("e");
    errno::set_raw(0);
    assert!(err(x::unlink(e.as_ptr()), Errno::EISDIR), "unlink of a directory");
    assert!(err(x::rmdir(f.as_ptr()), Errno::ENOTDIR), "rmdir of a file");
    assert_eq!(x::remove(f.as_ptr()), 0, "remove: a file");
    assert_eq!(x::remove(e.as_ptr()), 0, "remove: an empty directory");
    let g = d.file("g", b"");
    assert_eq!(x::unlink(g.as_ptr()), 0);
    std::fs::create_dir(d.0.join("h")).unwrap();
    assert_eq!(x::rmdir(d.at("h").as_ptr()), 0);
    assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 0);
}

#[test]
fn s13_4_c_5() {
    // rename replaces the target atomically; renameat2 adds NOREPLACE and EXCHANGE.
    let d = Dir::new("c45");
    let (a, b) = (d.file("a", b"A"), d.file("b", b"B"));
    assert_eq!(x::rename(a.as_ptr(), b.as_ptr()), 0);
    assert_eq!(std::fs::read(d.0.join("b")).unwrap(), b"A", "replaced");
    assert!(!d.0.join("a").exists());
    let a = d.file("a", b"A2");
    errno::set_raw(0);
    assert!(err(x::renameat2(AT_FDCWD, a.as_ptr(), AT_FDCWD, b.as_ptr(), 1), Errno::EEXIST), "RENAME_NOREPLACE");
    assert_eq!(x::renameat2(AT_FDCWD, a.as_ptr(), AT_FDCWD, b.as_ptr(), 2), 0, "RENAME_EXCHANGE");
    assert_eq!((std::fs::read(d.0.join("a")).unwrap(), std::fs::read(d.0.join("b")).unwrap()), (b"A".to_vec(), b"A2".to_vec()));
}

#[test]
fn s13_7_c_5() {
    // utimensat/futimens: nanosecond times, UTIME_NOW and UTIME_OMIT per field.
    const UTIME_NOW: i64 = (1 << 30) - 1;
    const UTIME_OMIT: i64 = (1 << 30) - 2;
    let d = Dir::new("c75");
    let f = d.file("f", b"");
    let t = [Timespec { sec: 1_000_000_000, nsec: 123_456_789 }, Timespec { sec: 1_500_000_000, nsec: 987_654_321 }];
    assert_eq!(x::utimensat(AT_FDCWD, f.as_ptr(), Some(&t), 0), 0);
    let st = stat(&f);
    assert_eq!((st.atime.sec, st.atime.nsec, st.mtime.sec, st.mtime.nsec), (1_000_000_000, 123_456_789, 1_500_000_000, 987_654_321));
    let fd = io::liella_open3(f.as_ptr(), O_RDWR, 0);
    let keep = [Timespec { sec: 0, nsec: UTIME_OMIT }, Timespec { sec: 0, nsec: UTIME_NOW }];
    assert_eq!(x::futimens(fd, Some(&keep)), 0);
    io::close(fd);
    let st = stat(&f);
    assert_eq!((st.atime.sec, st.atime.nsec), (1_000_000_000, 123_456_789), "UTIME_OMIT keeps the access time");
    assert!(st.mtime.sec > 1_600_000_000, "UTIME_NOW sets the modification time");
}

#[test]
fn s13_7_c_6() {
    // The older forms: utime in seconds, utimes and futimes in microseconds.
    let d = Dir::new("c76");
    let f = d.file("f", b"");
    assert_eq!(x::utime(f.as_ptr(), Some(&[1_000_000_000, 1_100_000_000])), 0);
    let st = stat(&f);
    assert_eq!((st.atime.sec, st.atime.nsec, st.mtime.sec), (1_000_000_000, 0, 1_100_000_000));
    let tv = [Timeval { sec: 1_200_000_000, usec: 5 }, Timeval { sec: 1_300_000_000, usec: 7 }];
    assert_eq!(x::utimes(f.as_ptr(), Some(&tv)), 0);
    let st = stat(&f);
    assert_eq!((st.atime.nsec, st.mtime.sec, st.mtime.nsec), (5_000, 1_300_000_000, 7_000), "microseconds");
    let fd = io::liella_open3(f.as_ptr(), O_RDWR, 0);
    assert_eq!(x::futimes(fd, Some(&[Timeval { sec: 1_400_000_000, usec: 1 }, Timeval { sec: 1_400_000_001, usec: 2 }])), 0);
    io::close(fd);
    assert_eq!(stat(&f).mtime.sec, 1_400_000_001);
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
