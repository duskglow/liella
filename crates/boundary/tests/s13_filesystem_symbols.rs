// Specified in part from spec §13 (phase 5: §13.4–13.7, §13.10–13.12, §13.14 and the §11.8
// lengths; phase 12: the working directory, directory streams, walks, realpath and temporary
// files, and the mount-table streams); `revoke` is phase 11.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §13 (173 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Each test works in its own directory under the temporary directory. Calls that need
//! privileges (mounting, swap, device nodes) are checked to reach the kernel: as an
//! unprivileged user they fail with the kernel's `EPERM`, and a malformed argument is still
//! rejected first where the C interface defines that.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/kernel.rs"]
mod kernel;

use core::ptr::{null, null_mut};
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicU32, Ordering};

use liella_abi::ctypes::{c_char, c_int};
use liella_abi::layout::struct_statvfs;
use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s13_filesystem as s;
use liella_kernel::uapi::{Stat, Statfs, Statx, Timespec, Timeval};

const ENOENT: i32 = 2;
const EBADF: i32 = 9;
const EACCES: i32 = 13;
const EFAULT: i32 = 14;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EISDIR: i32 = 21;
const EINVAL: i32 = 22;
const EPERM: i32 = 1;
const ENODATA: i32 = 61;
const ERANGE: i32 = 34;
const ENOTEMPTY: i32 = 39;
const ENOSYS: i32 = 38;
const ELOOP: i32 = 40;
const EOPNOTSUPP: i32 = 95;
const AT_FDCWD: c_int = -100;
/// A descriptor number no test is given (numbering is process-wide, so a number a test just
/// closed may already belong to a concurrently running test).
const CLOSED: c_int = 999;
const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
const AT_REMOVEDIR: c_int = 0x200;
const AT_EACCESS: c_int = 0x200;
const AT_EMPTY_PATH: c_int = 0x1000;
const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 2;
const O_CREAT: c_int = 0o100;
const O_DIRECTORY: c_int = 0o200000;
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;
const S_IFIFO: u32 = 0o010000;
const UTIME_OMIT: i64 = (1 << 30) - 2;

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty directory of this test's own; paths inside it with [`at`].
struct Dir(std::path::PathBuf);
impl Dir {
    fn new(tag: &str) -> Dir {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let p = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("s13d-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir(&p).unwrap();
        Dir(p)
    }
    fn at(&self, name: &str) -> CString {
        CString::new(self.0.join(name).into_os_string().into_encoded_bytes()).unwrap()
    }
    fn c(&self) -> CString {
        CString::new(self.0.clone().into_os_string().into_encoded_bytes()).unwrap()
    }
    /// A regular file `name` holding `content`.
    fn file(&self, name: &str, content: &[u8]) -> CString {
        std::fs::write(self.0.join(name), content).unwrap();
        self.at(name)
    }
    fn open(&self) -> c_int {
        let fd = io::liella_open3(self.c().as_ptr(), O_RDONLY | O_DIRECTORY, 0);
        assert!(fd >= 0);
        fd
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// -1 with `errno == e`.
fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn zstat() -> Stat {
    Stat::default()
}

fn exists(p: &CStr) -> bool {
    std::fs::symlink_metadata(p.to_str().unwrap()).is_ok()
}

fn mode_of(p: &CStr) -> u32 {
    let mut st = zstat();
    assert_eq!(s::lstat(p.as_ptr(), Some(&mut st)), 0);
    st.mode
}

fn root() -> bool {
    liella_kernel::sys::Sys::geteuid(&liella_boundary::sys::CSys) == 0
}

fn zero_statvfs() -> struct_statvfs {
    struct_statvfs {
        f_bsize: 0, f_frsize: 0, f_blocks: 0, f_bfree: 0, f_bavail: 0, f_files: 0, f_ffree: 0, f_favail: 0, f_fsid: 0, f_flag: 0,
        f_namemax: 0, f_type: 0, __f_spare: [0; 5],
    }
}

#[allow(dead_code)]
fn c_char_unused(_: *const c_char) {}

#[test]
fn sym_access() {
    let d = Dir::new("access");
    let f = d.file("f", b"");
    std::fs::set_permissions(f.to_str().unwrap(), std::os::unix::fs::PermissionsExt::from_mode(0o400)).unwrap();
    assert_eq!(s::access(f.as_ptr(), 0), 0, "F_OK");
    assert_eq!(s::access(f.as_ptr(), 4), 0, "R_OK");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::access(f.as_ptr(), 2) as isize, EACCES), "W_OK");
    }
    assert!(fails(s::access(d.at("none").as_ptr(), 0) as isize, ENOENT));
    assert!(fails(s::access(f.as_ptr(), 8) as isize, EINVAL), "an unknown mode bit");
}

#[test]
fn sym_addmntent() {
    let d = Dir::new("addmntent");
    let path = d.at("t");
    let f = mopen(&path, "w");
    let (a, b, c, o) = (cs("src x"), cs("/m"), cs("t"), cs("rw"));
    assert_eq!(s::addmntent(f, Some(&ment([&a, &b, &c, &o], 1, 2))), 0);
    assert_eq!(std::fs::read(path.to_str().unwrap()).unwrap(), b"src\\040x /m t rw 1 2\n", "written and flushed at once");
    let empty = cs("");
    errno::set_raw(0);
    assert_eq!(s::addmntent(f, Some(&ment([&a, &b, &c, &empty], 0, 0))), 1);
    assert_eq!(errno::get(), EINVAL, "an entry that would not read back");
    assert_eq!(s::addmntent(f, None), 1);
    assert_eq!(errno::get(), EINVAL);
    s::endmntent(f);
    errno::set_raw(0);
    assert_eq!(s::addmntent(null_mut(), Some(&ment([&a, &b, &c, &o], 0, 0))), 1);
    assert_eq!(errno::get(), EINVAL, "a NULL stream");
}
#[test]
fn sym_alphasort() {
    // The names in the current locale's collation (byte order in the C locale); the sign only.
    let (a, b, big) = (dirent(b"a"), dirent(b"b"), dirent(b"B"));
    let (pa, pb, pbig) = (ent_ptr(&a), ent_ptr(&b), ent_ptr(&big));
    assert!(s::alphasort(&pa, &pb) < 0);
    assert!(s::alphasort(&pb, &pa) > 0);
    assert_eq!(s::alphasort(&pa, &pa), 0);
    assert!(s::alphasort(&pa, &pbig) > 0, "C locale: 'B' before 'a'");
}

#[test]
fn sym_alphasort64() {
    let (a, b) = (dirent(b"a"), dirent(b"b"));
    let (pa, pb) = (ent_ptr(&a), ent_ptr(&b));
    assert!(s::alphasort64(&pa, &pb) < 0);
    assert_eq!(s::alphasort64(&pb, &pb), 0);
}

#[test]
fn sym_canonicalize_file_name() {
    let t = Tmp::new("cfn");
    t.dir("a");
    t.file("a/f");
    std::os::unix::fs::symlink("a", t.at("l")).unwrap();
    let want = t.at("a/f").as_os_str().as_encoded_bytes().to_vec();
    assert_eq!(ctext(s::canonicalize_file_name(cpath(&t.at("l/./f")).as_ptr())), want, "allocated");
    errno::set_raw(0);
    assert!(s::canonicalize_file_name(null()).is_null() && errno::get() == EINVAL, "NULL");
    errno::set_raw(0);
    assert!(s::canonicalize_file_name(cpath(&t.at("missing")).as_ptr()).is_null() && errno::get() == ENOENT);
}

#[test]
fn sym_chdir() {
    let t = Tmp::new("chdir");
    errno::set_raw(0);
    assert!(fails(s::chdir(null()) as isize, EFAULT), "NULL");
    in_child("sym_chdir", &t, |dir| {
        assert_eq!(s::chdir(cpath(dir).as_ptr()), 0);
        assert_eq!(kernel_cwd(), dir.as_os_str().as_encoded_bytes());
        errno::set_raw(0);
        assert!(fails(s::chdir(cs("missing-here").as_ptr()) as isize, ENOENT));
    });
}

#[test]
fn sym_chmod() {
    let d = Dir::new("chmod");
    let f = d.file("f", b"");
    assert_eq!(s::chmod(f.as_ptr(), 0o640), 0);
    assert_eq!(mode_of(&f) & 0o7777, 0o640);
    errno::set_raw(0);
    assert!(fails(s::chmod(d.at("none").as_ptr(), 0o600) as isize, ENOENT));
    assert!(fails(s::chmod(cs("/").as_ptr(), 0o755) as isize, EPERM) || root(), "not the owner");
}

#[test]
fn sym_chown() {
    let d = Dir::new("chown");
    let f = d.file("f", b"");
    let mut st = zstat();
    s::stat(f.as_ptr(), Some(&mut st));
    assert_eq!(s::chown(f.as_ptr(), st.uid, st.gid), 0, "to the current owner: allowed");
    assert_eq!(s::chown(f.as_ptr(), u32::MAX, u32::MAX), 0, "-1 leaves both unchanged");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::chown(f.as_ptr(), 0, u32::MAX) as isize, EPERM), "giving the file away");
    }
}

#[test]
fn sym_chroot() {
    // Reaches the kernel: unprivileged it is refused (EPERM), a missing directory is ENOENT.
    errno::set_raw(0);
    assert!(fails(s::chroot(null()) as isize, EFAULT));
    if root() {
        return;
    }
    let t = Tmp::new("chroot");
    errno::set_raw(0);
    assert!(fails(s::chroot(cpath(&t.path).as_ptr()) as isize, EPERM));
    errno::set_raw(0);
    assert!(fails(s::chroot(cpath(&t.at("missing")).as_ptr()) as isize, ENOENT));
}

#[test]
fn sym_closedir() {
    let t = Tmp::new("closedir");
    let d = s::opendir(cpath(&t.path).as_ptr());
    assert_eq!(s::closedir(d), 0);
    errno::set_raw(0);
    assert!(fails(s::closedir(null_mut()) as isize, EINVAL), "NULL");
    // A closed stream (its slot is recycled, never freed) is EBADF; checked alone, since a
    // concurrently running test could reuse the slot.
    in_child("sym_closedir", &t, |dir| {
        let d = s::opendir(cpath(dir).as_ptr());
        assert_eq!(s::closedir(d), 0);
        errno::set_raw(0);
        assert!(fails(s::closedir(d) as isize, EBADF), "closed twice");
    });
}

#[test]
fn sym_dirfd() {
    let t = Tmp::new("dirfd");
    let d = s::opendir(cpath(&t.path).as_ptr());
    let fd = s::dirfd(d);
    assert!(fd >= 0);
    let mut st = zstat();
    assert_eq!(s::fstat(fd, Some(&mut st)), 0);
    assert_eq!(st.mode & S_IFMT, S_IFDIR);
    s::closedir(d);
    errno::set_raw(0);
    assert!(fails(s::dirfd(null_mut()) as isize, EINVAL));
}

#[test]
fn sym_eaccess() {
    let d = Dir::new("eaccess");
    let f = d.file("f", b"");
    std::fs::set_permissions(f.to_str().unwrap(), std::os::unix::fs::PermissionsExt::from_mode(0o100)).unwrap();
    assert_eq!(s::eaccess(f.as_ptr(), 1), 0, "X_OK by the effective IDs");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::eaccess(f.as_ptr(), 4) as isize, EACCES));
    }
}

#[test]
fn sym_endfsent() {
    let t = Tmp::new("endfsent");
    in_child("sym_endfsent", &t, |_| {
        let want = fstab_entries();
        s::endfsent();
        s::endfsent();
        let e = s::getfsent();
        assert_eq!(show_fs(e).map(|v| v.0), want.first().cloned(), "getfsent opens the file again");
        let _ = s::getfsent();
        s::endfsent();
        assert_eq!(show_fs(s::getfsent()).map(|v| v.0), want.first().cloned(), "and starts at the first entry");
        s::endfsent();
    });
}

fn endmntent_case(end: extern "C" fn(*mut liella_abi::layout::FILE) -> c_int, tag: &str) {
    let d = Dir::new(tag);
    let path = d.at("t");
    let f = mopen(&path, "w");
    liella_boundary::posix::s12_stdio::fputs(c"pending".as_ptr(), f);
    assert_eq!(end(f), 1);
    assert_eq!(std::fs::read(path.to_str().unwrap()).unwrap(), b"pending", "flushed when closed");
    assert_eq!(end(null_mut()), 1, "NULL: nothing to close");
    let f = s::setmntent(c"/dev/full".as_ptr(), c"w".as_ptr());
    if !f.is_null() {
        liella_boundary::posix::s12_stdio::fputs(c"x".as_ptr(), f);
        errno::set_raw(0);
        assert_eq!(end(f), 1, "1 even when the flush fails");
        assert_eq!(errno::get(), 28, "ENOSPC");
    }
}

#[test]
fn sym___endmntent() {
    endmntent_case(s::__endmntent, "__endmntent");
}

#[test]
fn sym_endmntent() {
    endmntent_case(s::endmntent, "endmntent");
}
#[test]
fn sym_euidaccess() {
    let d = Dir::new("euidaccess");
    let f = d.file("f", b"");
    std::fs::set_permissions(f.to_str().unwrap(), std::os::unix::fs::PermissionsExt::from_mode(0o100)).unwrap();
    assert_eq!(s::euidaccess(f.as_ptr(), 1), 0, "X_OK by the effective IDs");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::euidaccess(f.as_ptr(), 4) as isize, EACCES));
    }
}

