// Specified from spec §17 (phase 5: §17.1; phase 6: §17.2; phase 10: §17.3–17.6 and `initgroups`).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §17. From §17.3 the formats, lookups, enumerations and login records
//! run through `liella::users` and `liella_legacy::{interim_db, accounting}` on fixture files
//! (paths are parameters; cursors and `utmp` databases of their own), the C conventions through
//! the exports; the system's files are only read. `initgroups` runs as a conformance program in
//! a user namespace.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/scratch.rs"]
mod scratch;

use core::ffi::c_void;
use core::ptr::null_mut;

use liella::error::Errno;
use liella_boundary::errno;
use liella_boundary::posix::s17_users as x;
use liella_boundary::sys::CSys;
use liella_kernel::testing::Faulty;
use liella::users;
use liella_abi::ctypes::c_int;
use liella_abi::layout::{struct_group, struct_passwd, struct_spwd};
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as sio;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_RDWR};
use liella_kernel::Fd;
use liella_legacy::accounting as acct;
use liella_legacy::cred_compat;
use liella_legacy::interim_db::{self, Key};

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

/// A fixture file under the target directory holding `content` (removed when the first value is
/// dropped), and its NUL-terminated path.
fn fixture(tag: &str, content: &str) -> (scratch::Scratch, Vec<u8>) {
    let p = scratch::Scratch::path(&format!("c17-{tag}"));
    std::fs::write(&p, content).unwrap();
    let z = p.c().into_bytes_with_nul();
    (p, z)
}

/// A user record as `name:passwd:uid:gid:gecos:dir:shell`.
fn pw(p: &users::Passwd<'_>) -> String {
    let t = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    format!("{}:{}:{}:{}:{}:{}:{}", t(p.name), t(p.passwd), p.uid, p.gid, t(p.gecos), t(p.dir), t(p.shell))
}

/// A group record as `name:passwd:gid:members`.
fn gr(g: &users::Group<'_>) -> String {
    let t = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    format!("{}:{}:{}:{}", t(g.name), t(g.passwd), g.gid, g.members.iter().map(t).collect::<Vec<_>>().join(","))
}

/// Every record of `file` an enumeration returns, through a cursor of its own.
fn enumerate<T>(file: &[u8], parse: impl Fn(&[u8]) -> Option<T>) -> Vec<T> {
    let c = interim_db::Cursor::new();
    let mut out = Vec::new();
    while let Some(v) = c.next(&CSys, file, |l| parse(l).map_or(interim_db::Step::Skip, interim_db::Step::Take)).unwrap() {
        out.push(v);
    }
    c.end(&CSys);
    out
}

const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n# comment\n\n+nis::::::\nalice:x:1000:1000:Alice,,,:/home/alice:/bin/sh\nbad:x:-1:1:::\ndup:x:1000:5:second:/:/s\n  bob:*:1001:100::/home/bob:\n";













/// A login record of `kind` on `line` with `id` for `user`.
fn rec(kind: i16, line: &str, id: &str, user: &str) -> users::Utmp {
    let mut u = users::Utmp { kind, pid: 42, ..users::Utmp::default() };
    users::set_field(&mut u.line, line.as_bytes());
    users::set_field(&mut u.id, id.as_bytes());
    users::set_field(&mut u.user, user.as_bytes());
    u
}

fn line_of(u: &users::Utmp) -> String {
    format!("{} {} {}", u.kind, String::from_utf8_lossy(users::field_text(&u.line)), String::from_utf8_lossy(users::field_text(&u.user)))
}

#[test]
fn s17_1_c_0() {
    let (mut r, mut e, mut s) = (0, 0, 0);
    x::getresuid(Some(&mut r), Some(&mut e), Some(&mut s));
    assert_eq!((r, e), (x::getuid(), x::geteuid()), "real and effective, and the saved one");
    let (mut gr, mut ge, mut gs) = (0, 0, 0);
    x::getresgid(Some(&mut gr), Some(&mut ge), Some(&mut gs));
    assert_eq!((gr, ge), (x::getgid(), x::getegid()));
    let n = x::getgroups(0, null_mut());
    assert!(n >= 0, "getgroups(0, NULL): the count");
    let mut l = vec![0u32; usize::try_from(n).unwrap()];
    x::getgroups(n, l.as_mut_ptr());
    assert!(l.iter().all(|&g| x::group_member(g) == 1));
    assert_eq!(x::group_member(x::getegid()), 1, "the effective GID counts, even when not listed");
}

#[test]
fn s17_1_g_0() {
    // The ID queries have no error channel; getgroups fails only for a short buffer (and a
    // kernel answer injected as a failure is reported, not hidden).
    errno::set_raw(0);
    for _ in 0..3 {
        let _ = (x::getuid(), x::geteuid(), x::getgid(), x::getegid());
    }
    assert_eq!(errno::get(), 0, "never fail, never touch errno");
    let n = x::getgroups(0, null_mut());
    if n > 0 {
        let mut one = vec![0u32; usize::try_from(n - 1).unwrap()];
        assert_eq!((x::getgroups(n - 1, one.as_mut_ptr()), errno::get()), (-1, Errno::EINVAL.get()));
    }
    let k = Faulty::new(CSys);
    k.fail("getgroups", Errno::EINVAL, 0, 1);
    assert_eq!(cred_compat::getgroups(&k, 4, &mut [0u32; 4]), Err(Errno::EINVAL));
}

/// §17.2 Contract: Change the process's user or group identities, following the POSIX rules:
#[test]
fn s17_2_c_0() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
/// §17.2 Contract: A privileged process (one with `CAP_SETUID`/`CAP_SETGID`) may set any values.
#[test]
fn s17_2_c_1() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
/// §17.2 Contract: An unprivileged process may only switch among its real, effective and saved IDs.
#[test]
fn s17_2_c_2() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
/// §17.2 Contract: `setuid` with privilege sets all three. Also:
#[test]
fn s17_2_c_3() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
/// §17.2 Contract: **`setgroups`** replaces the supplementary list. It requires privilege, and is denied in a user namespace when `/proc/self/setgroups` is `deny`.
#[test]
fn s17_2_c_4() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
/// §17.2 Contract: **`initgroups(user, g)`** sets the supplementary list to all groups that `user` belongs to, plus `g`.
#[test]
fn s17_2_c_5() {
    // The list (fixture group file): `g` first, then each group naming the user (a shared GID once per group).
    let (_g, group) = fixture("c5-group", "a:x:10:alice\nb:x:11:bob, alice\nc:x:10:alice\n+x:x:0:alice\nd:x:12:alicex\n");
    assert_eq!(interim_db::group_list(&CSys, &group, b"alice", 99).unwrap().as_slice(), [99, 10, 11, 10]);
    assert_eq!(interim_db::group_list(&CSys, &group, b"nobody", 99).unwrap().as_slice(), [99]);
    // Installed process-wide (a privileged process in a user namespace).
    conformance(&["s17_initgroups"]);
}
/// §17.2 Contract: **`setfsuid`/`setfsgid`** set the file-system IDs used for access checks. They return the previous value and *cannot report failure*. ⚑ HIST.
#[test]
fn s17_2_c_6() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
/// §17.2 Guarantees: **The change applies to every thread in the process atomically**, as POSIX requires.
#[test]
fn s17_2_g_0() {
    // Atomic for the process: a thread that cannot follow ends it (s17_setxid_abort).
    conformance(&["s17_setxid", "s17_setxid_ids", "s17_setxid_abort"]);
}
/// §17.2 Failure: `EPERM`, `EINVAL`, `EAGAIN` (`RLIMIT_NPROC` on some kernels).
#[test]
fn s17_2_f_0() {
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
#[test]
fn s17_3_c_1() {
    // By name and by UID, the first matching record; compatibility entries are no users.
    let (_g, f) = fixture("c1-passwd", PASSWD);
    let k = &CSys;
    let by = |key| interim_db::passwd_by(k, &f, key, pw).unwrap();
    assert_eq!(by(Key::Name(b"alice")).as_deref(), Some("alice:x:1000:1000:Alice,,,:/home/alice:/bin/sh"));
    assert_eq!(by(Key::Id(1000)).as_deref(), Some("alice:x:1000:1000:Alice,,,:/home/alice:/bin/sh"), "the first of two with the UID");
    assert_eq!(by(Key::Name(b"bob")).as_deref(), Some("bob:*:1001:100::/home/bob:"), "white space before the name is skipped");
    assert_eq!(by(Key::Name(b"+nis")), None);
    assert_eq!(by(Key::Id(0)).as_deref(), Some("root:x:0:0:root:/root:/bin/bash"), "not the compatibility entry's empty UID");
    assert_eq!(by(Key::Name(b"bad")), None, "a line that does not parse");
    // Enumeration: every record in file order, compatibility entries included.
    let names: Vec<String> = enumerate(&f, |l| users::passwd(l).map(|p| String::from_utf8_lossy(p.name).into_owned()));
    assert_eq!(names, ["root", "+nis", "alice", "dup", "bob"]);
    // Through the exports, the system's database answers as the `files` source reads it.
    let want = interim_db::passwd_by(k, interim_db::PASSWD, Key::Name(b"root"), |p| p.uid).unwrap();
    let p = x::getpwnam(c"root".as_ptr());
    assert_eq!(want.is_some(), !p.is_null());
}
#[test]
fn s17_3_c_2() {
    let (_g, f) = fixture("c2-group", "root:x:0:\nusers:x:100: alice ,,bob\n+:::\nshort:x\nwheel:x:10:alice\n");
    let k = &CSys;
    let by = |key| interim_db::group_by(k, &f, key, gr).unwrap();
    assert_eq!(by(Key::Name(b"users")).as_deref(), Some("users:x:100:alice ,bob"), "members: white space before kept off, empty entries dropped");
    assert_eq!(by(Key::Id(10)).as_deref(), Some("wheel:x:10:alice"));
    assert_eq!(by(Key::Name(b"+")), None);
    assert_eq!(by(Key::Name(b"short")), None);
    let all: Vec<String> = enumerate(&f, |l| users::group(l).map(|g| gr(&g)));
    assert_eq!(all, ["root:x:0:", "users:x:100:alice ,bob", "+::0:", "wheel:x:10:alice"]);
    let p = x::getgrgid(0);
    assert_eq!(interim_db::group_by(k, interim_db::GROUP, Key::Id(0), |_| ()).unwrap().is_some(), !p.is_null());
}
#[test]
fn s17_3_c_3() {
    // `putpwent`/`putgrent` write lines that the `f` forms read back as the same records.
    let (path, z) = fixture("c3", "");
    let cpath = std::ffi::CString::new(&z[..z.len() - 1]).unwrap();
    let fp = sio::fopen(cpath.as_ptr(), c"w".as_ptr()).cast::<c_void>();
    let t = |c: &'static core::ffi::CStr| c.as_ptr() as usize;
    let p = struct_passwd { pw_name: t(c"carol"), pw_passwd: t(c"x"), pw_uid: 1002, pw_gid: 1002, pw_gecos: t(c"Carol"), pw_dir: t(c"/home/carol"), pw_shell: t(c"/bin/sh") };
    assert_eq!(x::putpwent(Some(&p), fp), 0);
    let mem = [t(c"carol"), t(c"dave"), 0];
    let g = struct_group { gr_name: t(c"staff"), gr_passwd: t(c"x"), gr_gid: 50, gr_mem: mem.as_ptr() as usize };
    assert_eq!(x::putgrent(Some(&g), fp), 0);
    assert_eq!(sio::fclose(fp.cast()), 0);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, "carol:x:1002:1002:Carol:/home/carol:/bin/sh\nstaff:x:50:carol,dave\n");
    let fp = sio::fopen(cpath.as_ptr(), c"r".as_ptr()).cast::<c_void>();
    let r = x::fgetpwent(fp);
    assert!(!r.is_null(), "the passwd line reads back");
    let mut back = struct_passwd { pw_name: 0, pw_passwd: 0, pw_uid: 0, pw_gid: 0, pw_gecos: 0, pw_dir: 0, pw_shell: 0 };
    st::memcpy(core::ptr::from_mut(&mut back).cast(), r.cast(), size_of::<struct_passwd>());
    assert_eq!((back.pw_uid, back.pw_gid), (1002, 1002));
    // The group line is not a passwd record (three fields after the name): the end.
    assert!(x::fgetpwent(fp).is_null());
    assert_eq!(sio::fclose(fp.cast()), 0);
    assert_eq!(users::group(text.lines().nth(1).unwrap().as_bytes()).map(|g| gr(&g)).as_deref(), Some("staff:x:50:carol,dave"));
}
#[test]
fn s17_3_c_4() {
    // The groups listing the user, the base group first, one entry per group (a GID two groups share twice); too small an array is -1
    // with the count needed.
    let (_g, f) = fixture("c4-group", "a:x:10:alice\nb:x:20:alice,bob\nbase:x:7:alice\nc:x:20:alice\n");
    assert_eq!(interim_db::group_list(&CSys, &f, b"alice", 7).unwrap().as_slice(), [7, 10, 20, 20]);
    let mut list = [0u32; 1];
    let mut n: c_int = 1;
    let r = x::getgrouplist(c"root".as_ptr(), 0x7fff_fff1, list.as_mut_ptr(), Some(&mut n));
    assert_eq!(list[0], 0x7fff_fff1, "the base group first");
    assert_eq!(r, if n <= 1 { n } else { -1 }, "the count, or -1 with the count needed in n");
    let mut none: c_int = 0;
    assert_eq!(x::getgrouplist(c"root".as_ptr(), 0x7fff_fff1, list.as_mut_ptr(), Some(&mut none)), -1);
    assert!(none >= 1, "the count needed includes the base group");
}
#[test]
fn s17_3_c_5() {
    let mut buf = [0u8; 2048];
    if interim_db::passwd_by(&CSys, interim_db::PASSWD, Key::Id(0), |_| ()).unwrap().is_none() {
        skip_note::skipped("skipped: no UID 0 in /etc/passwd");
        return;
    }
    assert_eq!(x::getpw(0, buf.as_mut_ptr().cast()), 0);
    let n = buf.iter().position(|&c| c == 0).unwrap();
    let want = interim_db::passwd_by(&CSys, interim_db::PASSWD, Key::Id(0), pw).unwrap().unwrap();
    assert_eq!(String::from_utf8_lossy(&buf[..n]), want, "the passwd(5) line, without a newline");
}
#[test]
fn s17_3_g_0() {
    // The `files` source is the only source (D-15.6): the exports answer as the interim lookup.
    let k = &CSys;
    for uid in [0, x::getuid(), 0x7fff_fff0] {
        let want = interim_db::passwd_by(k, interim_db::PASSWD, Key::Id(uid), |p| String::from_utf8_lossy(p.name).into_owned()).unwrap();
        let p = x::getpwuid(uid);
        assert_eq!(want.is_some(), !p.is_null(), "uid {uid}");
    }
    // Static storage: one record per thread for each kind, overwritten by the next call.
    let a = x::getpwuid(0) as usize;
    let b = x::getpwnam(c"root".as_ptr()) as usize;
    if a != 0 {
        assert_eq!(a, b, "the same storage for the user lookups");
        let other = std::thread::spawn(|| x::getpwuid(0) as usize).join().unwrap();
        assert_ne!(other, a, "per thread");
    }
    // The `_r` forms use the caller's storage.
    let mut buf = [0u8; 1024];
    let mut out = struct_passwd { pw_name: 0, pw_passwd: 0, pw_uid: 0, pw_gid: 0, pw_gecos: 0, pw_dir: 0, pw_shell: 0 };
    let mut res = 0usize;
    if x::getpwuid_r(0, Some(&mut out), buf.as_mut_ptr().cast(), 1024, Some(&mut res)) == 0 && res != 0 {
        let base = buf.as_ptr() as usize;
        assert!(out.pw_name >= base && out.pw_name < base + 1024);
    }
}
#[test]
fn s17_3_f_1() {
    // Not found: NULL, and errno as it was (0 or anything else).
    for before in [0, 12345] {
        errno::set_raw(before);
        assert!(x::getpwnam(c"no-such-user-here".as_ptr()).is_null());
        assert!(x::getgrnam(c"no-such-group-here".as_ptr()).is_null());
        assert!(x::getpwuid(0x7fff_fff0).is_null());
        assert_eq!(errno::get(), before);
    }
    // A database that cannot be read is an error, not "not found".
    let k = &CSys;
    assert_eq!(interim_db::passwd_by(k, b"/nonexistent/passwd\0", Key::Id(0), |_| ()), Err(Errno::ENOENT));
    // `_r`: 0 with a NULL result when not found; ERANGE for a buffer too small.
    let mut out = struct_passwd { pw_name: 0, pw_passwd: 0, pw_uid: 0, pw_gid: 0, pw_gecos: 0, pw_dir: 0, pw_shell: 0 };
    let (mut buf, mut res) = ([0u8; 1024], 1usize);
    assert_eq!(x::getpwnam_r(c"no-such-user-here".as_ptr(), Some(&mut out), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(res, 0);
    if !x::getpwuid(0).is_null() {
        assert_eq!(x::getpwuid_r(0, Some(&mut out), buf.as_mut_ptr().cast(), 1, Some(&mut res)), Errno::ERANGE.get());
        assert_eq!(res, 0);
    }
}
#[test]
fn s17_4_c_0() {
    // The same pattern for `struct spwd` and `struct sgrp` (fixture files).
    let (_g, sp) = fixture("c40-shadow", "root:!:19000:0:99999:7:::\n+x::::::::\nalice:$6$s$h:19500:1:90:14:30:20000:\nbad:x:a:1:2:3:4:5:\n");
    let k = &CSys;
    let aging = interim_db::shadow_by(k, &sp, b"alice", |s| (s.aging, s.flag)).unwrap();
    assert_eq!(aging, Some(([19500, 1, 90, 14, 30, 20000], u64::MAX)), "the aging fields; an empty flag is all ones");
    assert_eq!(interim_db::shadow_by(k, &sp, b"root", |s| s.aging).unwrap(), Some([19000, 0, 99999, 7, -1, -1]), "empty fields are -1");
    assert_eq!(interim_db::shadow_by(k, &sp, b"bad", |_| ()).unwrap(), None);
    let (_g, sg) = fixture("c40-gshadow", "users:!:alice:alice,bob\nwheel::a1,a2:\n");
    let g = interim_db::gshadow_by(k, &sg, b"wheel", |g| (g.admins.iter().count(), g.members.iter().count())).unwrap();
    assert_eq!(g, Some((2, 0)));
    // The `s` forms parse one line.
    let p = x::sgetspent(c"carol:hash:1:2:3:4:5:6:".as_ptr());
    assert!(!p.is_null());
    let mut s = struct_spwd { sp_namp: 0, sp_pwdp: 0, sp_lstchg: 0, sp_min: 0, sp_max: 0, sp_warn: 0, sp_inact: 0, sp_expire: 0, sp_flag: 0 };
    st::memcpy(core::ptr::from_mut(&mut s).cast(), p.cast(), size_of::<struct_spwd>());
    assert_eq!((s.sp_lstchg, s.sp_expire, s.sp_flag), (1, 6, u64::MAX));
    assert!(x::sgetspent(c"carol:hash".as_ptr()).is_null(), "too few fields");
    assert!(!x::sgetsgent(c"g:p:a:m".as_ptr()).is_null());
}
#[test]
fn s17_5_c_1() {
    let (_g, passwd) = fixture("c51-passwd", "root:x:0:0:root:/root:/bin/sh\nalice:x:1000:1000::/home/alice:/bin/sh\n");
    let (_g, set) = fixture("c51-loginuid-set", "1000");
    let (_g, unset) = fixture("c51-loginuid-unset", "4294967295");
    let mut rec = users::Utmp { kind: users::USER_PROCESS, pid: 7, ..users::Utmp::default() };
    users::set_field(&mut rec.line, b"pts/9");
    users::set_field(&mut rec.user, b"bob");
    let (utmp_path, utmp) = fixture("c51-utmp", "");
    std::fs::write(&utmp_path, rec.to_bytes()).unwrap();
    let k = &CSys;
    let name = |loginuid: &[u8], tty: Option<&[u8]>| acct::login_name(k, acct::LoginFiles { loginuid, passwd: &passwd, utmp: &utmp }, tty, |n| String::from_utf8_lossy(n).into_owned());
    assert_eq!(name(&set, None).as_deref(), Ok("alice"), "the audit login UID through the user database");
    assert_eq!(name(&unset, Some(b"pts/9")).as_deref(), Ok("bob"), "unset: the utmp entry of the terminal");
    let (_g, other) = fixture("c51-loginuid-unknown", "4242");
    assert_eq!(name(&other, Some(b"pts/9")).as_deref(), Ok("bob"), "a UID the database does not know: the utmp entry too");
    // Through the export, as the session's files say.
    let p = x::getlogin();
    let uid: u32 = std::fs::read_to_string("/proc/self/loginuid").ok().and_then(|t| t.trim().parse().ok()).unwrap_or(u32::MAX);
    if let Some(want) = interim_db::passwd_by(k, interim_db::PASSWD, Key::Id(uid), |p| p.name.len()).unwrap_or(None) {
        assert!(!p.is_null());
        assert_eq!(st::strlen(p), want);
    }
}
#[test]
fn s17_5_c_2() {
    let (_g, ttys) = fixture("c52-ttys", "# terminals\n\nconsole\n  tty1 getty vt100 on # comment\npts/0\n");
    let k = &CSys;
    assert_eq!(acct::tty_slot(k, &ttys, b"tty1"), 2, "the index among the entries, from 1");
    assert_eq!(acct::tty_slot(k, &ttys, b"pts/0"), 3);
    assert_eq!(acct::tty_slot(k, &ttys, b"pts/1"), 0, "none: 0");
    assert_eq!(acct::tty_slot(k, b"/nonexistent/ttys\0", b"tty1"), 0);
}
#[test]
fn s17_5_f_0() {
    let (_g, passwd) = fixture("c5f-passwd", "root:x:0:0::/:/bin/sh\n");
    let (_g, unset) = fixture("c5f-loginuid", "4294967295");
    let (_g, utmp) = fixture("c5f-utmp", "");
    let k = &CSys;
    let files = acct::LoginFiles { loginuid: &unset, passwd: &passwd, utmp: &utmp };
    assert_eq!(acct::login_name(k, files, None, |_| ()), Err(Errno::ENXIO), "no terminal");
    assert_eq!(acct::login_name(k, files, Some(b"pts/4"), |_| ()), Err(Errno::ENOENT), "no record for the terminal");
}
#[test]
fn s17_5a_c_0() {
    if std::env::var("LIELLA_C17_SHELLS").is_ok() {
        let (_g, f) = fixture("c5a-shells", "/bin/sh\n# comment\n\n  /usr/bin/zsh  \nrelative\n/bin/bash # trailing comment\n/bin/csh");
        let k = &CSys;
        let next = || acct::next_shell(k, &f, |s| String::from_utf8_lossy(s).into_owned());
        assert_eq!(next().as_deref(), Some("/bin/sh"));
        let f2 = f.clone();
        assert_eq!(std::thread::spawn(move || acct::next_shell(&CSys, &f2, |s| String::from_utf8_lossy(s).into_owned())).join().unwrap().as_deref(), Some("/usr/bin/zsh"), "the cursor is the process's");
        assert_eq!(next().as_deref(), Some("/bin/bash"));
        assert_eq!(next().as_deref(), Some("/bin/csh"));
        assert_eq!(next(), None);
        acct::rewind_shells(k);
        let missing = || acct::next_shell(k, b"/nonexistent/shells\0", |s| String::from_utf8_lossy(s).into_owned());
        assert_eq!((missing(), missing(), missing()), (Some("/bin/sh".into()), Some("/bin/csh".into()), None), "no file: /bin/sh and /bin/csh");
        return;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args(["s17_5a_c_0", "--exact", "--test-threads=1"]).env("LIELLA_C17_SHELLS", "1").output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}
#[test]
fn s17_6_c_0() {
    // Read and write: records written are read back, in order (a cursor of its own).
    let (_g, f) = fixture("c60-utmp", "");
    let db = acct::UtmpDb::new();
    let k = &CSys;
    db.set_name(k, &f[..f.len() - 1]).unwrap();
    db.rewind(k).unwrap();
    db.put(k, &rec(users::LOGIN_PROCESS, "tty1", "1", "LOGIN")).unwrap();
    db.put(k, &rec(users::USER_PROCESS, "pts/0", "p0", "alice")).unwrap();
    db.rewind(k).unwrap();
    let all: Vec<String> = std::iter::from_fn(|| db.next(k).unwrap()).map(|u| line_of(&u)).collect();
    assert_eq!(all, ["6 tty1 LOGIN", "7 pts/0 alice"]);
    db.close(k);
}
#[test]
fn s17_6_c_1() {
    assert_eq!(acct::UTMP, b"/var/run/utmp\0", "the default file of the cursor");
    // An unnamed cursor opens it (reading only: an unprivileged test cannot write it).
    let db = acct::UtmpDb::new();
    if std::path::Path::new("/var/run/utmp").exists() {
        assert!(db.rewind(&CSys).is_ok());
        db.close(&CSys);
    }
}
#[test]
fn s17_6_c_2() {
    assert_eq!(acct::WTMP, b"/var/log/wtmp\0");
    // History: records are appended (a fixture history file).
    let (path, f) = fixture("c62-wtmp", "");
    let k = &CSys;
    acct::append(k, &f, &rec(users::USER_PROCESS, "pts/1", "p1", "bob")).unwrap();
    acct::append(k, &f, &acct::wtmp_record(b"pts/1", b"", b"", 42, (100, 5))).unwrap();
    let b = std::fs::read(&path).unwrap();
    assert_eq!(b.len(), 2 * users::UTMP_SIZE);
    let second = users::Utmp::from_bytes(b[users::UTMP_SIZE..].try_into().unwrap());
    assert_eq!((second.kind, second.time), (users::DEAD_PROCESS, (100, 5)), "a logout record");
    assert_eq!(acct::append(k, b"/nonexistent/wtmp\0", &second), Err(Errno::ENOENT), "not created");
}
#[test]
fn s17_6_c_3() {
    // btmp (failed logins) has the same records; every field survives the file and the C layout.
    let mut u = rec(users::LOGIN_PROCESS, "tty7", "7", "mallory");
    users::set_field(&mut u.host, b"attacker.example");
    u.exit = [3, 4];
    u.session = 99;
    u.time = (1_700_000_000, 123_456);
    u.addr = [1, 2, 3, 4];
    let b = u.to_bytes();
    assert_eq!(b.len(), 384);
    assert_eq!(users::Utmp::from_bytes(&b), u);
    assert_eq!(liella_abi::pack::utmp_in(&liella_abi::pack::utmp(&u)), u);
    assert_eq!(size_of::<liella_abi::layout::struct_utmp>(), 384);
    // Offsets of Appendix C: ut_line 8, ut_id 40, ut_user 44, ut_host 76, ut_tv 340.
    assert_eq!(&b[8..12], b"tty7");
    assert_eq!(&b[40..41], b"7");
    assert_eq!(&b[44..51], b"mallory");
    assert_eq!(i32::from_ne_bytes(b[340..344].try_into().unwrap()), 1_700_000_000);
    let (path, f) = fixture("c63-btmp", "");
    acct::append(&CSys, &f, &u).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b);
}
#[test]
fn s17_6_c_4() {
    let (_g, f) = fixture("c64-utmp", "");
    let db = acct::UtmpDb::new();
    let k = &CSys;
    db.set_name(k, &f[..f.len() - 1]).unwrap();
    db.rewind(k).unwrap();
    for u in [rec(users::LOGIN_PROCESS, "tty1", "1", "LOGIN"), rec(users::BOOT_TIME, "~", "~~", "reboot"), rec(users::USER_PROCESS, "pts/0", "ts/0", "alice")] {
        db.put(k, &u).unwrap();
    }
    db.rewind(k).unwrap();
    assert_eq!(line_of(&db.find_id(k, &rec(users::DEAD_PROCESS, "", "ts/0", "")).unwrap()), "7 pts/0 alice", "by ID among process records");
    db.rewind(k).unwrap();
    assert_eq!(line_of(&db.find_id(k, &rec(users::BOOT_TIME, "", "", "")).unwrap()), "2 ~ reboot", "by type among time records");
    assert_eq!(db.find_id(k, &rec(users::EMPTY, "", "1", "")), Err(Errno::EINVAL));
    db.rewind(k).unwrap();
    assert_eq!(line_of(&db.find_line(k, &rec(0, "tty1", "", "")).unwrap()), "6 tty1 LOGIN", "by line");
    assert_eq!(db.find_line(k, &rec(0, "tty1", "", "")), Err(Errno::ESRCH), "forward from the position");
    // pututline replaces the record getutid finds.
    db.rewind(k).unwrap();
    db.put(k, &rec(users::DEAD_PROCESS, "pts/0", "ts/0", "")).unwrap();
    db.rewind(k).unwrap();
    let all: Vec<String> = std::iter::from_fn(|| db.next(k).unwrap()).map(|u| line_of(&u)).collect();
    assert_eq!(all, ["6 tty1 LOGIN", "2 ~ reboot", "8 pts/0 "]);
    db.close(k);
}
#[test]
fn s17_6_c_5() {
    // login: the record replaces the one with its ID (or is appended) in utmp and is appended to
    // wtmp; logout: the session on the line becomes a dead process; logwtmp: an appended record.
    let (upath, utmp) = fixture("c65-utmp", "");
    let (wpath, wtmp) = fixture("c65-wtmp", "");
    let k = &CSys;
    let mut u = rec(users::USER_PROCESS, "pts/3", "p3", "carol");
    users::set_field(&mut u.host, b"h.example");
    acct::put_in(k, &utmp, &u).unwrap();
    acct::put_in(k, &utmp, &u).unwrap();
    acct::append(k, &wtmp, &u).unwrap();
    assert_eq!(std::fs::metadata(&upath).unwrap().len(), 384, "replaced, not appended twice");
    assert_eq!(acct::logout_in(k, &utmp, b"pts/3", (500, 6)), Ok(true));
    let after = users::Utmp::from_bytes(std::fs::read(&upath).unwrap()[..384].try_into().unwrap());
    assert_eq!((after.kind, users::field_text(&after.user), users::field_text(&after.host), after.time), (users::DEAD_PROCESS, &b""[..], &b""[..], (500, 6)));
    assert_eq!(users::field_text(&after.line), b"pts/3", "the line stays");
    assert_eq!(acct::logout_in(k, &utmp, b"pts/3", (501, 0)), Ok(false), "already dead");
    let w = acct::wtmp_record(b"ttyX", b"bob", b"remote", 9, (1, 2));
    assert_eq!((w.kind, w.pid, users::field_text(&w.host)), (users::USER_PROCESS, 9, &b"remote"[..]));
    acct::append(k, &wtmp, &w).unwrap();
    assert_eq!(std::fs::metadata(&wpath).unwrap().len(), 768);
    assert!(acct::now(k).unwrap().0 > 1_600_000_000, "logout and logwtmp stamp the current time");
    // D-17.5 (owner ruling 2026-10-22, #415): the 32-bit ut_tv holds unsigned seconds, so times
    // to 2106-02-07 06:28:15 UTC are kept; a time outside [0, 2^32) is stamped and written as no
    // time, (0, 0), never wrapped. Pure functions of the given time: the clock is never set.
    const MAX: i64 = 0xFFFF_FFFF;
    assert_eq!(users::UT_SEC_MAX, MAX);
    assert_eq!(acct::stamp(0, 0), (0, 0), "the epoch");
    assert_eq!(acct::stamp(i64::from(i32::MAX), 999_999_000), (i64::from(i32::MAX), 999_999), "2038-01-19 03:14:07");
    assert_eq!(acct::stamp(i64::from(i32::MAX) + 1, 5_000), (i64::from(i32::MAX) + 1, 5), "2038-01-19 03:14:08 is kept");
    assert_eq!(acct::stamp(MAX, 1_000), (MAX, 1), "2106-02-07 06:28:15, the last second that fits");
    assert_eq!(acct::stamp(MAX + 1, 0), (0, 0), "2106-02-07 06:28:16: no time, not wrapped to 0");
    assert_eq!(acct::stamp(1 << 33, 7_000), (0, 0), "far future");
    assert_eq!(acct::stamp(-1, 0), (0, 0), "before 1970");
    assert_eq!(acct::stamp(i64::MIN, 0), (0, 0));
    // The file bytes: before 2038 as the signed field, after it the unsigned value.
    let mut late = w;
    for (sec, raw) in [(0i64, 0u32), (1, 1), (i64::from(i32::MAX), 0x7FFF_FFFF), (i64::from(i32::MAX) + 1, 0x8000_0000), (3_000_000_000, 3_000_000_000), (MAX - 1, 0xFFFF_FFFE), (MAX, 0xFFFF_FFFF)] {
        late.time = (sec, 7);
        let bytes = late.to_bytes();
        assert_eq!(bytes[340..344], raw.to_ne_bytes(), "{sec} as stored");
        if let Ok(old) = i32::try_from(sec) {
            assert_eq!(bytes[340..344], old.to_ne_bytes(), "{sec}: the bytes of the signed field");
        }
        assert_eq!(users::Utmp::from_bytes(&bytes).time, (sec, 7), "{sec} round trip through the file");
        let c = liella_abi::pack::utmp(&late);
        assert_eq!(c.ut_tv.tv_sec.to_ne_bytes(), raw.to_ne_bytes(), "{sec}: the C member holds the same 32 bits");
        assert_eq!(liella_abi::pack::utmp_in(&c).time, (sec, 7), "{sec}: and reads back as unsigned");
    }
    for sec in [-5, -1, MAX + 1, 1 << 32, i64::MAX, i64::MIN] {
        late.time = (sec, 1);
        assert_eq!(users::Utmp::from_bytes(&late.to_bytes()).time, (0, 0), "{sec}: written as no time");
        assert_eq!((liella_abi::pack::utmp(&late).ut_tv.tv_sec, liella_abi::pack::utmp(&late).ut_tv.tv_usec), (0, 0), "{sec}: and so to C");
    }
    // A C caller's 32-bit value is written as given: a negative member is the unsigned time.
    let mut c = liella_abi::pack::utmp(&w);
    c.ut_tv.tv_sec = -5;
    assert_eq!(liella_abi::pack::utmp_in(&c).time.0, MAX - 4);
    assert_eq!(liella_abi::pack::utmp(&liella_abi::pack::utmp_in(&c)).ut_tv.tv_sec, -5, "the caller's bits unchanged");
    late.time = (3_000_000_000, 9);
    acct::append(k, &wtmp, &late).unwrap();
    assert_eq!(std::fs::metadata(&wpath).unwrap().len(), 1152, "the record is still written");
    let tail: [u8; 384] = std::fs::read(&wpath).unwrap()[768..].try_into().unwrap();
    assert_eq!(users::Utmp::from_bytes(&tail).time, (3_000_000_000, 9), "a 2065 time in the file reads back");
}
#[test]
fn s17_6_c_6() {
    let (_g, a) = fixture("c66-a", "");
    let (_g, b) = fixture("c66-b", "");
    let db = acct::UtmpDb::new();
    let k = &CSys;
    db.set_name(k, &a[..a.len() - 1]).unwrap();
    db.rewind(k).unwrap();
    db.put(k, &rec(users::USER_PROCESS, "pts/1", "1", "in-a")).unwrap();
    db.set_name(k, &b[..b.len() - 1]).unwrap();
    db.rewind(k).unwrap();
    assert_eq!(db.next(k).unwrap(), None, "the other file");
    db.set_name(k, &a[..a.len() - 1]).unwrap();
    assert_eq!(db.next(k).unwrap().map(|u| line_of(&u)).as_deref(), Some("7 pts/1 in-a"));
    assert_eq!(db.set_name(k, &[b'x'; 5000]), Err(Errno::ENAMETOOLONG));
    db.close(k);
}
#[test]
fn s17_6_g_0() {
    assert_eq!(acct::UTMP_LOCK_TIMEOUT_MS, 10_000, "§17.6: 10 s");
    let (_g, f) = fixture("c6g-utmp", "");
    let k = &CSys;
    let path = KStr::from_nul_terminated(&f).unwrap();
    let holder = k.openat(Fd::CWD, path, O_RDWR | O_CLOEXEC, 0).unwrap();
    let other = k.openat(Fd::CWD, path, O_RDWR | O_CLOEXEC, 0).unwrap();
    // A writer waits while another holds the lock, and gives up after the timeout (a short one
    // here), with no signal handler installed.
    acct::lock_file(k, holder, 1, 0).unwrap();
    let t = std::time::Instant::now();
    assert_eq!(acct::lock_file(k, other, 1, 250), Err(Errno::EAGAIN));
    assert!(t.elapsed() >= std::time::Duration::from_millis(250));
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let caught = u64::from_str_radix(status.lines().find_map(|l| l.strip_prefix("SigCgt:")).unwrap().trim(), 16).unwrap();
    assert_eq!(caught & (1 << (14 - 1)), 0, "no SIGALRM handler");
    // Released during the wait: taken.
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        acct::unlock_file(&CSys, holder);
    });
    assert_eq!(acct::lock_file(k, other, 1, 5_000), Ok(()));
    releaser.join().unwrap();
    // A record update through a locked file waits for it too.
    acct::unlock_file(k, other);
    acct::lock_file(k, holder, 0, 0).unwrap();
    let t = std::time::Instant::now();
    let db = acct::UtmpDb::new();
    db.set_name(k, &f[..f.len() - 1]).unwrap();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        acct::unlock_file(&CSys, holder);
    });
    db.put(k, &rec(users::USER_PROCESS, "pts/2", "2", "dave")).unwrap();
    assert!(t.elapsed() >= std::time::Duration::from_millis(100), "the update waited for the reader's lock");
    releaser.join().unwrap();
    db.close(k);
    let _ = (k.close(holder), k.close(other));
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s17_3_f_2() {
    // The _r forms: 0 with a NULL result for not found; ERANGE for a buffer too small (retry
    // with a larger one).
    let mut pw: struct_passwd = unsafe_free_passwd();
    let mut result = 1usize;
    let mut tiny = [0 as core::ffi::c_char; 4];
    assert_eq!(x::getpwnam_r(c"root".as_ptr(), Some(&mut pw), tiny.as_mut_ptr(), tiny.len(), Some(&mut result)), Errno::ERANGE.get());
    assert_eq!(result, 0);
    let mut buf = [0 as core::ffi::c_char; 4096];
    assert_eq!(x::getpwnam_r(c"root".as_ptr(), Some(&mut pw), buf.as_mut_ptr(), buf.len(), Some(&mut result)), 0);
    assert_ne!(result, 0, "found with a large enough buffer");
    result = 1;
    assert_eq!(x::getpwnam_r(c"no-such-user-here".as_ptr(), Some(&mut pw), buf.as_mut_ptr(), buf.len(), Some(&mut result)), 0);
    assert_eq!(result, 0, "not found: 0 and NULL");
}