#[test]
fn sym_faccessat() {
    let d = Dir::new("faccessat");
    let f = d.file("f", b"");
    std::fs::set_permissions(f.to_str().unwrap(), std::os::unix::fs::PermissionsExt::from_mode(0o500)).unwrap();
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    assert_eq!(s::faccessat(dir, cs("f").as_ptr(), 1, AT_EACCESS), 0, "AT_EACCESS (faccessat2)");
    assert_eq!(s::faccessat(dir, cs("l").as_ptr(), 4, AT_SYMLINK_NOFOLLOW), 0, "AT_SYMLINK_NOFOLLOW");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::faccessat(dir, cs("f").as_ptr(), 2, AT_EACCESS) as isize, EACCES));
    }
    assert!(fails(s::faccessat(dir, cs("f").as_ptr(), 0, 0x1) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_fchdir() {
    let t = Tmp::new("fchdir");
    errno::set_raw(0);
    assert!(fails(s::fchdir(-1) as isize, EBADF));
    in_child("sym_fchdir", &t, |dir| {
        let fd = io::liella_open3(cpath(dir).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
        assert_eq!(s::fchdir(fd), 0);
        assert_eq!(kernel_cwd(), dir.as_os_str().as_encoded_bytes());
        io::close(fd);
    });
}

#[test]
fn sym_fchmod() {
    let d = Dir::new("fchmod");
    let f = d.file("f", b"");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    assert_eq!(s::fchmod(fd, 0o604), 0);
    assert_eq!(mode_of(&f) & 0o777, 0o604);
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::fchmod(CLOSED, 0o600) as isize, EBADF));
}

#[test]
fn sym_fchmodat() {
    let d = Dir::new("fchmodat");
    let f = d.file("f", b"");
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    assert_eq!(s::fchmodat(dir, cs("l").as_ptr(), 0o611, 0), 0, "follows the link");
    assert_eq!(mode_of(&f) & 0o777, 0o611);
    assert_eq!(s::fchmodat(dir, cs("f").as_ptr(), 0o622, AT_SYMLINK_NOFOLLOW), 0, "AT_SYMLINK_NOFOLLOW on a file");
    assert_eq!(mode_of(&f) & 0o777, 0o622);
    errno::set_raw(0);
    assert!(fails(s::fchmodat(dir, cs("l").as_ptr(), 0o600, AT_SYMLINK_NOFOLLOW) as isize, EOPNOTSUPP), "symlinks have no mode");
    assert!(fails(s::fchmodat(dir, cs("f").as_ptr(), 0o600, 0x1) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_fchown() {
    let d = Dir::new("fchown");
    let f = d.file("f", b"");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    s::fstat(fd, Some(&mut st));
    assert_eq!(s::fchown(fd, u32::MAX, st.gid), 0);
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::fchown(CLOSED, 0, 0) as isize, EBADF));
}

#[test]
fn sym_fchownat() {
    let d = Dir::new("fchownat");
    d.file("f", b"");
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    assert_eq!(s::fchownat(dir, cs("l").as_ptr(), u32::MAX, u32::MAX, AT_SYMLINK_NOFOLLOW), 0);
    assert_eq!(s::fchownat(dir, cs("").as_ptr(), u32::MAX, u32::MAX, AT_EMPTY_PATH), 0);
    errno::set_raw(0);
    assert!(fails(s::fchownat(dir, cs("f").as_ptr(), 0, 0, 0x1) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_fdopendir() {
    let t = Tmp::new("fdopendir");
    t.file("f");
    let fd = io::liella_open3(cpath(&t.path).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let d = s::fdopendir(fd);
    assert!(!d.is_null());
    assert_eq!(s::dirfd(d), fd);
    assert_eq!(count_entries(d), 3);
    s::closedir(d);
    errno::set_raw(0);
    assert!(s::fdopendir(-1).is_null() && errno::get() == EBADF);
    let f = io::liella_open3(cpath(&t.at("f")).as_ptr(), O_RDONLY, 0);
    errno::set_raw(0);
    assert!(s::fdopendir(f).is_null() && errno::get() == ENOTDIR);
    io::close(f);
}

#[test]
fn sym_fdopendir_position_and_reused_buffer() {
    // WL-HOT-DU: fdopendir no longer asks for the position up front, and a closed stream's
    // buffer serves the next one. telldir before the first readdir is the descriptor's
    // position; a stream on a reused buffer sees only its own directory's entries.
    let t = Tmp::new("fdopendir_pos");
    for i in 0..40 {
        t.file(&format!("big{i:02}"));
    }
    t.dir("small");
    t.file("small/only");
    let big = cpath(&t.path);
    let d = s::opendir(big.as_ptr());
    assert!(!d.is_null());
    assert!(!s::readdir(d).is_null() && !s::readdir(d).is_null());
    let pos = s::telldir(d);
    assert!(pos > 0);
    let rest = count_entries(d);
    assert_eq!(rest, 43 - 2, "40 files, small, . and .., minus the two read");
    assert_eq!(s::closedir(d), 0);
    let fd = io::liella_open3(big.as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    assert_eq!(io::lseek(fd, pos, 0), pos);
    let d = s::fdopendir(fd);
    assert!(!d.is_null());
    assert_eq!(s::telldir(d), pos, "the descriptor's position before any readdir");
    assert_eq!(count_entries(d), rest, "continues where the descriptor was");
    assert_eq!(s::closedir(d), 0);
    // The big directory's records are still in the recycled buffer: the small one must not
    // show any of them.
    let fd = io::liella_open3(cpath(&t.at("small")).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let d = s::fdopendir(fd);
    assert!(!d.is_null());
    assert_eq!(s::telldir(d), 0);
    assert_eq!(count_entries(d), 3);
    assert_eq!(s::closedir(d), 0);
    let d = s::opendir(cpath(&t.at("small")).as_ptr());
    assert!(!d.is_null());
    assert_eq!(count_entries(d), 3);
    assert_eq!(s::closedir(d), 0);
}

#[test]
fn sym_fgetxattr() {
    let d = Dir::new("fgetxattr");
    let f = d.file("f", b"");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    errno::set_raw(0);
    assert!(fails(s::fgetxattr(fd, cs("user.k").as_ptr(), null_mut(), 0), ENODATA));
    io::close(fd);
    assert!(fails(s::fgetxattr(CLOSED, cs("user.k").as_ptr(), null_mut(), 0), EBADF));
}

#[test]
fn sym_flistxattr() {
    let d = Dir::new("flistxattr");
    let f = d.file("f", b"");
    s::setxattr(f.as_ptr(), cs("user.q").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut b = [0u8; 2];
    errno::set_raw(0);
    assert!(fails(s::flistxattr(fd, b.as_mut_ptr().cast(), 2), ERANGE), "too small");
    io::close(fd);
}

#[test]
fn sym_fremovexattr() {
    let d = Dir::new("fremovexattr");
    let f = d.file("f", b"");
    s::setxattr(f.as_ptr(), cs("user.k").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    assert_eq!(s::fremovexattr(fd, cs("user.k").as_ptr()), 0);
    io::close(fd);
}

#[test]
fn sym_fsconfig() {
    errno::set_raw(0);
    assert!(fails(s::fsconfig(-1, 99, null(), null(), 0) as isize, EINVAL), "an unknown command, before the kernel");
    let r = s::fsconfig(-1, 1, cs("size").as_ptr(), cs("1m").as_ptr().cast(), 0);
    assert!(r == -1 && [EBADF, EINVAL].contains(&errno::get()), "FSCONFIG_SET_STRING reaches the kernel");
    assert!(fails(s::fsconfig(-1, 2, cs("k").as_ptr(), b"xy".as_ptr().cast(), -1) as isize, EINVAL), "a negative binary length");
}

#[test]
fn sym_fsetxattr() {
    let d = Dir::new("fsetxattr");
    let f = d.file("f", b"");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    assert_eq!(s::fsetxattr(fd, cs("user.k").as_ptr(), b"vv".as_ptr().cast(), 2, 0), 0);
    let mut b = [0u8; 4];
    assert_eq!(s::fgetxattr(fd, cs("user.k").as_ptr(), b.as_mut_ptr().cast(), 4), 2);
    io::close(fd);
}

#[test]
fn sym_fsmount() {
    errno::set_raw(0);
    let r = s::fsmount(-1, 0, 0);
    assert!(r == -1 && [EBADF, EPERM].contains(&errno::get()));
}

#[test]
fn sym_fsopen() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    assert!(fails(s::fsopen(cs("tmpfs").as_ptr(), 0) as isize, EPERM));
}

#[test]
fn sym_fspick() {
    errno::set_raw(0);
    let r = s::fspick(AT_FDCWD, cs("/").as_ptr(), 0);
    assert!(r >= 0 || errno::get() == EPERM);
}

#[test]
fn sym_fstat64() {
    let d = Dir::new("fstat64");
    let f = d.file("f", b"abc");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::fstat64(fd, Some(&mut st)), 0);
    assert_eq!((st.mode & S_IFMT, st.size), (S_IFREG, 3));
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::fstat64(-1, Some(&mut st)) as isize, EBADF));
}

#[test]
fn sym_fstat() {
    let d = Dir::new("fstat");
    let f = d.file("f", b"abc");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::fstat(fd, Some(&mut st)), 0);
    assert_eq!((st.mode & S_IFMT, st.size), (S_IFREG, 3));
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::fstat(-1, Some(&mut st)) as isize, EBADF));
}

#[test]
fn sym_fstatat64() {
    let d = Dir::new("fstatat64");
    d.file("f", b"abcd");
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    let mut st = zstat();
    assert_eq!(s::fstatat64(dir, cs("l").as_ptr(), Some(&mut st), 0), 0);
    assert_eq!(st.size, 4);
    assert_eq!(s::fstatat64(dir, cs("l").as_ptr(), Some(&mut st), AT_SYMLINK_NOFOLLOW), 0);
    assert_eq!(st.mode & S_IFMT, S_IFLNK);
    assert_eq!(s::fstatat64(dir, cs("").as_ptr(), Some(&mut st), AT_EMPTY_PATH), 0);
    assert_eq!(st.mode & S_IFMT, S_IFDIR, "AT_EMPTY_PATH: the descriptor itself");
    errno::set_raw(0);
    assert!(fails(s::fstatat64(dir, cs("x").as_ptr(), Some(&mut st), 0x1) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_fstatat() {
    let d = Dir::new("fstatat");
    d.file("f", b"abcd");
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    let mut st = zstat();
    assert_eq!(s::fstatat(dir, cs("l").as_ptr(), Some(&mut st), 0), 0);
    assert_eq!(st.size, 4);
    assert_eq!(s::fstatat(dir, cs("l").as_ptr(), Some(&mut st), AT_SYMLINK_NOFOLLOW), 0);
    assert_eq!(st.mode & S_IFMT, S_IFLNK);
    assert_eq!(s::fstatat(dir, cs("").as_ptr(), Some(&mut st), AT_EMPTY_PATH), 0);
    assert_eq!(st.mode & S_IFMT, S_IFDIR, "AT_EMPTY_PATH: the descriptor itself");
    errno::set_raw(0);
    assert!(fails(s::fstatat(dir, cs("x").as_ptr(), Some(&mut st), 0x1) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_fstatfs() {
    let fd = io::liella_open3(cs("/proc").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut b = Statfs::default();
    assert_eq!(s::fstatfs(fd, Some(&mut b)), 0);
    assert_eq!(b.ftype, 0x9fa0);
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::fstatfs(CLOSED, Some(&mut b)) as isize, EBADF));
}

#[test]
fn sym_fstatfs64() {
    let fd = io::liella_open3(cs("/proc").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut b = Statfs::default();
    assert_eq!(s::fstatfs64(fd, Some(&mut b)), 0);
    assert_eq!(b.ftype, 0x9fa0);
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::fstatfs64(CLOSED, Some(&mut b)) as isize, EBADF));
}

#[test]
fn sym_fstatvfs() {
    let fd = io::liella_open3(cs("/proc").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut v = zero_statvfs();
    let mut f = Statfs::default();
    assert_eq!(s::fstatvfs(fd, Some(&mut v)), 0);
    assert_eq!(s::statfs(cs("/proc").as_ptr(), Some(&mut f)), 0);
    assert_eq!((v.f_bsize, v.f_blocks, v.f_namemax, u64::from(v.f_type)), (f.bsize as u64, f.blocks, f.namelen as u64, f.ftype as u64));
    assert_eq!(v.f_favail, v.f_ffree);
    assert_eq!(v.f_flag, (f.flags & !0x20) as u64, "f_flag is f_flags without ST_VALID");
    assert_ne!(v.f_flag & 0x2, 0, "/proc is mounted nosuid (ST_NOSUID)");
    assert_eq!(v.f_fsid, (i64::from(f.fsid[0]) as u64) | (u64::from(f.fsid[1] as u32) << 32));
    errno::set_raw(0);
    assert!(fails(s::fstatvfs(fd, None) as isize, EFAULT));
}

#[test]
fn sym_fstatvfs64() {
    let fd = io::liella_open3(cs("/proc").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut v = zero_statvfs();
    let mut f = Statfs::default();
    assert_eq!(s::fstatvfs64(fd, Some(&mut v)), 0);
    assert_eq!(s::statfs(cs("/proc").as_ptr(), Some(&mut f)), 0);
    assert_eq!((v.f_bsize, v.f_blocks, v.f_namemax, u64::from(v.f_type)), (f.bsize as u64, f.blocks, f.namelen as u64, f.ftype as u64));
    assert_eq!(v.f_favail, v.f_ffree);
    assert_eq!(v.f_flag, (f.flags & !0x20) as u64, "f_flag is f_flags without ST_VALID");
    assert_ne!(v.f_flag & 0x2, 0, "/proc is mounted nosuid (ST_NOSUID)");
    assert_eq!(v.f_fsid, (i64::from(f.fsid[0]) as u64) | (u64::from(f.fsid[1] as u32) << 32));
    errno::set_raw(0);
    assert!(fails(s::fstatvfs64(fd, None) as isize, EFAULT));
}

#[test]
fn sym_ftruncate() {
    let d = Dir::new("ftruncate");
    let p = d.file("f", b"0123456789");
    let fd = io::liella_open3(p.as_ptr(), O_RDWR, 0);
    assert_eq!(s::ftruncate(fd, 4), 0);
    assert_eq!(std::fs::read(p.to_str().unwrap()).unwrap(), b"0123");
    assert_eq!(s::ftruncate(fd, 8), 0);
    assert_eq!(std::fs::read(p.to_str().unwrap()).unwrap(), b"0123\0\0\0\0", "extension reads as zeros");
    errno::set_raw(0);
    assert!(fails(s::ftruncate(fd, -1) as isize, EINVAL));
    io::close(fd);
    let ro = io::liella_open3(p.as_ptr(), O_RDONLY, 0);
    assert!(fails(s::ftruncate(ro, 0) as isize, EINVAL), "not open for writing");
    io::close(ro);
}

#[test]
fn sym_ftruncate64() {
    let d = Dir::new("ftruncate64");
    let p = d.file("f", b"0123456789");
    let fd = io::liella_open3(p.as_ptr(), O_RDWR, 0);
    assert_eq!(s::ftruncate64(fd, 4), 0);
    assert_eq!(std::fs::read(p.to_str().unwrap()).unwrap(), b"0123");
    assert_eq!(s::ftruncate64(fd, 8), 0);
    assert_eq!(std::fs::read(p.to_str().unwrap()).unwrap(), b"0123\0\0\0\0", "extension reads as zeros");
    errno::set_raw(0);
    assert!(fails(s::ftruncate64(fd, -1) as isize, EINVAL));
    io::close(fd);
    let ro = io::liella_open3(p.as_ptr(), O_RDONLY, 0);
    assert!(fails(s::ftruncate64(ro, 0) as isize, EINVAL), "not open for writing");
    io::close(ro);
}

#[test]
fn sym_fts64_children() {
    let t = Tmp::new("fts64_children");
    t.file("f");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts64_open(av.as_ptr(), 16 | 4, None);
    assert!(!s::fts64_children(f, 0).is_null(), "before the first read: the roots");
    assert!(!s::fts64_read(f).is_null());
    let c = s::fts64_children(f, 0);
    assert!(!c.is_null());
    errno::set_raw(0);
    assert!(s::fts64_children(f, 5).is_null() && errno::get() == EINVAL, "an unknown option");
    s::fts64_close(f);
}

#[test]
fn sym_fts64_close() {
    let t = Tmp::new("fts64_close");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts64_open(av.as_ptr(), 16, None);
    assert!(!s::fts64_read(f).is_null());
    assert_eq!(s::fts64_close(f), 0);
    errno::set_raw(0);
    assert!(fails(s::fts64_close(null_mut()) as isize, EINVAL), "NULL");
}

#[test]
fn sym_fts64_open() {
    let t = Tmp::new("fts64_open");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts64_open(av.as_ptr(), 16 | 4, None);
    assert!(!f.is_null());
    assert_eq!(s::fts64_close(f), 0);
    let none: [*mut c_char; 1] = [null_mut()];
    errno::set_raw(0);
    assert!(s::fts64_open(none.as_ptr(), 16, None).is_null() && errno::get() == EINVAL, "no paths (D-13.13)");
    errno::set_raw(0);
    assert!(s::fts64_open(av.as_ptr(), 16 | 0x1000, None).is_null() && errno::get() == EINVAL, "an unknown option");
    let empty = [cs("")];
    let ev = argv(&empty);
    errno::set_raw(0);
    assert!(s::fts64_open(ev.as_ptr(), 16, None).is_null() && errno::get() == ENOENT, "an empty path");
}

#[test]
fn sym_fts64_read() {
    let t = Tmp::new("fts64_read");
    t.file("f");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts64_open(av.as_ptr(), 16 | 4, None);
    let mut infos = Vec::new();
    loop {
        let e = s::fts64_read(f);
        if e.is_null() {
            break;
        }
        infos.push(u16::from_ne_bytes(cbytes(e.cast_const(), 120)[98..100].try_into().unwrap()));
    }
    assert_eq!(infos, [1, 8, 6], "D, F, DP");
    errno::set_raw(EIO);
    assert!(s::fts64_read(f).is_null());
    assert_eq!(errno::get(), 0, "the end: NULL with errno 0 (fts(3))");
    s::fts64_close(f);
}

#[test]
fn sym_fts64_set() {
    let t = Tmp::new("fts64_set");
    t.dir("d");
    t.file("d/x");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts64_open(av.as_ptr(), 16 | 4, None);
    let root = s::fts64_read(f);
    assert_eq!(s::fts64_set(f, root, 4), 0, "FTS_SKIP");
    let next = s::fts64_read(f);
    assert_eq!(u16::from_ne_bytes(cbytes(next.cast_const(), 120)[98..100].try_into().unwrap()), 6, "skipped: the post-order entry next");
    errno::set_raw(0);
    assert!(fails(s::fts64_set(f, next, 7) as isize, EINVAL), "an invalid instruction: -1 (fts(3))");
    s::fts64_close(f);
}

#[test]
fn sym_fts_children() {
    let t = Tmp::new("fts_children");
    t.file("f");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts_open(av.as_ptr(), 16 | 4, None);
    assert!(!s::fts_children(f, 0).is_null(), "before the first read: the roots");
    assert!(!s::fts_read(f).is_null());
    let c = s::fts_children(f, 0);
    assert!(!c.is_null());
    errno::set_raw(0);
    assert!(s::fts_children(f, 5).is_null() && errno::get() == EINVAL, "an unknown option");
    s::fts_close(f);
}

#[test]
fn sym_fts_close() {
    let t = Tmp::new("fts_close");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts_open(av.as_ptr(), 16, None);
    assert!(!s::fts_read(f).is_null());
    assert_eq!(s::fts_close(f), 0);
    errno::set_raw(0);
    assert!(fails(s::fts_close(null_mut()) as isize, EINVAL), "NULL");
}

#[test]
fn sym_fts_open() {
    let t = Tmp::new("fts_open");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts_open(av.as_ptr(), 16 | 4, None);
    assert!(!f.is_null());
    assert_eq!(s::fts_close(f), 0);
    let none: [*mut c_char; 1] = [null_mut()];
    errno::set_raw(0);
    assert!(s::fts_open(none.as_ptr(), 16, None).is_null() && errno::get() == EINVAL, "no paths (D-13.13)");
    errno::set_raw(0);
    assert!(s::fts_open(av.as_ptr(), 16 | 0x1000, None).is_null() && errno::get() == EINVAL, "an unknown option");
    let empty = [cs("")];
    let ev = argv(&empty);
    errno::set_raw(0);
    assert!(s::fts_open(ev.as_ptr(), 16, None).is_null() && errno::get() == ENOENT, "an empty path");
}

#[test]
fn sym_fts_read() {
    let t = Tmp::new("fts_read");
    t.file("f");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts_open(av.as_ptr(), 16 | 4, None);
    let mut infos = Vec::new();
    loop {
        let e = s::fts_read(f);
        if e.is_null() {
            break;
        }
        infos.push(u16::from_ne_bytes(cbytes(e.cast_const(), 120)[98..100].try_into().unwrap()));
    }
    assert_eq!(infos, [1, 8, 6], "D, F, DP");
    errno::set_raw(EIO);
    assert!(s::fts_read(f).is_null());
    assert_eq!(errno::get(), 0, "the end: NULL with errno 0 (fts(3))");
    s::fts_close(f);
}

#[test]
fn sym_fts_set() {
    let t = Tmp::new("fts_set");
    t.dir("d");
    t.file("d/x");
    let args = [cpath(&t.path)];
    let av = argv(&args);
    let f = s::fts_open(av.as_ptr(), 16 | 4, None);
    let root = s::fts_read(f);
    assert_eq!(s::fts_set(f, root, 4), 0, "FTS_SKIP");
    let next = s::fts_read(f);
    assert_eq!(u16::from_ne_bytes(cbytes(next.cast_const(), 120)[98..100].try_into().unwrap()), 6, "skipped: the post-order entry next");
    errno::set_raw(0);
    assert!(fails(s::fts_set(f, next, 7) as isize, EINVAL), "an invalid instruction: -1 (fts(3))");
    s::fts_close(f);
}

#[test]
fn sym_ftw() {
    let t = Tmp::new("ftw");
    t.dir("d");
    t.file("d/f");
    CALLS.set(0);
    assert_eq!(s::ftw(cpath(&t.path).as_ptr(), Some(count_ftw), 4), 0);
    assert_eq!(CALLS.get(), 3, "the root, d and d/f");
    errno::set_raw(0);
    assert!(fails(s::ftw(cpath(&t.at("missing")).as_ptr(), Some(count_ftw), 4) as isize, ENOENT));
}

#[test]
fn sym_ftw64() {
    let t = Tmp::new("ftw64");
    t.file("f");
    CALLS.set(0);
    assert_eq!(s::ftw64(cpath(&t.path).as_ptr(), Some(count_ftw), 4), 0);
    assert_eq!(CALLS.get(), 2);
}

#[test]
fn sym_futimens() {
    let d = Dir::new("futimens");
    let f = d.file("f", b"");
    let fd = io::liella_open3(f.as_ptr(), O_RDWR, 0);
    let ts = [Timespec { sec: 9, nsec: 9 }, Timespec { sec: 10, nsec: 10 }];
    // Not a descriptor: EBADF, also for AT_FDCWD (POSIX; the reference; validation T9).
    for bad in [-1, -100] {
        errno::set_raw(0);
        assert_eq!((s::futimens(bad, None), errno::get()), (-1, 9), "{bad}");
    }
    assert_eq!(s::futimens(fd, Some(&ts)), 0);
    let mut st = zstat();
    s::fstat(fd, Some(&mut st));
    assert_eq!((st.mtime.sec, st.mtime.nsec), (10, 10));
    assert_eq!(s::futimens(fd, None), 0, "NULL: now");
    io::close(fd);
}

#[test]
fn sym___fxstat64() {
    let d = Dir::new("__fxstat64");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__fxstat64(1, fd, Some(&mut st)), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__fxstat64(0, fd, Some(&mut st)), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__fxstat64(3, fd, Some(&mut st)) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym___fxstat() {
    let d = Dir::new("__fxstat");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__fxstat(1, fd, Some(&mut st)), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__fxstat(0, fd, Some(&mut st)), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__fxstat(3, fd, Some(&mut st)) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym___fxstatat64() {
    let d = Dir::new("__fxstatat64");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__fxstatat64(1, AT_FDCWD, f.as_ptr(), Some(&mut st), 0), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__fxstatat64(0, AT_FDCWD, f.as_ptr(), Some(&mut st), 0), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__fxstatat64(3, AT_FDCWD, f.as_ptr(), Some(&mut st), 0) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym___fxstatat() {
    let d = Dir::new("__fxstatat");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__fxstatat(1, AT_FDCWD, f.as_ptr(), Some(&mut st), 0), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__fxstatat(0, AT_FDCWD, f.as_ptr(), Some(&mut st), 0), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__fxstatat(3, AT_FDCWD, f.as_ptr(), Some(&mut st), 0) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym_get_current_dir_name() {
    // A new block naming the working directory ($PWD when it does, else the physical path).
    let p = ctext(s::get_current_dir_name());
    assert_eq!(p.first(), Some(&b'/'));
    let (mut a, mut b) = (zstat(), zstat());
    assert_eq!(s::stat(CString::new(p).unwrap().as_ptr(), Some(&mut a)), 0);
    assert_eq!(s::stat(cs(".").as_ptr(), Some(&mut b)), 0);
    assert_eq!((a.dev, a.ino), (b.dev, b.ino));
}

#[test]
fn sym_getcwd() {
    let k = kernel_cwd();
    let mut b = vec![0 as c_char; 8192];
    assert_eq!(s::getcwd(b.as_mut_ptr(), b.len()), b.as_mut_ptr());
    assert_eq!(ctext(b.as_ptr()), k);
    assert_eq!(ctext(s::getcwd(null_mut(), 0)), k, "allocated");
    errno::set_raw(0);
    assert!(s::getcwd(b.as_mut_ptr(), 0).is_null() && errno::get() == EINVAL, "size 0 with a buffer");
    errno::set_raw(0);
    assert!(s::getcwd(b.as_mut_ptr(), 2).is_null() && errno::get() == ERANGE);
}

#[test]
fn sym___getcwd_chk() {
    if std::env::var("LIELLA_S13_CHK").is_ok() {
        let mut b = [0 as c_char; 2];
        s::__getcwd_chk(b.as_mut_ptr(), 4096, 2);
        return;
    }
    let k = kernel_cwd();
    let mut b = vec![0 as c_char; 4096];
    assert_eq!(ctext(s::__getcwd_chk(b.as_mut_ptr(), 4096, 4096)), k);
    errno::set_raw(0);
    assert!(s::__getcwd_chk(b.as_mut_ptr(), 2, 4096).is_null() && errno::get() == ERANGE, "a size too small: ERANGE");
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args(["sym___getcwd_chk", "--exact", "--test-threads=1"]).env("LIELLA_S13_CHK", "1").output().unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "a path written beyond the object: the fortify abort");
    assert!(String::from_utf8_lossy(&o.stderr).contains("buffer overflow detected"));
}

#[test]
fn sym_getdents64() {
    let t = Tmp::new("getdents64");
    let fd = io::liella_open3(cpath(&t.path).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut b = vec![0u8; 1024];
    errno::set_raw(0);
    assert!(fails(s::getdents64(fd, b.as_mut_ptr().cast(), 1), EINVAL), "too small for the next record");
    let n = s::getdents64(fd, b.as_mut_ptr().cast(), b.len());
    assert!(n >= 48, "two records");
    assert_eq!(s::getdents64(fd, b.as_mut_ptr().cast(), b.len()), 0, "the end");
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::getdents64(-1, b.as_mut_ptr().cast(), b.len()), EBADF));
}

#[test]
fn sym_getdirentries() {
    let t = Tmp::new("getdirentries");
    let fd = io::liella_open3(cpath(&t.path).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut b = vec![0u8; 1024];
    let mut base = 99i64;
    assert!(s::getdirentries(fd, b.as_mut_ptr().cast(), b.len(), Some(&mut base)) > 0);
    assert_eq!(base, 0, "the position before the read");
    assert_eq!(s::getdirentries(fd, b.as_mut_ptr().cast(), b.len(), Some(&mut base)), 0);
    assert_ne!(base, 0, "now at the end");
    io::close(fd);
}

#[test]
fn sym_getdirentries64() {
    let t = Tmp::new("getdirentries64");
    let fd = io::liella_open3(cpath(&t.path).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut b = vec![0u8; 1024];
    let mut base = 99i64;
    assert!(s::getdirentries64(fd, b.as_mut_ptr().cast(), b.len(), Some(&mut base)) > 0);
    assert_eq!(base, 0);
    errno::set_raw(0);
    assert!(fails(s::getdirentries64(fd, b.as_mut_ptr().cast(), b.len(), None), EFAULT), "NULL basep");
    io::close(fd);
}

#[test]
fn sym_getfsent() {
    let t = Tmp::new("getfsent");
    in_child("sym_getfsent", &t, |_| {
        let want = fstab_entries();
        let mut got = Vec::new();
        loop {
            errno::set_raw(0);
            match show_fs(s::getfsent()) {
                Some((e, ty)) => {
                    assert!(["rw", "rq", "ro", "sw", "xx", "??"].contains(&ty.as_str()), "fs_type {ty}");
                    got.push(e);
                }
                None => break,
            }
        }
        if std::path::Path::new("/etc/fstab").is_file() {
            assert_eq!(errno::get(), 0, "the end leaves errno alone");
        } else {
            assert_eq!(errno::get(), ENOENT, "no /etc/fstab");
        }
        assert_eq!(got, want, "the entries getmntent reads");
        s::endfsent();
    });
}

/// `getfsspec` (`n` 0) or `getfsfile` (`n` 1) on the first entry of `/etc/fstab` with a
/// non-empty field, a missing key and NULL. Lookups do not touch the enumeration's cursor.
fn lookup_case(look: extern "C" fn(*const c_char) -> *mut liella_abi::layout::struct_fstab, n: usize) {
    let all = fstab_entries();
    if let Some(e) = all.iter().find(|e| !field(e, n).is_empty()) {
        let key = cs(&field(e, n));
        assert_eq!(show_fs(look(key.as_ptr())).map(|v| v.0).as_ref(), Some(e), "the first entry with that field");
    }
    errno::set_raw(0);
    assert!(look(c"/liella/no/such/entry".as_ptr()).is_null() && errno::get() == 0, "not found: errno unchanged");
    assert!(look(null()).is_null() && errno::get() == EFAULT, "a NULL key");
}

#[test]
fn sym_getfsfile() {
    lookup_case(s::getfsfile, 1);
}

#[test]
fn sym_getfsspec() {
    lookup_case(s::getfsspec, 0);
}

#[test]
fn sym_getmntent() {
    let d = Dir::new("getmntent");
    let path = d.file("t", b"# c\n\n/dev/a\\040b /m ext4 rw,noatime 1 2\nx /y\n");
    let f = mopen(&path, "r");
    let a = s::getmntent(f);
    assert_eq!(show(a), "[/dev/a b] [/m] [ext4] [rw,noatime] 1 2");
    let b = s::getmntent(f);
    assert_eq!(a, b, "the thread's block, overwritten");
    assert_eq!(show(a), "[x] [/y] [] [] 0 0");
    errno::set_raw(0);
    assert!(s::getmntent(f).is_null() && errno::get() == 0, "the end");
    s::endmntent(f);
    assert!(s::getmntent(null_mut()).is_null() && errno::get() == EINVAL, "a NULL stream");
}

type GetR = extern "C" fn(*mut liella_abi::layout::FILE, Option<&mut liella_abi::layout::struct_mntent>, *mut c_char, c_int) -> *mut liella_abi::layout::struct_mntent;

fn getmntent_r_case(get: GetR, tag: &str) {
    let d = Dir::new(tag);
    let path = d.file("t", b"src /dir t o 3 4\n");
    let f = mopen(&path, "r");
    let mut me = zero_ment();
    let mut buf = [0 as c_char; 64];
    errno::set_raw(0);
    assert!(get(f, Some(&mut me), buf.as_mut_ptr(), 12).is_null() && errno::get() == ERANGE, "12 bytes cannot hold the strings (13 needed)");
    assert!(get(f, Some(&mut me), buf.as_mut_ptr(), -1).is_null() && errno::get() == EINVAL, "a negative size");
    assert!(get(f, None, buf.as_mut_ptr(), 64).is_null() && errno::get() == EFAULT, "a NULL mntbuf");
    let r = get(f, Some(&mut me), buf.as_mut_ptr(), 13);
    assert_eq!(r, core::ptr::from_mut(&mut me), "the retry returns the line, in the caller's structure");
    assert_eq!(show(r), "[src] [/dir] [t] [o] 3 4");
    let at = buf.as_ptr().expose_provenance();
    assert_eq!((me.mnt_fsname, me.mnt_dir, me.mnt_type, me.mnt_opts), (at, at + 4, at + 9, at + 11), "the strings in the buffer");
    errno::set_raw(0);
    assert!(get(f, Some(&mut me), buf.as_mut_ptr(), 64).is_null() && errno::get() == 0, "the end");
    s::endmntent(f);
}

#[test]
fn sym___getmntent_r() {
    getmntent_r_case(s::__getmntent_r, "__getmntent_r");
}

#[test]
fn sym_getmntent_r() {
    getmntent_r_case(s::getmntent_r, "getmntent_r");
}
#[test]
fn sym_getwd() {
    let mut b = vec![0 as c_char; 4096];
    assert_eq!(s::getwd(b.as_mut_ptr()), b.as_mut_ptr());
    assert_eq!(ctext(b.as_ptr()), kernel_cwd());
    errno::set_raw(0);
    assert!(s::getwd(null_mut()).is_null() && errno::get() == EINVAL, "NULL (getwd(3))");
}

#[test]
fn sym___getwd_chk() {
    if std::env::var("LIELLA_S13_CHK").is_ok() {
        let mut b = [0 as c_char; 2];
        s::__getwd_chk(b.as_mut_ptr(), 2);
        return;
    }
    let mut b = vec![0 as c_char; 4096];
    assert_eq!(ctext(s::__getwd_chk(b.as_mut_ptr(), 4096)), kernel_cwd());
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args(["sym___getwd_chk", "--exact", "--test-threads=1"]).env("LIELLA_S13_CHK", "1").output().unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "the fortify abort");
}

#[test]
fn sym_getxattr() {
    let d = Dir::new("getxattr");
    let f = d.file("f", b"");
    s::setxattr(f.as_ptr(), cs("user.k").as_ptr(), b"12345".as_ptr().cast(), 5, 0);
    assert_eq!(s::getxattr(f.as_ptr(), cs("user.k").as_ptr(), null_mut(), 0), 5, "size 0 asks the size");
    let mut b = [0u8; 5];
    assert_eq!(s::getxattr(f.as_ptr(), cs("user.k").as_ptr(), b.as_mut_ptr().cast(), 5), 5);
    assert_eq!(&b, b"12345");
    errno::set_raw(0);
    assert!(fails(s::getxattr(f.as_ptr(), cs("user.k").as_ptr(), b.as_mut_ptr().cast(), 2), ERANGE));
    assert!(fails(s::getxattr(f.as_ptr(), cs("user.none").as_ptr(), b.as_mut_ptr().cast(), 5), ENODATA));
}

#[test]
fn sym_gnu_dev_major() {
    assert_eq!(s::gnu_dev_major(s::gnu_dev_makedev(8, 1)), 8);
    assert_eq!(s::gnu_dev_major(s::gnu_dev_makedev(0xfff_ffff, 0)), 0xfff_ffff, "majors beyond 12 bits");
    let mut st = zstat();
    s::stat(cs("/dev/null").as_ptr(), Some(&mut st));
    assert_eq!(s::gnu_dev_major(st.rdev), 1, "/dev/null is (1, 3)");
}

#[test]
fn sym_gnu_dev_makedev() {
    assert_eq!(s::gnu_dev_makedev(1, 3), 0x103, "the kernel's encoding for small numbers");
    assert_eq!(s::gnu_dev_makedev(0x1000, 0x100), 0x0000_1000_0010_0000, "glibc's 64-bit layout: major above bit 44, minor from bit 20");
    for (ma, mi) in [(0, 0), (259, 7), (0xffff_ffff, 0xffff_ffff)] {
        let d = s::gnu_dev_makedev(ma, mi);
        assert_eq!((s::gnu_dev_major(d), s::gnu_dev_minor(d)), (ma, mi));
    }
}

#[test]
fn sym_gnu_dev_minor() {
    assert_eq!(s::gnu_dev_minor(s::gnu_dev_makedev(8, 0x12345)), 0x12345, "minors beyond 8 bits");
    let mut st = zstat();
    s::stat(cs("/dev/null").as_ptr(), Some(&mut st));
    assert_eq!(s::gnu_dev_minor(st.rdev), 3);
}

#[test]
fn sym_hasmntopt() {
    let (a, o) = (cs("a"), cs("rw,size=10,noro,ro"));
    let m = ment([&a, &a, &a, &o], 0, 0);
    let base = o.as_ptr().cast_mut();
    assert_eq!(s::hasmntopt(Some(&m), c"ro".as_ptr()), base.wrapping_add(16), "a whole option, not a suffix");
    assert_eq!(s::hasmntopt(Some(&m), c"size".as_ptr()), base.wrapping_add(3), "by name");
    assert_eq!(s::hasmntopt(Some(&m), c"size=10".as_ptr()), base.wrapping_add(3), "exactly");
    errno::set_raw(0);
    assert!(s::hasmntopt(Some(&m), c"size=1".as_ptr()).is_null() && errno::get() == 0);
    assert!(s::hasmntopt(Some(&m), c"rw,size=10".as_ptr()).is_null(), "a comma names no option");
    assert!(s::hasmntopt(Some(&m), null()).is_null() && errno::get() == EINVAL, "a NULL query");
    assert!(s::hasmntopt(None, c"ro".as_ptr()).is_null() && errno::get() == EINVAL, "a NULL entry");
}
#[test]
fn sym_lchmod() {
    let d = Dir::new("lchmod");
    let f = d.file("f", b"");
    let l = d.at("l");
    std::os::unix::fs::symlink("f", l.to_str().unwrap()).unwrap();
    assert_eq!(s::lchmod(f.as_ptr(), 0o633), 0, "a file: changed");
    assert_eq!(mode_of(&f) & 0o777, 0o633);
    errno::set_raw(0);
    assert!(fails(s::lchmod(l.as_ptr(), 0o600) as isize, EOPNOTSUPP), "a symlink: EOPNOTSUPP");
    assert_eq!(mode_of(&f) & 0o777, 0o633, "and its target untouched");
}

#[test]
fn sym_lchown() {
    let d = Dir::new("lchown");
    let f = d.file("f", b"");
    let mut st = zstat();
    s::stat(f.as_ptr(), Some(&mut st));
    assert_eq!(s::lchown(f.as_ptr(), st.uid, st.gid), 0, "to the current owner: allowed");
    assert_eq!(s::lchown(f.as_ptr(), u32::MAX, u32::MAX), 0, "-1 leaves both unchanged");
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::lchown(f.as_ptr(), 0, u32::MAX) as isize, EPERM), "giving the file away");
    }
}

#[test]
fn sym_lgetxattr() {
    let d = Dir::new("lgetxattr");
    let f = d.file("f", b"");
    let l = d.at("l");
    std::os::unix::fs::symlink("f", l.to_str().unwrap()).unwrap();
    s::setxattr(f.as_ptr(), cs("user.k").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    errno::set_raw(0);
    assert!(fails(s::lgetxattr(l.as_ptr(), cs("user.k").as_ptr(), null_mut(), 0), ENODATA), "the link has none");
    assert_eq!(s::getxattr(l.as_ptr(), cs("user.k").as_ptr(), null_mut(), 0), 1, "getxattr follows");
}

#[test]
fn sym_link() {
    let d = Dir::new("link");
    let a = d.file("a", b"x");
    let b = d.at("b");
    assert_eq!(s::link(a.as_ptr(), b.as_ptr()), 0);
    let (mut sa, mut sb) = (zstat(), zstat());
    s::stat(a.as_ptr(), Some(&mut sa));
    s::stat(b.as_ptr(), Some(&mut sb));
    assert_eq!((sa.ino, sa.nlink), (sb.ino, 2), "a second name for the same inode");
    errno::set_raw(0);
    assert!(fails(s::link(a.as_ptr(), b.as_ptr()) as isize, EEXIST));
    assert!(fails(s::link(d.at("none").as_ptr(), d.at("c").as_ptr()) as isize, ENOENT));
}

#[test]
fn sym_linkat() {
    let d = Dir::new("linkat");
    let a = d.file("a", b"x");
    let l = d.at("l");
    std::os::unix::fs::symlink(a.to_str().unwrap(), l.to_str().unwrap()).unwrap();
    let dir = d.open();
    assert_eq!(s::linkat(dir, cs("l").as_ptr(), dir, cs("hard-to-link").as_ptr(), 0), 0);
    assert_eq!(mode_of(&d.at("hard-to-link")) & S_IFMT, S_IFLNK, "without AT_SYMLINK_FOLLOW: the link itself");
    assert_eq!(s::linkat(dir, cs("l").as_ptr(), dir, cs("hard-to-file").as_ptr(), 0x400), 0);
    assert_eq!(mode_of(&d.at("hard-to-file")) & S_IFMT, S_IFREG, "AT_SYMLINK_FOLLOW");
    errno::set_raw(0);
    assert!(fails(s::linkat(dir, cs("a").as_ptr(), dir, cs("x").as_ptr(), 0x1) as isize, EINVAL), "an unknown flag");
    io::close(dir);
}

#[test]
fn sym_listxattr() {
    let d = Dir::new("listxattr");
    let f = d.file("f", b"");
    s::setxattr(f.as_ptr(), cs("user.a").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    s::setxattr(f.as_ptr(), cs("user.bb").as_ptr(), b"2".as_ptr().cast(), 1, 0);
    let n = s::listxattr(f.as_ptr(), null_mut(), 0);
    assert!(n >= 15, "size 0 asks the size");
    let mut b = vec![0u8; n as usize];
    assert_eq!(s::listxattr(f.as_ptr(), b.as_mut_ptr().cast(), b.len()), n);
    let names: Vec<&[u8]> = b.split(|&c| c == 0).filter(|s| s.starts_with(b"user.")).collect();
    assert!(names.contains(&&b"user.a"[..]) && names.contains(&&b"user.bb"[..]), "concatenated NUL-terminated names");
}

#[test]
fn sym_llistxattr() {
    let d = Dir::new("llistxattr");
    d.file("f", b"");
    let l = d.at("l");
    std::os::unix::fs::symlink("f", l.to_str().unwrap()).unwrap();
    s::setxattr(d.at("f").as_ptr(), cs("user.a").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    let mut b = [0u8; 256];
    let n = s::llistxattr(l.as_ptr(), b.as_mut_ptr().cast(), 256);
    assert!(n >= 0);
    assert!(!b[..n as usize].windows(6).any(|w| w == b"user.a"), "the link's own list");
}

#[test]
fn sym_lremovexattr() {
    let d = Dir::new("lremovexattr");
    d.file("f", b"");
    let l = d.at("l");
    std::os::unix::fs::symlink("f", l.to_str().unwrap()).unwrap();
    s::setxattr(l.as_ptr(), cs("user.k").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    errno::set_raw(0);
    assert!(fails(s::lremovexattr(l.as_ptr(), cs("user.k").as_ptr()) as isize, EPERM), "user.* on a symlink itself is not writable");
    assert_eq!(s::removexattr(l.as_ptr(), cs("user.k").as_ptr()), 0);
}

#[test]
fn sym_lsetxattr() {
    let d = Dir::new("lsetxattr");
    let f = d.file("f", b"");
    let l = d.at("l");
    std::os::unix::fs::symlink("f", l.to_str().unwrap()).unwrap();
    errno::set_raw(0);
    assert!(fails(s::lsetxattr(l.as_ptr(), cs("user.k").as_ptr(), b"v".as_ptr().cast(), 1, 0) as isize, EPERM), "user.* on a symlink itself");
    assert_eq!(s::lsetxattr(f.as_ptr(), cs("user.k").as_ptr(), b"v".as_ptr().cast(), 1, 0), 0);
}

#[test]
fn sym_lstat64() {
    let d = Dir::new("lstat64");
    let l = d.at("l");
    std::os::unix::fs::symlink("target-text", l.to_str().unwrap()).unwrap();
    let mut st = zstat();
    assert_eq!(s::lstat64(l.as_ptr(), Some(&mut st)), 0);
    assert_eq!((st.mode & S_IFMT, st.size), (S_IFLNK, 11), "the link itself; its size is the target's length");
}

#[test]
fn sym_lstat() {
    let d = Dir::new("lstat");
    let l = d.at("l");
    std::os::unix::fs::symlink("target-text", l.to_str().unwrap()).unwrap();
    let mut st = zstat();
    assert_eq!(s::lstat(l.as_ptr(), Some(&mut st)), 0);
    assert_eq!((st.mode & S_IFMT, st.size), (S_IFLNK, 11), "the link itself; its size is the target's length");
}

#[test]
fn sym___lxstat64() {
    let d = Dir::new("__lxstat64");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__lxstat64(1, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__lxstat64(0, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__lxstat64(3, f.as_ptr(), Some(&mut st)) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym___lxstat() {
    let d = Dir::new("__lxstat");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__lxstat(1, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__lxstat(0, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__lxstat(3, f.as_ptr(), Some(&mut st)) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym_mkdir() {
    let d = Dir::new("mkdir");
    let sub = d.at("sub");
    assert_eq!(s::mkdir(sub.as_ptr(), 0o700), 0);
    assert_eq!(mode_of(&sub) & (S_IFMT | 0o777), S_IFDIR | 0o700);
    errno::set_raw(0);
    assert!(fails(s::mkdir(sub.as_ptr(), 0o700) as isize, EEXIST));
    assert!(fails(s::mkdir(d.at("x/y").as_ptr(), 0o700) as isize, ENOENT));
}

#[test]
fn sym_mkdirat() {
    let d = Dir::new("mkdirat");
    let dir = d.open();
    assert_eq!(s::mkdirat(dir, cs("sub").as_ptr(), 0o755), 0);
    assert_eq!(mode_of(&d.at("sub")) & S_IFMT, S_IFDIR);
    io::close(dir);
}

#[test]
fn sym_mkdtemp() {
    let t = Tmp::new("mkdtemp");
    let mut b = template(&t, "aXXXXXX");
    let r = s::mkdtemp(b.as_mut_ptr());
    assert_eq!(r, b.as_mut_ptr(), "the template itself");
    assert!(replaced(&b));
    assert!(std::fs::metadata(String::from_utf8(ctext(b.as_ptr())).unwrap()).unwrap().is_dir());
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkdtemp(b.as_mut_ptr()) };
    assert!(r2.is_null(), "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkfifo() {
    let d = Dir::new("mkfifo");
    let p = d.at("fifo");
    assert_eq!(s::mkfifo(p.as_ptr(), 0o600), 0);
    assert_eq!(mode_of(&p) & (S_IFMT | 0o777), S_IFIFO | 0o600);
    errno::set_raw(0);
    assert!(fails(s::mkfifo(p.as_ptr(), 0o600) as isize, EEXIST));
}

#[test]
fn sym_mkfifoat() {
    let d = Dir::new("mkfifoat");
    let dir = d.open();
    assert_eq!(s::mkfifoat(dir, cs("fifo").as_ptr(), S_IFREG | 0o600), 0, "type bits in mode are replaced");
    assert_eq!(mode_of(&d.at("fifo")) & S_IFMT, S_IFIFO);
    io::close(dir);
}

#[test]
fn sym_mknod() {
    let d = Dir::new("mknod");
    let p = d.at("node");
    assert_eq!(s::mknod(p.as_ptr(), S_IFIFO | 0o600, 0u64), 0, "a FIFO");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFIFO);
    let p = d.at("reg");
    assert_eq!(s::mknod(p.as_ptr(), S_IFREG | 0o600, 0u64), 0, "a regular file");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFREG);
    let p = d.at("dev");
    errno::set_raw(0);
    let big = 1u64 << 40;
    assert!(fails(s::mknod(p.as_ptr(), 0o020000 | 0o600, big) as isize, EINVAL), "a device number beyond 32 bits");
    let r = s::mknod(p.as_ptr(), 0o020000 | 0o600, 0x0103u64);
    assert!(r == 0 || (!root() && errno::get() == EPERM), "a character device needs privilege");
}

#[test]
fn sym_mknodat() {
    let d = Dir::new("mknodat");
    let p = d.at("node");
    assert_eq!(s::mknodat(AT_FDCWD, p.as_ptr(), S_IFIFO | 0o600, 0u64), 0, "a FIFO");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFIFO);
    let p = d.at("reg");
    assert_eq!(s::mknodat(AT_FDCWD, p.as_ptr(), S_IFREG | 0o600, 0u64), 0, "a regular file");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFREG);
    let p = d.at("dev");
    errno::set_raw(0);
    let big = 1u64 << 40;
    assert!(fails(s::mknodat(AT_FDCWD, p.as_ptr(), 0o020000 | 0o600, big) as isize, EINVAL), "a device number beyond 32 bits");
    let r = s::mknodat(AT_FDCWD, p.as_ptr(), 0o020000 | 0o600, 0x0103u64);
    assert!(r == 0 || (!root() && errno::get() == EPERM), "a character device needs privilege");
}

#[test]
fn sym_mkostemp64() {
    let t = Tmp::new("mkostemp64");
    let mut b = template(&t, "aXXXXXX");
    let r = s::mkostemp64(b.as_mut_ptr(), O_CLOEXEC);
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkostemp64(b.as_mut_ptr(), O_CLOEXEC) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkostemp() {
    let t = Tmp::new("mkostemp");
    let mut b = template(&t, "aXXXXXX");
    let r = s::mkostemp(b.as_mut_ptr(), O_CLOEXEC);
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkostemp(b.as_mut_ptr(), O_CLOEXEC) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkostemps64() {
    let t = Tmp::new("mkostemps64");
    let mut b = template(&t, "aXXXXXX.sfx");
    let r = s::mkostemps64(b.as_mut_ptr(), 4, O_CLOEXEC);
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkostemps64(b.as_mut_ptr(), 4, O_CLOEXEC) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkostemps() {
    let t = Tmp::new("mkostemps");
    let mut b = template(&t, "aXXXXXX.sfx");
    let r = s::mkostemps(b.as_mut_ptr(), 4, O_CLOEXEC);
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkostemps(b.as_mut_ptr(), 4, O_CLOEXEC) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkstemp() {
    let t = Tmp::new("mkstemp");
    let mut b = template(&t, "aXXXXXX");
    let r = s::mkstemp(b.as_mut_ptr());
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkstemp(b.as_mut_ptr()) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkstemp_deep_template() {
    // Trace: s13_9_c_1. A template about 2 KiB long, 60 directories (32-byte names) down, as
    // the `deep` stress profile has (MKSTEMP-LONG): only the six X change, the file is new,
    // mode 0600, and the descriptor is read-write.
    let t = Tmp::new("mkstemp-deep");
    let deep: String = (0..60).map(|i| format!("{i:032}/")).collect();
    std::fs::create_dir_all(t.at(&deep)).unwrap();
    let mut b = template(&t, &format!("{deep}aXXXXXX"));
    let before = b.clone();
    assert!(b.len() > 2000, "{}", b.len());
    let r = s::mkstemp(b.as_mut_ptr());
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    let n = b.len();
    assert_eq!(b[..n - 8], before[..n - 8], "the directories are unchanged");
    assert_eq!(b[n - 1], 0);
    let name = std::path::PathBuf::from(String::from_utf8(ctext(b.as_ptr())).unwrap());
    let md = std::fs::symlink_metadata(&name).unwrap();
    assert!(md.is_file());
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&md.permissions()) & 0o777, 0o600);
    assert_eq!(io::write(r, b"x".as_ptr().cast(), 1), 1);
    io::close(r);
    std::fs::remove_file(&name).unwrap();
}

#[test]
fn sym_mkstemp64() {
    let t = Tmp::new("mkstemp64");
    let mut b = template(&t, "aXXXXXX");
    let r = s::mkstemp64(b.as_mut_ptr());
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkstemp64(b.as_mut_ptr()) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkstemps64() {
    let t = Tmp::new("mkstemps64");
    let mut b = template(&t, "aXXXXXX.sfx");
    let r = s::mkstemps64(b.as_mut_ptr(), 4);
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkstemps64(b.as_mut_ptr(), 4) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mkstemps() {
    let t = Tmp::new("mkstemps");
    let mut b = template(&t, "aXXXXXX.sfx");
    let r = s::mkstemps(b.as_mut_ptr(), 4);
    assert!(r >= 0, "{r}");
    assert!(replaced(&b));
    io::close(r);
    errno::set_raw(0);
    let mut bad = template(&t, "aXXXXX");
    let before = bad.clone();
    let r2 = { let b = &mut bad; s::mkstemps(b.as_mut_ptr(), 4) };
    assert!(r2 == -1, "a bad template");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(bad, before, "unchanged");
}

#[test]
fn sym_mktemp() {
    let t = Tmp::new("mktemp");
    let mut b = template(&t, "aXXXXXX");
    assert_eq!(s::mktemp(b.as_mut_ptr()), b.as_mut_ptr());
    assert!(replaced(&b));
    assert!(std::fs::symlink_metadata(String::from_utf8(ctext(b.as_ptr())).unwrap()).is_err(), "nothing created");
    let mut bad = template(&t, "aXXXXX");
    errno::set_raw(0);
    assert_eq!(s::mktemp(bad.as_mut_ptr()), bad.as_mut_ptr());
    assert_eq!((bad[0], errno::get()), (0, EINVAL), "emptied, with errno");
}

#[test]
fn sym_mount() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    assert!(fails(s::mount(cs("none").as_ptr(), cs("/mnt").as_ptr(), cs("tmpfs").as_ptr(), 0, cs("size=1m").as_ptr().cast()) as isize, EPERM));
    assert!(fails(s::mount(null(), cs("/mnt").as_ptr(), null(), 0x1000 | 0x4000, null()) as isize, EPERM), "a bind remount: NULL source, type and data");
}

#[test]
fn sym_mount_setattr() {
    let attr = [0u64; 4];
    if kernel::before(5, 12) {
        kernel::note("mount_setattr", "5.12");
        errno::set_raw(0);
        assert!(fails(s::mount_setattr(AT_FDCWD, cs("/").as_ptr(), 0, attr.as_ptr().cast_mut().cast(), 32) as isize, 38), "ENOSYS");
        return;
    }
    errno::set_raw(0);
    assert!(fails(s::mount_setattr(AT_FDCWD, cs("/").as_ptr(), 0, attr.as_ptr().cast_mut().cast(), 16) as isize, EINVAL), "shorter than the first version");
    let long = [0u64, 0, 0, 0, 1];
    assert!(fails(s::mount_setattr(AT_FDCWD, cs("/").as_ptr(), 0, long.as_ptr().cast_mut().cast(), 40) as isize, 7), "unknown nonzero tail: E2BIG");
    let r = s::mount_setattr(AT_FDCWD, cs("/").as_ptr(), 0, attr.as_ptr().cast_mut().cast(), 32);
    assert!(r == 0 || [EPERM, EINVAL].contains(&errno::get()), "reaches the kernel");
}

#[test]
fn sym_move_mount() {
    errno::set_raw(0);
    let r = s::move_mount(-1, cs("").as_ptr(), AT_FDCWD, cs("/mnt").as_ptr(), 4);
    assert!(r == -1 && [EBADF, EPERM].contains(&errno::get()));
}

#[test]
fn sym_nftw() {
    let t = Tmp::new("nftw");
    t.dir("d");
    t.file("d/f");
    CALLS.set(0);
    assert_eq!(s::nftw(cpath(&t.path).as_ptr(), Some(count_nftw), 4, 1), 0);
    assert_eq!(CALLS.get(), 3);
    CALLS.set(0);
    STOP_AT.set(2);
    assert_eq!(s::nftw(cpath(&t.path).as_ptr(), Some(count_nftw), 4, 1 | 16), 1, "FTW_STOP under FTW_ACTIONRETVAL is returned");
    STOP_AT.set(0);
    errno::set_raw(0);
    assert!(fails(s::nftw(cpath(&t.at("missing")).as_ptr(), Some(count_nftw), 4, 0) as isize, ENOENT));
}

#[test]
fn sym_nftw64() {
    let t = Tmp::new("nftw64");
    t.file("f");
    CALLS.set(0);
    assert_eq!(s::nftw64(cpath(&t.path).as_ptr(), Some(count_nftw), 4, 8), 0);
    assert_eq!(CALLS.get(), 2);
}

#[test]
fn sym_open_tree() {
    let d = Dir::new("open_tree");
    let fd = s::open_tree(AT_FDCWD, d.c().as_ptr(), 0);
    assert!(fd >= 0, "without OPEN_TREE_CLONE: an O_PATH descriptor, unprivileged");
    let mut st = zstat();
    assert_eq!(io::liella_fcntl3(fd, 1, 0) & 1, 0);
    assert_eq!(s::fstat(fd, Some(&mut st)), 0);
    assert_eq!(st.mode & S_IFMT, S_IFDIR);
    io::close(fd);
}

#[test]
fn sym_opendir() {
    let t = Tmp::new("opendir");
    t.file("f");
    let d = s::opendir(cpath(&t.path).as_ptr());
    assert!(!d.is_null());
    assert_eq!(count_entries(d), 3);
    s::closedir(d);
    errno::set_raw(0);
    assert!(s::opendir(cpath(&t.at("missing")).as_ptr()).is_null() && errno::get() == ENOENT);
    errno::set_raw(0);
    assert!(s::opendir(cpath(&t.at("f")).as_ptr()).is_null() && errno::get() == ENOTDIR);
    errno::set_raw(0);
    assert!(s::opendir(null()).is_null() && errno::get() == EFAULT);
}

#[test]
fn sym_pivot_root() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    let r = s::pivot_root(cs("/tmp").as_ptr(), cs("/tmp").as_ptr());
    assert!(r == -1 && [EPERM, EINVAL].contains(&errno::get()));
}

#[test]
fn sym_readdir() {
    let t = Tmp::new("readdir");
    t.file("only");
    let d = s::opendir(cpath(&t.path).as_ptr());
    let mut names = Vec::new();
    loop {
        let e = s::readdir(d);
        if e.is_null() {
            break;
        }
        names.push(ctext(e.cast::<u8>().wrapping_add(19).cast()));
    }
    names.sort();
    assert_eq!(names, [b".".to_vec(), b"..".to_vec(), b"only".to_vec()]);
    errno::set_raw(0);
    assert!(s::readdir(d).is_null() && errno::get() == 0, "the end: errno untouched");
    s::closedir(d);
    errno::set_raw(0);
    assert!(s::readdir(null_mut()).is_null() && errno::get() == EINVAL, "NULL");
}

#[test]
fn sym_readdir64() {
    let t = Tmp::new("readdir64");
    t.file("only");
    let d = s::opendir(cpath(&t.path).as_ptr());
    let mut n = 0;
    while !s::readdir64(d).is_null() {
        n += 1;
    }
    assert_eq!(n, 3);
    s::closedir(d);
}

#[test]
fn sym_readdir64_r() {
    let t = Tmp::new("readdir64_r");
    let d = s::opendir(cpath(&t.path).as_ptr());
    let mut e = struct_dirent64 { d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256] };
    let mut res: *mut struct_dirent64 = null_mut();
    let mut n = 0;
    while s::readdir64_r(d, Some(&mut e), Some(&mut res)) == 0 && !res.is_null() {
        assert_eq!(res, core::ptr::from_mut(&mut e));
        n += 1;
    }
    assert_eq!(n, 2);
    s::closedir(d);
}

#[test]
fn sym_readdir_r() {
    let t = Tmp::new("readdir_r");
    t.file("only");
    let d = s::opendir(cpath(&t.path).as_ptr());
    let mut e = dirent(b"");
    let mut res: *mut struct_dirent = null_mut();
    let mut n = 0;
    while s::readdir_r(d, Some(&mut e), Some(&mut res)) == 0 && !res.is_null() {
        n += 1;
    }
    assert_eq!(n, 3);
    assert_eq!(s::readdir_r(d, None, Some(&mut res)), EFAULT, "the error is returned, not set");
    s::closedir(d);
}

#[test]
fn sym_readlink() {
    let d = Dir::new("readlink");
    let l = d.at("l");
    std::os::unix::fs::symlink("0123456789", l.to_str().unwrap()).unwrap();
    let mut b = [b'#'; 16];
    assert_eq!(s::readlink(l.as_ptr(), b.as_mut_ptr().cast(), 16), 10);
    assert_eq!(&b[..11], b"0123456789#", "no terminator");
    assert_eq!(s::readlink(l.as_ptr(), b.as_mut_ptr().cast(), 4), 4, "silent truncation");
    errno::set_raw(0);
    let f = d.file("f", b"");
    assert!(fails(s::readlink(f.as_ptr(), b.as_mut_ptr().cast(), 16), EINVAL), "not a symlink");
}

#[test]
fn sym___readlink_chk() {
    if std::env::var("LIELLA_S13_CHK").is_ok() {
        let l = cs("/proc/self/exe");
        let mut b = [0u8; 4];
        s::__readlink_chk(l.as_ptr(), b.as_mut_ptr().cast(), 8, 4);
        return;
    }
    let d = Dir::new("__readlink_chk");
    let l = d.at("l");
    std::os::unix::fs::symlink("abc", l.to_str().unwrap()).unwrap();
    let mut b = [0u8; 8];
    assert_eq!(s::__readlink_chk(l.as_ptr(), b.as_mut_ptr().cast(), 8, 8), 3);
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym___readlink_chk", "--exact", "--test-threads=1"])
        .env("LIELLA_S13_CHK", "1")
        .output()
        .unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "n beyond the buffer: the fortify abort");
    assert!(String::from_utf8_lossy(&o.stderr).contains("buffer overflow detected"));
}

#[test]
fn sym___readlinkat_chk() {
    if std::env::var("LIELLA_S13_CHK").is_ok() {
        let l = cs("/proc/self/exe");
        let mut b = [0u8; 4];
        s::__readlinkat_chk(AT_FDCWD, l.as_ptr(), b.as_mut_ptr().cast(), 8, 4);
        return;
    }
    let d = Dir::new("__readlinkat_chk");
    let l = d.at("l");
    std::os::unix::fs::symlink("abc", l.to_str().unwrap()).unwrap();
    let mut b = [0u8; 8];
    assert_eq!(s::__readlinkat_chk(AT_FDCWD, l.as_ptr(), b.as_mut_ptr().cast(), 8, 8), 3);
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym___readlinkat_chk", "--exact", "--test-threads=1"])
        .env("LIELLA_S13_CHK", "1")
        .output()
        .unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "n beyond the buffer: the fortify abort");
    assert!(String::from_utf8_lossy(&o.stderr).contains("buffer overflow detected"));
}

#[test]
fn sym_readlinkat() {
    let d = Dir::new("readlinkat");
    std::os::unix::fs::symlink("tgt", d.0.join("l")).unwrap();
    let dir = d.open();
    let mut b = [0u8; 8];
    assert_eq!(s::readlinkat(dir, cs("l").as_ptr(), b.as_mut_ptr().cast(), 8), 3);
    assert_eq!(&b[..3], b"tgt");
    let fd = io::liella_open3(d.at("l").as_ptr(), 0o10000000 | 0o400000, 0);
    assert_eq!(s::readlinkat(fd, cs("").as_ptr(), b.as_mut_ptr().cast(), 8), 3, "an empty path names the O_PATH descriptor's link");
    io::close(fd);
    io::close(dir);
}

// no spec clause: compat-plan.tsv behaviour row realpath@GLIBC_2.2.5 (LG-2; probe-settled).
#[test]
fn compat_realpath_2_2_5_needs_a_buffer() {
    // Trace: no spec clause (compat-plan.tsv behaviour row realpath@GLIBC_2.2.5; LG-2).
    let t = Tmp::new("realpath225");
    t.dir("a");
    let want = t.at("a").as_os_str().as_encoded_bytes().to_vec();
    errno::set_raw(0);
    assert!(s::liella_compat_realpath(cpath(&t.at("a")).as_ptr(), null_mut()).is_null(), "no allocation");
    assert_eq!(errno::get(), 22, "EINVAL");
    errno::set_raw(0);
    assert!(s::liella_compat_realpath(null(), null_mut()).is_null());
    assert_eq!(errno::get(), 22, "EINVAL before the path is looked at");
    let mut b = vec![0 as c_char; 4096];
    assert_eq!(s::liella_compat_realpath(cpath(&t.at("a/.")).as_ptr(), b.as_mut_ptr()), b.as_mut_ptr());
    assert_eq!(ctext(b.as_ptr()), want, "with a buffer: realpath");
    errno::set_raw(0);
    assert!(s::liella_compat_realpath(cpath(&t.at("missing")).as_ptr(), b.as_mut_ptr()).is_null());
    assert_eq!(errno::get(), 2, "ENOENT as realpath");
    errno::set_raw(0);
}

#[test]
fn sym_realpath() {
    let t = Tmp::new("realpath");
    t.dir("a");
    std::os::unix::fs::symlink("a", t.at("l")).unwrap();
    let want = t.at("a").as_os_str().as_encoded_bytes().to_vec();
    let mut b = vec![0 as c_char; 4096];
    assert_eq!(s::realpath(cpath(&t.at("l/.")).as_ptr(), b.as_mut_ptr()), b.as_mut_ptr());
    assert_eq!(ctext(b.as_ptr()), want);
    assert_eq!(ctext(s::realpath(cpath(&t.at("l")).as_ptr(), null_mut())), want, "allocated");
    errno::set_raw(0);
    assert!(s::realpath(null(), b.as_mut_ptr()).is_null() && errno::get() == EINVAL);
    errno::set_raw(0);
    assert!(s::realpath(cpath(&t.at("l/missing")).as_ptr(), b.as_mut_ptr()).is_null() && errno::get() == ENOENT);
    assert_eq!(ctext(b.as_ptr()), t.at("a/missing").as_os_str().as_encoded_bytes(), "the prefix resolved");
}

#[test]
fn sym___realpath_chk() {
    if let Ok(p) = std::env::var("LIELLA_S13_CHK") {
        let mut b = [0 as c_char; 4];
        s::__realpath_chk(cs(&p).as_ptr(), b.as_mut_ptr(), 4);
        return;
    }
    let t = Tmp::new("realpath_chk");
    let mut b = vec![0 as c_char; 4096];
    assert_eq!(ctext(s::__realpath_chk(cpath(&t.path).as_ptr(), b.as_mut_ptr(), 4096)), t.path.as_os_str().as_encoded_bytes());
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args(["sym___realpath_chk", "--exact", "--test-threads=1"]).env("LIELLA_S13_CHK", &t.path).output().unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "a result beyond a 4-byte object: the fortify abort");
}

#[test]
fn sym_removexattr() {
    let d = Dir::new("removexattr");
    let f = d.file("f", b"");
    s::setxattr(f.as_ptr(), cs("user.k").as_ptr(), b"1".as_ptr().cast(), 1, 0);
    assert_eq!(s::removexattr(f.as_ptr(), cs("user.k").as_ptr()), 0);
    errno::set_raw(0);
    assert!(fails(s::removexattr(f.as_ptr(), cs("user.k").as_ptr()) as isize, ENODATA));
}

#[test]
fn sym_rename() {
    let d = Dir::new("rename");
    let a = d.file("a", b"new");
    let b = d.file("b", b"old");
    assert_eq!(s::rename(a.as_ptr(), b.as_ptr()), 0, "replaces an existing target");
    assert_eq!(std::fs::read(b.to_str().unwrap()).unwrap(), b"new");
    assert!(!exists(&a));
    errno::set_raw(0);
    let sub = d.at("sub");
    std::fs::create_dir(sub.to_str().unwrap()).unwrap();
    assert!(fails(s::rename(b.as_ptr(), sub.as_ptr()) as isize, EISDIR));
    assert!(fails(s::rename(a.as_ptr(), b.as_ptr()) as isize, ENOENT));
}

#[test]
fn sym_renameat2() {
    let d = Dir::new("renameat2");
    d.file("a", b"A");
    d.file("b", b"B");
    let dir = d.open();
    errno::set_raw(0);
    assert!(fails(s::renameat2(dir, cs("a").as_ptr(), dir, cs("b").as_ptr(), 1) as isize, EEXIST), "RENAME_NOREPLACE");
    assert_eq!(s::renameat2(dir, cs("a").as_ptr(), dir, cs("b").as_ptr(), 2), 0, "RENAME_EXCHANGE");
    assert_eq!(std::fs::read(d.0.join("a")).unwrap(), b"B");
    assert_eq!(std::fs::read(d.0.join("b")).unwrap(), b"A");
    assert!(fails(s::renameat2(dir, cs("a").as_ptr(), dir, cs("c").as_ptr(), 3) as isize, EINVAL), "both is invalid");
    io::close(dir);
}

#[test]
fn sym_renameat() {
    let d = Dir::new("renameat");
    d.file("a", b"1");
    std::fs::create_dir(d.0.join("sub")).unwrap();
    let dir = d.open();
    let sub = io::liella_open3(d.at("sub").as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    assert_eq!(s::renameat(dir, cs("a").as_ptr(), sub, cs("moved").as_ptr()), 0);
    assert!(exists(&d.at("sub/moved")));
    io::close(sub);
    io::close(dir);
}

#[test]
fn sym_revoke() {
    // §19.5 (phase 11): BSD's revoke is not implemented on Linux; ENOSYS for every argument.
    errno::set_raw(0);
    for path in [cs("/dev/null"), cs("/nonexistent/x"), cs("")] {
        assert!(fails(liella_boundary::posix::s19_terminals::revoke(path.as_ptr()) as isize, ENOSYS));
    }
    assert!(fails(liella_boundary::posix::s19_terminals::revoke(null()) as isize, ENOSYS), "not even read");
}
#[test]
fn sym_rewinddir() {
    let t = Tmp::new("rewinddir");
    let d = s::opendir(cpath(&t.path).as_ptr());
    assert_eq!(count_entries(d), 2);
    t.file("new");
    s::rewinddir(d);
    assert_eq!(count_entries(d), 3, "restarted, and the new entry seen");
    s::closedir(d);
}

#[test]
fn sym_rmdir() {
    let d = Dir::new("rmdir");
    let sub = d.at("sub");
    std::fs::create_dir(sub.to_str().unwrap()).unwrap();
    std::fs::write(d.0.join("sub/x"), b"").unwrap();
    errno::set_raw(0);
    assert!(fails(s::rmdir(sub.as_ptr()) as isize, ENOTEMPTY));
    std::fs::remove_file(d.0.join("sub/x")).unwrap();
    assert_eq!(s::rmdir(sub.as_ptr()), 0);
    let f = d.file("f", b"");
    assert!(fails(s::rmdir(f.as_ptr()) as isize, ENOTDIR));
}

#[test]
fn sym_scandir() {
    let t = Tmp::new("scandir");
    t.file("b");
    t.file("a");
    let mut list = null_mut();
    let n = s::scandir(cpath(&t.path).as_ptr(), Some(&mut list), None, Some(alpha));
    assert_eq!(listed(list, n), [".", "..", "a", "b"]);
    errno::set_raw(0);
    assert!(fails(s::scandir(cpath(&t.at("missing")).as_ptr(), Some(&mut list), None, None) as isize, ENOENT));
}

#[test]
fn sym_scandir64() {
    let t = Tmp::new("scandir64");
    t.file("a");
    let mut list: *mut *mut struct_dirent64 = null_mut();
    assert_eq!(s::scandir64(cpath(&t.path).as_ptr(), Some(&mut list), None, None), 3);
    assert!(!list.is_null());
}

#[test]
fn sym_scandirat64() {
    let t = Tmp::new("scandirat64");
    let mut list: *mut *mut struct_dirent64 = null_mut();
    assert_eq!(s::scandirat64(AT_FDCWD, cpath(&t.path).as_ptr(), Some(&mut list), None, None), 2);
}

#[test]
fn sym_scandirat() {
    let t = Tmp::new("scandirat");
    t.dir("sub");
    t.file("sub/x");
    let fd = io::liella_open3(cpath(&t.path).as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut list = null_mut();
    let n = s::scandirat(fd, cs("sub").as_ptr(), Some(&mut list), None, Some(alpha));
    assert_eq!(listed(list, n), [".", "..", "x"], "relative to the descriptor");
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::scandirat(CLOSED, cs("sub").as_ptr(), Some(&mut list), None, None) as isize, EBADF));
}

#[test]
fn sym_seekdir() {
    let t = Tmp::new("seekdir");
    for n in ["a", "b", "c"] {
        t.file(n);
    }
    let d = s::opendir(cpath(&t.path).as_ptr());
    s::readdir(d);
    let pos = s::telldir(d);
    let next = ctext(s::readdir(d).cast::<u8>().wrapping_add(19).cast());
    s::seekdir(d, pos);
    assert_eq!(ctext(s::readdir(d).cast::<u8>().wrapping_add(19).cast()), next);
    s::closedir(d);
}

#[test]
fn sym_setfsent() {
    let t = Tmp::new("setfsent");
    in_child("sym_setfsent", &t, |_| {
        let want = fstab_entries();
        errno::set_raw(0);
        if std::path::Path::new("/etc/fstab").is_file() {
            assert_eq!(s::setfsent(), 1);
            let _ = s::getfsent();
            assert_eq!(s::setfsent(), 1, "already open");
            assert_eq!(show_fs(s::getfsent()).map(|v| v.0), want.first().cloned(), "back at the first entry");
        } else {
            assert_eq!(s::setfsent(), 0);
            assert!([ENOENT, EISDIR].contains(&errno::get()));
        }
        s::endfsent();
    });
}

type Open = extern "C" fn(*const c_char, *const c_char) -> *mut liella_abi::layout::FILE;

fn setmntent_case(open: Open, tag: &str) {
    use liella_kernel::ops::FcntlInt;
    use liella_kernel::sys::Sys;
    let d = Dir::new(tag);
    let path = d.file("t", b"a /a t o 0 0\n");
    for (mode, acc) in [("r", 0), ("r+", 2), ("a", 1), ("w", 1)] {
        let f = open(path.as_ptr(), cs(mode).as_ptr());
        assert!(!f.is_null(), "{mode}");
        let fd = liella_kernel::fd::Fd::new(liella_boundary::posix::s12_stdio::fileno(f));
        assert_eq!(liella_boundary::sys::CSys.fcntl_int(fd, FcntlInt::GetFd, 0), Ok(1), "close-on-exec ({mode})");
        assert_eq!(liella_boundary::sys::CSys.fcntl_int(fd, FcntlInt::GetFl, 0).unwrap() & 3, acc, "{mode}");
        s::endmntent(f);
    }
    errno::set_raw(0);
    assert!(open(d.at("none").as_ptr(), c"r".as_ptr()).is_null() && errno::get() == ENOENT);
    assert!(open(path.as_ptr(), c"q".as_ptr()).is_null() && errno::get() == EINVAL, "a bad mode");
    assert!(open(null(), c"r".as_ptr()).is_null() && errno::get() == EFAULT);
}

#[test]
fn sym___setmntent() {
    setmntent_case(s::__setmntent, "__setmntent");
}

#[test]
fn sym_setmntent() {
    setmntent_case(s::setmntent, "setmntent");
}
#[test]
fn sym_setxattr() {
    let d = Dir::new("setxattr");
    let f = d.file("f", b"");
    let (k, v) = (cs("user.liella"), b"value");
    assert_eq!(s::setxattr(f.as_ptr(), k.as_ptr(), v.as_ptr().cast(), 5, 0), 0);
    errno::set_raw(0);
    assert!(fails(s::setxattr(f.as_ptr(), k.as_ptr(), v.as_ptr().cast(), 5, 1) as isize, EEXIST), "XATTR_CREATE");
    assert!(fails(s::setxattr(f.as_ptr(), cs("user.none").as_ptr(), v.as_ptr().cast(), 5, 2) as isize, ENODATA), "XATTR_REPLACE");
    assert!(fails(s::setxattr(f.as_ptr(), cs("bogus.ns").as_ptr(), v.as_ptr().cast(), 5, 0) as isize, EOPNOTSUPP), "an unknown namespace");
    let mut b = [0u8; 8];
    assert_eq!(s::getxattr(f.as_ptr(), k.as_ptr(), b.as_mut_ptr().cast(), 8), 5);
}

#[test]
fn sym___statfs() {
    let mut b = Statfs::default();
    assert_eq!(s::__statfs(cs("/proc").as_ptr(), Some(&mut b)), 0);
    assert_eq!(b.ftype, 0x9fa0, "PROC_SUPER_MAGIC");
    assert_ne!(b.flags & 0x20, 0, "ST_VALID: the kernel fills f_flags");
    errno::set_raw(0);
    assert!(fails(s::__statfs(cs("/no/such/path").as_ptr(), Some(&mut b)) as isize, ENOENT));
}

#[test]
fn sym_statfs() {
    let mut b = Statfs::default();
    assert_eq!(s::statfs(cs("/proc").as_ptr(), Some(&mut b)), 0);
    assert_eq!(b.ftype, 0x9fa0, "PROC_SUPER_MAGIC");
    assert_ne!(b.flags & 0x20, 0, "ST_VALID: the kernel fills f_flags");
    errno::set_raw(0);
    assert!(fails(s::statfs(cs("/no/such/path").as_ptr(), Some(&mut b)) as isize, ENOENT));
}

#[test]
fn sym_statfs64() {
    let mut b = Statfs::default();
    assert_eq!(s::statfs64(cs("/proc").as_ptr(), Some(&mut b)), 0);
    assert_eq!(b.ftype, 0x9fa0, "PROC_SUPER_MAGIC");
    assert_ne!(b.flags & 0x20, 0, "ST_VALID: the kernel fills f_flags");
    errno::set_raw(0);
    assert!(fails(s::statfs64(cs("/no/such/path").as_ptr(), Some(&mut b)) as isize, ENOENT));
}

#[test]
fn sym_statvfs() {
    let mut v = zero_statvfs();
    let mut f = Statfs::default();
    assert_eq!(s::statvfs(cs("/proc").as_ptr(), Some(&mut v)), 0);
    assert_eq!(s::statfs(cs("/proc").as_ptr(), Some(&mut f)), 0);
    assert_eq!((v.f_bsize, v.f_blocks, v.f_namemax, u64::from(v.f_type)), (f.bsize as u64, f.blocks, f.namelen as u64, f.ftype as u64));
    assert_eq!(v.f_favail, v.f_ffree);
    assert_eq!(v.f_flag, (f.flags & !0x20) as u64, "f_flag is f_flags without ST_VALID");
    assert_ne!(v.f_flag & 0x2, 0, "/proc is mounted nosuid (ST_NOSUID)");
    assert_eq!(v.f_fsid, (i64::from(f.fsid[0]) as u64) | (u64::from(f.fsid[1] as u32) << 32));
    errno::set_raw(0);
    assert!(fails(s::statvfs(cs("/proc").as_ptr(), None) as isize, EFAULT));
}

#[test]
fn sym_statvfs64() {
    let mut v = zero_statvfs();
    let mut f = Statfs::default();
    assert_eq!(s::statvfs64(cs("/proc").as_ptr(), Some(&mut v)), 0);
    assert_eq!(s::statfs(cs("/proc").as_ptr(), Some(&mut f)), 0);
    assert_eq!((v.f_bsize, v.f_blocks, v.f_namemax, u64::from(v.f_type)), (f.bsize as u64, f.blocks, f.namelen as u64, f.ftype as u64));
    assert_eq!(v.f_favail, v.f_ffree);
    assert_eq!(v.f_flag, (f.flags & !0x20) as u64, "f_flag is f_flags without ST_VALID");
    assert_ne!(v.f_flag & 0x2, 0, "/proc is mounted nosuid (ST_NOSUID)");
    assert_eq!(v.f_fsid, (i64::from(f.fsid[0]) as u64) | (u64::from(f.fsid[1] as u32) << 32));
    errno::set_raw(0);
    assert!(fails(s::statvfs64(cs("/proc").as_ptr(), None) as isize, EFAULT));
}

#[test]
fn sym_statx() {
    let d = Dir::new("statx");
    let f = d.file("f", b"hello");
    let mut sx = Statx::default();
    assert_eq!(s::statx(AT_FDCWD, f.as_ptr(), 0, 0x7ff, Some(&mut sx)), 0, "STATX_BASIC_STATS");
    assert_eq!(sx.mask & 0x7ff, 0x7ff, "the basic fields are filled");
    assert_eq!((u32::from(sx.mode) & S_IFMT, sx.size), (S_IFREG, 5));
    let mut st = zstat();
    s::stat(f.as_ptr(), Some(&mut st));
    assert_eq!(sx.ino, st.ino);
    errno::set_raw(0);
    assert!(fails(s::statx(AT_FDCWD, f.as_ptr(), 0, 0x8000_0000, Some(&mut sx)) as isize, EINVAL), "STATX__RESERVED");
}

#[test]
fn sym_swapoff() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    assert!(fails(s::swapoff(cs("/no/such/swap").as_ptr()) as isize, EPERM));
}

#[test]
fn sym_swapon() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    assert!(fails(s::swapon(cs("/no/such/swap").as_ptr(), 0) as isize, EPERM));
}

#[test]
fn sym_symlink() {
    let d = Dir::new("symlink");
    let l = d.at("l");
    assert_eq!(s::symlink(cs("no/such/target").as_ptr(), l.as_ptr()), 0, "the target is not resolved");
    assert_eq!(std::fs::read_link(l.to_str().unwrap()).unwrap().to_str().unwrap(), "no/such/target");
    errno::set_raw(0);
    assert!(fails(s::symlink(cs("t").as_ptr(), l.as_ptr()) as isize, EEXIST));
}

#[test]
fn sym_symlinkat() {
    let d = Dir::new("symlinkat");
    let dir = d.open();
    assert_eq!(s::symlinkat(cs("target").as_ptr(), dir, cs("rel").as_ptr()), 0);
    assert_eq!(std::fs::read_link(d.0.join("rel")).unwrap().to_str().unwrap(), "target");
    errno::set_raw(0);
    assert!(fails(s::symlinkat(cs("t").as_ptr(), -1, cs("rel2").as_ptr()) as isize, EBADF));
    io::close(dir);
}

#[test]
fn sym_telldir() {
    let t = Tmp::new("telldir");
    let d = s::opendir(cpath(&t.path).as_ptr());
    assert_eq!(s::telldir(d), 0, "the start");
    s::readdir(d);
    assert_ne!(s::telldir(d), 0, "the file system's cookie after the first entry");
    s::closedir(d);
    errno::set_raw(0);
    assert!(fails(s::telldir(null_mut()) as isize, EINVAL));
}

#[test]
fn sym_truncate() {
    let d = Dir::new("truncate");
    let p = d.file("f", b"0123456789");
    assert_eq!(s::truncate(p.as_ptr(), 3), 0);
    assert_eq!(std::fs::read(p.to_str().unwrap()).unwrap(), b"012");
    errno::set_raw(0);
    assert!(fails(s::truncate(d.at("missing").as_ptr(), 0) as isize, ENOENT));
    assert!(fails(s::truncate(d.c().as_ptr(), 0) as isize, EISDIR));
    assert!(fails(s::truncate(null(), 0) as isize, EFAULT));
}

#[test]
fn sym_truncate64() {
    let d = Dir::new("truncate64");
    let p = d.file("f", b"0123456789");
    assert_eq!(s::truncate64(p.as_ptr(), 3), 0);
    assert_eq!(std::fs::read(p.to_str().unwrap()).unwrap(), b"012");
    errno::set_raw(0);
    assert!(fails(s::truncate64(d.at("missing").as_ptr(), 0) as isize, ENOENT));
    assert!(fails(s::truncate64(d.c().as_ptr(), 0) as isize, EISDIR));
    assert!(fails(s::truncate64(null(), 0) as isize, EFAULT));
}

#[test]
fn sym_umask() {
    // The mask is process-wide: changed only in a child, then checked there.
    if std::env::var("LIELLA_S13_UMASK").is_ok() {
        let old = s::umask(0o027);
        let back = s::umask(old);
        let d = Dir::new("umask");
        s::umask(0o077);
        let p = d.at("f");
        let fd = io::liella_open3(p.as_ptr(), O_RDWR | O_CREAT, 0o666);
        io::close(fd);
        let ok = back == 0o027 && mode_of(&p) & 0o777 == 0o600 && s::umask(0o1_777_777) == 0o077 && s::umask(0) == 0o777;
        drop(d); // `exit` runs no destructors
        std::process::exit(if ok { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_umask", "--exact", "--test-threads=1"])
        .env("LIELLA_S13_UMASK", "1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "returns the old mask; masks creation; only the 0777 bits are kept");
}

#[test]
fn sym_umount() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    assert!(fails(s::umount(cs("/").as_ptr()) as isize, EPERM));
}

#[test]
fn sym_umount2() {
    if root() {
        return; // these tests describe the unprivileged answer
    }
    errno::set_raw(0);
    assert!(fails(s::umount2(cs("/").as_ptr(), 2) as isize, EPERM), "MNT_DETACH");
}

#[test]
fn sym_unlink() {
    let d = Dir::new("unlink");
    let f = d.file("f", b"");
    assert_eq!(s::unlink(f.as_ptr()), 0);
    assert!(!exists(&f));
    errno::set_raw(0);
    assert!(fails(s::unlink(f.as_ptr()) as isize, ENOENT));
    assert!(fails(s::unlink(d.c().as_ptr()) as isize, EISDIR), "a directory (Linux)");
}

#[test]
fn sym_unlinkat() {
    let d = Dir::new("unlinkat");
    d.file("f", b"");
    std::fs::create_dir(d.0.join("sub")).unwrap();
    let dir = d.open();
    assert_eq!(s::unlinkat(dir, cs("f").as_ptr(), 0), 0);
    errno::set_raw(0);
    assert!(fails(s::unlinkat(dir, cs("sub").as_ptr(), 0) as isize, EISDIR));
    assert_eq!(s::unlinkat(dir, cs("sub").as_ptr(), AT_REMOVEDIR), 0);
    assert!(fails(s::unlinkat(dir, cs("x").as_ptr(), 0x1) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_ustat() {
    let mut st = zstat();
    s::stat(cs("/").as_ptr(), Some(&mut st));
    let mut u = [0u8; 32];
    let r = s::ustat(st.dev, Some(&mut u));
    assert!(r == 0 || [EINVAL, 38].contains(&errno::get()), "the kernel's answer (ENOSYS where removed)");
    errno::set_raw(0);
    assert!(fails(s::ustat(1 << 40, Some(&mut u)) as isize, EINVAL), "a device number beyond 32 bits");
}

#[test]
fn sym_utime() {
    let d = Dir::new("utime");
    let f = d.file("f", b"");
    assert_eq!(s::utime(f.as_ptr(), Some(&[1_000_000, 2_000_000])), 0);
    let mut st = zstat();
    s::stat(f.as_ptr(), Some(&mut st));
    assert_eq!((st.atime.sec, st.mtime.sec, st.mtime.nsec), (1_000_000, 2_000_000, 0), "actime, modtime");
    assert_eq!(s::utime(f.as_ptr(), None), 0, "NULL: now");
    s::stat(f.as_ptr(), Some(&mut st));
    assert!(st.mtime.sec > 2_000_000);
}

#[test]
fn sym_utimensat() {
    let d = Dir::new("utimensat");
    d.file("f", b"");
    std::os::unix::fs::symlink("f", d.0.join("l")).unwrap();
    let dir = d.open();
    let ts = [Timespec { sec: 3, nsec: 123 }, Timespec { sec: 4, nsec: UTIME_OMIT }];
    let mut before = zstat();
    s::stat(d.at("f").as_ptr(), Some(&mut before));
    assert_eq!(s::utimensat(dir, cs("f").as_ptr(), Some(&ts), 0), 0);
    let mut st = zstat();
    s::stat(d.at("f").as_ptr(), Some(&mut st));
    assert_eq!((st.atime.sec, st.atime.nsec), (3, 123), "nanoseconds");
    assert_eq!(st.mtime, before.mtime, "UTIME_OMIT");
    assert_eq!(s::utimensat(dir, cs("l").as_ptr(), Some(&ts), AT_SYMLINK_NOFOLLOW), 0, "the link itself");
    errno::set_raw(0);
    let bad = [Timespec { sec: 0, nsec: 1_000_000_000 }, Timespec { sec: 0, nsec: 0 }];
    assert!(fails(s::utimensat(dir, cs("f").as_ptr(), Some(&bad), 0) as isize, EINVAL));
    io::close(dir);
}

#[test]
fn sym_versionsort() {
    // strverscmp order: digit runs compare as numbers.
    let (a2, a10, a01, a1) = (dirent(b"a2"), dirent(b"a10"), dirent(b"a01"), dirent(b"a1"));
    assert!(s::versionsort(&ent_ptr(&a2), &ent_ptr(&a10)) < 0);
    assert!(s::versionsort(&ent_ptr(&a01), &ent_ptr(&a1)) < 0);
    assert_eq!(s::versionsort(&ent_ptr(&a1), &ent_ptr(&a1)), 0);
}

#[test]
fn sym_versionsort64() {
    let (a2, a10) = (dirent(b"a2"), dirent(b"a10"));
    assert!(s::versionsort64(&ent_ptr(&a10), &ent_ptr(&a2)) > 0);
}

#[test]
fn sym___xmknod() {
    let d = Dir::new("__xmknod");
    let p = d.at("node");
    assert_eq!(s::__xmknod(0, p.as_ptr(), S_IFIFO | 0o600, Some(&0u64)), 0, "a FIFO");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFIFO);
    let p = d.at("reg");
    assert_eq!(s::__xmknod(0, p.as_ptr(), S_IFREG | 0o600, Some(&0u64)), 0, "a regular file");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFREG);
    let p = d.at("dev");
    errno::set_raw(0);
    let big = 1u64 << 40;
    assert!(fails(s::__xmknod(0, p.as_ptr(), 0o020000 | 0o600, Some(&big)) as isize, EINVAL), "a device number beyond 32 bits");
    let r = s::__xmknod(0, p.as_ptr(), 0o020000 | 0o600, Some(&0x0103u64));
    assert!(r == 0 || (!root() && errno::get() == EPERM), "a character device needs privilege");
    assert!(fails(s::__xmknod(1, p.as_ptr(), S_IFIFO | 0o600, Some(&0u64)) as isize, EINVAL), "an unknown _MKNOD_VER");
}

#[test]
fn sym___xmknodat() {
    let d = Dir::new("__xmknodat");
    let p = d.at("node");
    assert_eq!(s::__xmknodat(0, AT_FDCWD, p.as_ptr(), S_IFIFO | 0o600, Some(&0u64)), 0, "a FIFO");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFIFO);
    let p = d.at("reg");
    assert_eq!(s::__xmknodat(0, AT_FDCWD, p.as_ptr(), S_IFREG | 0o600, Some(&0u64)), 0, "a regular file");
    assert_eq!(mode_of(&p) & S_IFMT, S_IFREG);
    let p = d.at("dev");
    errno::set_raw(0);
    let big = 1u64 << 40;
    assert!(fails(s::__xmknodat(0, AT_FDCWD, p.as_ptr(), 0o020000 | 0o600, Some(&big)) as isize, EINVAL), "a device number beyond 32 bits");
    let r = s::__xmknodat(0, AT_FDCWD, p.as_ptr(), 0o020000 | 0o600, Some(&0x0103u64));
    assert!(r == 0 || (!root() && errno::get() == EPERM), "a character device needs privilege");
    assert!(fails(s::__xmknodat(1, AT_FDCWD, p.as_ptr(), S_IFIFO | 0o600, Some(&0u64)) as isize, EINVAL), "an unknown _MKNOD_VER");
}

#[test]
fn sym___xstat64() {
    let d = Dir::new("__xstat64");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__xstat64(1, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__xstat64(0, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__xstat64(3, f.as_ptr(), Some(&mut st)) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym___xstat() {
    let d = Dir::new("__xstat");
    let f = d.file("f", b"xyz");
    let fd = io::liella_open3(f.as_ptr(), O_RDONLY, 0);
    let mut st = zstat();
    assert_eq!(s::__xstat(1, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_LINUX");
    assert_eq!(st.size, 3);
    assert_eq!(s::__xstat(0, f.as_ptr(), Some(&mut st)), 0, "_STAT_VER_KERNEL names the same structure on x86-64");
    errno::set_raw(0);
    assert!(fails(s::__xstat(3, f.as_ptr(), Some(&mut st)) as isize, EINVAL), "an unknown version");
    io::close(fd);
}

#[test]
fn sym_stat64() {
    let d = Dir::new("stat64");
    let f = d.file("f", b"12345");
    let l = d.at("l");
    std::os::unix::fs::symlink(f.to_str().unwrap(), l.to_str().unwrap()).unwrap();
    let mut st = zstat();
    assert_eq!(s::stat64(l.as_ptr(), Some(&mut st)), 0);
    assert_eq!((st.mode & S_IFMT, st.size, st.nlink), (S_IFREG, 5, 1), "follows the symlink");
    let m = std::fs::metadata(f.to_str().unwrap()).unwrap();
    assert_eq!(st.ino, std::os::unix::fs::MetadataExt::ino(&m));
    assert_eq!(st.mtime.sec, std::os::unix::fs::MetadataExt::mtime(&m));
    errno::set_raw(0);
    assert!(fails(s::stat64(d.at("none").as_ptr(), Some(&mut st)) as isize, ENOENT));
    assert!(fails(s::stat64(f.as_ptr(), None) as isize, EFAULT));
    std::os::unix::fs::symlink("loop", d.0.join("loop")).unwrap();
    assert!(fails(s::stat64(d.at("loop").as_ptr(), Some(&mut st)) as isize, ELOOP));
}

#[test]
fn sym_stat() {
    let d = Dir::new("stat");
    let f = d.file("f", b"12345");
    let l = d.at("l");
    std::os::unix::fs::symlink(f.to_str().unwrap(), l.to_str().unwrap()).unwrap();
    let mut st = zstat();
    assert_eq!(s::stat(l.as_ptr(), Some(&mut st)), 0);
    assert_eq!((st.mode & S_IFMT, st.size, st.nlink), (S_IFREG, 5, 1), "follows the symlink");
    let m = std::fs::metadata(f.to_str().unwrap()).unwrap();
    assert_eq!(st.ino, std::os::unix::fs::MetadataExt::ino(&m));
    assert_eq!(st.mtime.sec, std::os::unix::fs::MetadataExt::mtime(&m));
    errno::set_raw(0);
    assert!(fails(s::stat(d.at("none").as_ptr(), Some(&mut st)) as isize, ENOENT));
    assert!(fails(s::stat(f.as_ptr(), None) as isize, EFAULT));
    std::os::unix::fs::symlink("loop", d.0.join("loop")).unwrap();
    assert!(fails(s::stat(d.at("loop").as_ptr(), Some(&mut st)) as isize, ELOOP));
}

#[test]
fn sym_futimes() {
    let d = Dir::new("futimes");
    let f = d.file("f", b"");
    let fd = io::liella_open3(f.as_ptr(), O_RDWR, 0);
    let tv = [Timeval { sec: 5, usec: 1 }, Timeval { sec: 6, usec: 2 }];
    errno::set_raw(0);
    assert_eq!((s::futimes(-100, None), errno::get()), (-1, 9), "AT_FDCWD is not a descriptor (validation T9)");
    assert_eq!(s::futimes(fd, Some(&tv)), 0);
    let mut st = zstat();
    s::fstat(fd, Some(&mut st));
    assert_eq!((st.mtime.sec, st.mtime.nsec), (6, 2000));
    io::close(fd);
    errno::set_raw(0);
    assert!(fails(s::futimes(CLOSED, Some(&tv)) as isize, EBADF));
}

#[test]
fn sym_futimesat() {
    let d = Dir::new("futimesat");
    d.file("f", b"");
    let dir = d.open();
    let tv = [Timeval { sec: 7, usec: 0 }, Timeval { sec: 8, usec: 0 }];
    assert_eq!(s::futimesat(dir, cs("f").as_ptr(), Some(&tv)), 0);
    let mut st = zstat();
    s::stat(d.at("f").as_ptr(), Some(&mut st));
    assert_eq!(st.mtime.sec, 8);
    assert_eq!(s::futimesat(dir, null(), Some(&tv)), 0, "NULL path: the descriptor itself");
    s::stat(d.c().as_ptr(), Some(&mut st));
    assert_eq!(st.mtime.sec, 8);
    io::close(dir);
}

#[test]
fn sym_lutimes() {
    let d = Dir::new("lutimes");
    let f = d.file("f", b"");
    let tv = [Timeval { sec: 10, usec: 250_000 }, Timeval { sec: 20, usec: 999_999 }];
    assert_eq!(s::lutimes(f.as_ptr(), Some(&tv)), 0);
    let mut st = zstat();
    s::stat(f.as_ptr(), Some(&mut st));
    assert_eq!((st.atime.sec, st.atime.nsec, st.mtime.sec, st.mtime.nsec), (10, 250_000_000, 20, 999_999_000), "microseconds");
    errno::set_raw(0);
    let bad = [Timeval { sec: 1, usec: 1_000_000 }, Timeval { sec: 1, usec: 0 }];
    assert!(fails(s::lutimes(f.as_ptr(), Some(&bad)) as isize, EINVAL), "usec out of range");
    assert_eq!(s::lutimes(f.as_ptr(), None), 0, "NULL: now");
}

#[test]
fn sym_utimes() {
    let d = Dir::new("utimes");
    let f = d.file("f", b"");
    let tv = [Timeval { sec: 10, usec: 250_000 }, Timeval { sec: 20, usec: 999_999 }];
    assert_eq!(s::utimes(f.as_ptr(), Some(&tv)), 0);
    let mut st = zstat();
    s::stat(f.as_ptr(), Some(&mut st));
    assert_eq!((st.atime.sec, st.atime.nsec, st.mtime.sec, st.mtime.nsec), (10, 250_000_000, 20, 999_999_000), "microseconds");
    errno::set_raw(0);
    let bad = [Timeval { sec: 1, usec: 1_000_000 }, Timeval { sec: 1, usec: 0 }];
    assert!(fails(s::utimes(f.as_ptr(), Some(&bad)) as isize, EINVAL), "usec out of range");
    assert_eq!(s::utimes(f.as_ptr(), None), 0, "NULL: now");
}

// ------------------------------------------------------------------------------ phase 12 helpers

use p12::*;

/// Helpers of the phase-12 tests (§13.1–13.3, §13.8–13.9): scratch directories under Cargo's
/// temporary directory for integration tests (never /tmp), children for process-wide state, C
/// memory read through the library's own `strlen`/`memcpy`.
mod p12 {
    use core::ffi::c_void;
    use std::ffi::CString;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

    use liella_abi::ctypes::{c_char, c_int};
    pub(crate) use liella_abi::layout::struct_dirent;
    use liella_boundary::posix::s04_strings as st;
    use liella_boundary::posix::s13_filesystem as s;

    const CHILD: &str = "LIELLA_S13_CHILD";
    const CHILD_DIR: &str = "LIELLA_S13_DIR";

    pub(crate) struct Tmp {
        pub(crate) path: PathBuf,
        owned: bool,
    }

    impl Tmp {
        pub(crate) fn new(tag: &str) -> Tmp {
            if std::env::var(CHILD).is_ok() {
                if let Ok(p) = std::env::var(CHILD_DIR) {
                    return Tmp { path: PathBuf::from(p), owned: false };
                }
            }
            let n = super::SEQ.fetch_add(1, Ordering::Relaxed);
            let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("s13-{}-{tag}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp { path: std::fs::canonicalize(&p).unwrap(), owned: true }
        }
        pub(crate) fn at(&self, rel: &str) -> PathBuf {
            self.path.join(rel)
        }
        pub(crate) fn dir(&self, rel: &str) {
            std::fs::create_dir_all(self.at(rel)).unwrap();
        }
        pub(crate) fn file(&self, rel: &str) {
            std::fs::write(self.at(rel), b"").unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            if self.owned {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }

    pub(crate) fn cpath(p: &Path) -> CString {
        CString::new(p.as_os_str().as_encoded_bytes()).unwrap()
    }

    pub(crate) fn ctext(p: *const c_char) -> Vec<u8> {
        if p.is_null() {
            return Vec::new();
        }
        let n = st::strlen(p);
        let mut v = vec![0u8; n];
        st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
        v
    }

    pub(crate) fn cbytes(p: *const c_void, n: usize) -> Vec<u8> {
        let mut v = vec![0u8; n];
        st::memcpy(v.as_mut_ptr().cast(), p, n);
        v
    }

    pub(crate) fn kernel_cwd() -> Vec<u8> {
        use liella_kernel::sys::Sys;
        let mut b = vec![0u8; 65536];
        let n = liella_boundary::sys::CSys.getcwd(&mut b).unwrap();
        b.truncate(n - 1);
        b
    }

    /// Run `body` in a child (this test re-executed alone) with the directory `t`.
    pub(crate) fn in_child(name: &str, t: &Tmp, body: impl FnOnce(&Path)) {
        if std::env::var(CHILD).is_ok_and(|v| v == name) {
            body(&t.path);
            println!("child {name}: done");
            liella_kernel::sys::Sys::exit_group(&liella_boundary::sys::CSys, 0);
        }
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--test-threads=1", "--nocapture"])
            .env(CHILD, name)
            .env(CHILD_DIR, &t.path)
            .output()
            .unwrap();
        let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success() && out.contains(&format!("child {name}: done")), "{name} failed in its child ({:?}):\n{out}\n{err}", o.status);
    }

    pub(crate) fn dirent(name: &[u8]) -> struct_dirent {
        let mut d = struct_dirent { d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256] };
        for (slot, &c) in d.d_name.iter_mut().zip(name) {
            *slot = c as i8;
        }
        d
    }

    pub(crate) fn ent_ptr(d: &struct_dirent) -> *const c_void {
        core::ptr::from_ref(d).cast()
    }

    pub(crate) fn count_entries(d: *mut c_void) -> usize {
        let mut n = 0;
        while !s::readdir(d).is_null() {
            n += 1;
        }
        n
    }

    /// A template in `t`, as a writable C string.
    pub(crate) fn template(t: &Tmp, name: &str) -> Vec<c_char> {
        let mut v: Vec<c_char> = t.at(name).as_os_str().as_encoded_bytes().iter().map(|&b| b as c_char).collect();
        v.push(0);
        v
    }

    /// The six X of the template were replaced by letters and digits.
    pub(crate) fn replaced(b: &[c_char]) -> bool {
        let text = ctext(b.as_ptr());
        let name = &text[text.iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1)..];
        let slot = &name[1..7];
        slot.iter().all(|c| c.is_ascii_alphanumeric()) && slot != b"XXXXXX"
    }

    pub(crate) use liella_abi::layout::struct_dirent64;
    pub(crate) const EIO: i32 = 5;
    pub(crate) const O_CLOEXEC: c_int = 0o2000000;

    pub(crate) fn argv(args: &[CString]) -> Vec<*mut c_char> {
        let mut v: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
        v.push(core::ptr::null_mut());
        v
    }

    thread_local! {
        pub(crate) static CALLS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
        pub(crate) static STOP_AT: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    }

    pub(crate) extern "C" fn count_ftw(_: *const c_char, _: *const c_void, _: c_int) -> c_int {
        CALLS.set(CALLS.get() + 1);
        0
    }

    pub(crate) extern "C" fn count_nftw(_: *const c_char, _: *const c_void, _: c_int, _: *mut c_void) -> c_int {
        CALLS.set(CALLS.get() + 1);
        c_int::from(STOP_AT.get() != 0 && CALLS.get() == STOP_AT.get())
    }

    pub(crate) extern "C" fn alpha(a: *const *const c_void, b: *const *const c_void) -> c_int {
        s::alphasort(a, b)
    }

    /// The names of `scandir`'s list.
    pub(crate) fn listed(list: *mut *mut struct_dirent, n: c_int) -> Vec<String> {
        let n = usize::try_from(n).unwrap();
        let words = cbytes(list.cast_const().cast(), n * 8);
        (0..n)
            .map(|i| {
                let p = usize::try_from(u64::from_ne_bytes(words[i * 8..i * 8 + 8].try_into().unwrap())).unwrap();
                String::from_utf8_lossy(&ctext(core::ptr::with_exposed_provenance::<u8>(p).wrapping_add(19).cast())).into_owned()
            })
            .collect()
    }
}

// ------------------------------------------------------------------------------ §13.13 helpers

use p13::*;

/// Helpers of the mount-table tests (§13.13): entries shown as text, read through the library's
/// own `strlen`/`memcpy`. getmntent's block is the calling thread's; the fstab cursor is
/// process-wide, so the tests that move it run in a child.
mod p13 {
    use std::ffi::CString;

    use liella_abi::ctypes::c_char;
    use liella_abi::layout::{struct_fstab, struct_mntent, FILE};
    use liella_boundary::posix::s13_filesystem as s;

    use super::p12::{cbytes, ctext};

    fn text_at(b: &[u8], i: usize) -> String {
        let a = u64::from_ne_bytes(b[i * 8..i * 8 + 8].try_into().unwrap()) as usize;
        String::from_utf8_lossy(&ctext(core::ptr::with_exposed_provenance::<c_char>(a))).into_owned()
    }

    fn int_at(b: &[u8], at: usize) -> i32 {
        i32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// `[fsname] [dir] [type] [opts] freq passno`, or `NULL`.
    pub(crate) fn show(m: *const struct_mntent) -> String {
        if m.is_null() {
            return "NULL".into();
        }
        let b = cbytes(m.cast(), 40);
        format!("[{}] [{}] [{}] [{}] {} {}", text_at(&b, 0), text_at(&b, 1), text_at(&b, 2), text_at(&b, 3), int_at(&b, 32), int_at(&b, 36))
    }

    /// `[spec] [file] [vfstype] [mntops] freq passno` and `fs_type`, or `None`.
    pub(crate) fn show_fs(f: *const struct_fstab) -> Option<(String, String)> {
        if f.is_null() {
            return None;
        }
        let b = cbytes(f.cast(), 48);
        Some((format!("[{}] [{}] [{}] [{}] {} {}", text_at(&b, 0), text_at(&b, 1), text_at(&b, 2), text_at(&b, 3), int_at(&b, 40), int_at(&b, 44)), text_at(&b, 4)))
    }

    pub(crate) fn mopen(path: &CString, mode: &str) -> *mut FILE {
        let f = s::setmntent(path.as_ptr(), CString::new(mode).unwrap().as_ptr());
        assert!(!f.is_null(), "setmntent {path:?} {mode}");
        f
    }

    /// The entries of `/etc/fstab` as getmntent reads them (empty when there is no such file).
    pub(crate) fn fstab_entries() -> Vec<String> {
        let f = s::setmntent(c"/etc/fstab".as_ptr(), c"r".as_ptr());
        if f.is_null() {
            return Vec::new();
        }
        let mut v = Vec::new();
        loop {
            let m = s::getmntent(f);
            if m.is_null() {
                break;
            }
            v.push(show(m));
        }
        s::endmntent(f);
        v
    }

    /// The `n`th string field of a shown entry.
    pub(crate) fn field(shown: &str, n: usize) -> String {
        shown.trim_start_matches('[').split("] [").nth(n).unwrap().trim_end_matches(|c: char| c != ']').trim_end_matches(']').to_string()
    }

    /// A `struct mntent` naming C strings the caller keeps alive.
    pub(crate) fn ment(s: [&CString; 4], freq: i32, passno: i32) -> struct_mntent {
        let a = |c: &CString| c.as_ptr().expose_provenance();
        struct_mntent { mnt_fsname: a(s[0]), mnt_dir: a(s[1]), mnt_type: a(s[2]), mnt_opts: a(s[3]), mnt_freq: freq, mnt_passno: passno }
    }

    pub(crate) fn zero_ment() -> struct_mntent {
        struct_mntent { mnt_fsname: 0, mnt_dir: 0, mnt_type: 0, mnt_opts: 0, mnt_freq: 0, mnt_passno: 0 }
    }
}