#[test]
fn s17_3_f_3() {
    // End of an enumeration (D-17.3, #409, as glibc): the non-_r forms return NULL with errno
    // unchanged, again after the end; the _r form returns ENOENT with a NULL result, errno unchanged.
    let end = |next: &dyn Fn() -> bool, what: &str| {
        let mut n = 0u32;
        loop {
            errno::set_raw(77);
            if next() {
                break;
            }
            n += 1;
            assert!(n < 1 << 20, "{what}: the enumeration ends");
        }
        assert_eq!(errno::get(), 77, "{what}: the end leaves errno unchanged");
        assert!(next(), "{what}: NULL again after the end");
        assert_eq!(errno::get(), 77, "{what}: errno unchanged after the end");
    };
    x::setpwent();
    end(&|| x::getpwent().is_null(), "getpwent");
    x::endpwent();
    x::setgrent();
    end(&|| x::getgrent().is_null(), "getgrent");
    x::endgrent();
    x::setpwent();
    let mut pw = unsafe_free_passwd();
    let mut buf = [0 as core::ffi::c_char; 4096];
    let mut result = 1usize;
    let mut r;
    let mut n = 0u32;
    loop {
        errno::set_raw(77);
        r = x::getpwent_r(Some(&mut pw), buf.as_mut_ptr(), buf.len(), Some(&mut result));
        if r != 0 {
            break;
        }
        n += 1;
        assert!(n < 1 << 20, "getpwent_r: the enumeration ends");
    }
    assert_eq!((r, result, errno::get()), (Errno::ENOENT.get(), 0, 77), "getpwent_r: ENOENT, NULL result, errno unchanged");
    x::endpwent();
}

fn unsafe_free_passwd() -> struct_passwd {
    struct_passwd { pw_name: 0, pw_passwd: 0, pw_uid: 0, pw_gid: 0, pw_gecos: 0, pw_dir: 0, pw_shell: 0 }
}

#[test]
fn s17_5_c_3() {
    // getpass: without a terminal, the prompt on standard error and the line from standard
    // input, into the library's buffer (the terminal case, echo and the bound: the boundary's
    // sym_getpass and sym_getpass_bound, D-17.8).
    if std::env::var("LIELLA_S17_GETPASS").is_ok() {
        let p = x::getpass(c"Prompt: ".as_ptr()) as usize;
        let s = core::ptr::with_exposed_provenance::<core::ffi::c_char>(p);
        let n = st::strlen(s);
        let mut v = vec![0u8; n];
        st::memcpy(v.as_mut_ptr().cast(), s.cast(), n);
        println!("line=[{}]", String::from_utf8_lossy(&v));
        return;
    }
    let mut child = std::process::Command::new("setsid")
        .arg(std::env::current_exe().unwrap())
        .args(["s17_5_c_3", "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_S17_GETPASS", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"pass phrase\nrest\n").unwrap();
    drop(child.stdin.take());
    let o = child.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("line=[pass phrase]"), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Prompt: "));
}
