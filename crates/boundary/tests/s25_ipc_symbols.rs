// Specified from spec §25 (phase 5: §25.1–25.2; phase 13: §25.3–25.4).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §25 (28 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! System V objects are system-wide: each test creates private ones (`IPC_PRIVATE`) and
//! removes them. `semctl` is variadic: its fixed-arity target is tested here and the C entry
//! point through real varargs in `conformance/c/s25_semctl.c`.
//!
//! POSIX queues live in the IPC namespace: each queue test runs in a private one (the test
//! re-executed under `unshare --user --ipc --map-current-user`), so no test touches the
//! system's `/dev/mqueue`; without such a namespace the test says so and passes. `mq_open` is
//! variadic: its fixed-arity target `liella_mq_open4` is tested here and the C entry point
//! through real varargs in `conformance/c/s25_mqueue.c`. Shared-memory objects are made in a
//! scratch directory under `target/` that `selftest::shm_dir` substitutes for `/dev/shm/`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/scratch.rs"]
mod scratch;

use core::ptr::{null, null_mut};
use std::ffi::{CStr, CString};

use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s25_ipc as s;
use liella_kernel::uapi::{MqAttr, Sembuf, SigEvent, Timespec, SIGEV_NONE};

const ENOENT: i32 = 2;
const EAGAIN: i32 = 11;
const EFAULT: i32 = 14;
const EEXIST: i32 = 17;
const EINVAL: i32 = 22;
const ENOMSG: i32 = 42;
const E2BIG: i32 = 7;
const IPC_PRIVATE: i32 = 0;
const IPC_CREAT: i32 = 0o1000;
const IPC_EXCL: i32 = 0o2000;
const IPC_NOWAIT: i32 = 0o4000;
const IPC_RMID: i32 = 0;
const IPC_SET: i32 = 1;
const IPC_STAT: i32 = 2;
const IPC_INFO: i32 = 3;
const GETVAL: i32 = 12;
const SETVAL: i32 = 16;
const GETALL: i32 = 13;
const SETALL: i32 = 17;
const SEM_INFO: i32 = 19;
const MSG_INFO: i32 = 12;
const SHM_INFO: i32 = 14;

/// -1 with `errno == e`.
fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// A message: `long mtype` then the text.
#[repr(C)]
struct Msg<const N: usize> {
    mtype: i64,
    text: [u8; N],
}

#[test]
fn sym_ftok() {
    let p = scratch::Scratch::path("s25-ftok");
    std::fs::write(&p, b"").unwrap();
    let c = p.c();
    let m = std::fs::metadata(&p).unwrap();
    let (ino, dev) = (std::os::unix::fs::MetadataExt::ino(&m), std::os::unix::fs::MetadataExt::dev(&m));
    let want = ((ino & 0xffff) | ((dev & 0xff) << 16) | (0x41 << 24)) as u32 as i32;
    assert_eq!(s::ftok(c.as_ptr(), 0x41), want, "glibc's formula");
    assert_eq!(s::ftok(c.as_ptr(), 0x141), want, "only the low 8 bits of id");
    // Every project id 0..=255 gives the spec's key, including the negative ones (id >= 128).
    for id in 0..256i32 {
        let want = ((ino & 0xffff) | ((dev & 0xff) << 16) | ((id as u64) << 24)) as u32 as i32;
        errno::set_raw(0);
        assert_eq!(s::ftok(c.as_ptr(), id), want, "ftok id {id}");
        assert_eq!(errno::get(), 0, "a valid key leaves errno alone (id {id})");
        if id >= 128 {
            assert!(want < 0, "id {id} gives a negative key");
        }
    }
    assert_eq!(s::ftok(c.as_ptr(), -1), s::ftok(c.as_ptr(), 255), "id -1 is the low 8 bits, 255");
    std::fs::remove_file(&p).unwrap();
    errno::set_raw(0);
    assert!(fails(s::ftok(c.as_ptr(), 1) as isize, ENOENT));
}

// ------------------------------------------------------------------------------ POSIX queues

const O_RDONLY: i32 = 0;
const O_WRONLY: i32 = 1;
const O_RDWR: i32 = 2;
const O_CREAT: i32 = 0o100;
const O_NONBLOCK: i32 = 0o4000;
const EBADF: i32 = 9;
const EBUSY: i32 = 16;
const EMSGSIZE: i32 = 90;
const ETIMEDOUT: i32 = 110;
const IPCNS: &str = "LIELLA_S25_SYMBOLS_IPCNS";

/// Run `body` in a private IPC namespace (see the module documentation).
fn in_ipc_ns(test: &str, body: impl FnOnce()) {
    if std::env::var(IPCNS).as_deref() == Ok(test) {
        body();
        return;
    }
    let ns = ["--user", "--ipc", "--map-current-user"];
    if !std::process::Command::new("unshare").args(ns).arg("true").status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no private IPC namespace (unshare --user --ipc --map-current-user)");
        return;
    }
    let o = std::process::Command::new("unshare").args(ns).arg(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(IPCNS, test).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("1 passed"), "{test} in its namespace: {:?}\n{out}{}", o.status, String::from_utf8_lossy(&o.stderr));
}

fn mq(name: &CStr, flags: i32, attr: Option<&MqAttr>) -> i32 {
    s::liella_mq_open4(name.as_ptr(), flags, 0o600, attr.map_or(null(), |a| a as *const MqAttr))
}

/// A fresh queue of `n` messages of 8 bytes.
fn queue(name: &CStr, n: i64) -> i32 {
    let q = mq(name, O_RDWR | O_CREAT, Some(&MqAttr::new(n, 8)));
    assert!(q >= 0, "{name:?}: errno {}", errno::get());
    q
}

fn attr_of(q: i32) -> MqAttr {
    let mut a = MqAttr::default();
    assert_eq!(s::mq_getattr(q, Some(&mut a)), 0);
    a
}

#[test]
fn sym_mq_open() {
    in_ipc_ns("sym_mq_open", || {
        let q = mq(c"/o", O_RDWR | O_CREAT, Some(&MqAttr::new(3, 16)));
        assert!(q >= 0, "the descriptor");
        let a = attr_of(q);
        assert_eq!((a.maxmsg, a.msgsize), (3, 16), "the attributes reach the kernel");
        assert_eq!(io::liella_fcntl3(q, 1, 0), 1, "close-on-exec always");
        // Without O_CREAT the attribute argument is not read (here: no valid address).
        let again = s::liella_mq_open4(c"/o".as_ptr(), O_RDONLY, 0, core::ptr::with_exposed_provenance(8));
        assert!(again >= 0, "the optional arguments are ignored without O_CREAT");
        errno::set_raw(0);
        assert!(fails(mq(c"o", O_RDWR, None) as isize, EINVAL), "-1 with errno");
        let d = mq(c"/d", O_RDWR | O_CREAT, None);
        assert_eq!(attr_of(d).maxmsg, 10, "NULL attributes: the defaults");
        for q in [q, again, d] {
            s::mq_close(q);
        }
        s::mq_unlink(c"/o".as_ptr());
        s::mq_unlink(c"/d".as_ptr());
    });
    // The variadic C entry point.
    conformance(&["s25_mqueue"]);
}

#[test]
fn sym___mq_open_2() {
    in_ipc_ns("sym___mq_open_2", || {
        let q = queue(c"/o2", 2);
        let o = s::__mq_open_2(c"/o2".as_ptr(), O_RDWR);
        assert!(o >= 0, "without O_CREAT: mq_open");
        errno::set_raw(0);
        assert!(fails(s::__mq_open_2(c"/o2c".as_ptr(), O_RDWR | O_CREAT) as isize, EINVAL), "O_CREAT without mode and attributes");
        assert!(fails(s::__mq_open_2(c"/o2c".as_ptr(), O_RDWR) as isize, ENOENT), "nothing was created");
        s::mq_close(q);
        s::mq_close(o);
        s::mq_unlink(c"/o2".as_ptr());
    });
}

#[test]
fn sym_mq_close() {
    in_ipc_ns("sym_mq_close", || {
        let q = queue(c"/c", 1);
        assert_eq!(s::mq_close(q), 0);
        errno::set_raw(0);
        assert!(fails(s::mq_close(q) as isize, EBADF), "closed");
        assert!(fails(s::mq_close(-1) as isize, EBADF));
        s::mq_unlink(c"/c".as_ptr());
    });
}

#[test]
fn sym_mq_unlink() {
    in_ipc_ns("sym_mq_unlink", || {
        let q = queue(c"/u", 1);
        assert_eq!(s::mq_unlink(c"/u".as_ptr()), 0);
        errno::set_raw(0);
        assert!(fails(s::mq_unlink(c"/u".as_ptr()) as isize, ENOENT));
        assert!(fails(s::mq_unlink(c"u".as_ptr()) as isize, EINVAL), "no leading slash");
        assert!(fails(s::mq_unlink(null()) as isize, EFAULT), "NULL");
        s::mq_close(q);
    });
}

#[test]
fn sym_mq_send() {
    in_ipc_ns("sym_mq_send", || {
        let q = queue(c"/s", 1);
        assert_eq!(s::mq_send(q, c"abc".as_ptr(), 3, 7), 0);
        let mut b = [0u8; 8];
        let mut p = 0;
        assert_eq!(s::mq_receive(q, b.as_mut_ptr().cast(), 8, Some(&mut p)), 3);
        assert_eq!((&b[..3], p), (&b"abc"[..], 7), "the bytes and the priority");
        errno::set_raw(0);
        assert!(fails(s::mq_send(q, c"123456789".as_ptr(), 9, 0) as isize, EMSGSIZE));
        assert!(fails(s::mq_send(q, null(), 1, 0) as isize, EFAULT), "a NULL message");
        assert_eq!(s::mq_send(q, null(), 0, 0), 0, "an empty message");
        let r = mq(c"/s", O_RDONLY, None);
        assert!(fails(s::mq_send(r, c"x".as_ptr(), 1, 0) as isize, EBADF));
        s::mq_close(q);
        s::mq_close(r);
        s::mq_unlink(c"/s".as_ptr());
    });
}

#[test]
fn sym_mq_receive() {
    in_ipc_ns("sym_mq_receive", || {
        let q = mq(c"/r", O_RDWR | O_CREAT | O_NONBLOCK, Some(&MqAttr::new(2, 8)));
        s::mq_send(q, c"xy".as_ptr(), 2, 3);
        let mut b = [0u8; 8];
        assert_eq!(s::mq_receive(q, b.as_mut_ptr().cast(), 8, None), 2, "the length; a NULL priority");
        errno::set_raw(0);
        assert!(fails(s::mq_receive(q, b.as_mut_ptr().cast(), 8, None), EAGAIN), "empty, non-blocking");
        assert!(fails(s::mq_receive(q, b.as_mut_ptr().cast(), 7, None), EMSGSIZE), "below mq_msgsize");
        let w = mq(c"/r", O_WRONLY, None);
        assert!(fails(s::mq_receive(w, b.as_mut_ptr().cast(), 8, None), EBADF));
        s::mq_close(q);
        s::mq_close(w);
        s::mq_unlink(c"/r".as_ptr());
    });
}

#[test]
fn sym_mq_timedsend() {
    in_ipc_ns("sym_mq_timedsend", || {
        let q = queue(c"/ts", 1);
        assert_eq!(s::mq_timedsend(q, c"a".as_ptr(), 1, 0, None), 0, "a NULL timeout with room");
        let past = Timespec { sec: 0, nsec: 0 };
        errno::set_raw(0);
        assert!(fails(s::mq_timedsend(q, c"b".as_ptr(), 1, 0, Some(&past)) as isize, ETIMEDOUT), "full");
        assert!(fails(s::mq_timedsend(q, c"b".as_ptr(), 1, 0, Some(&Timespec { sec: 0, nsec: -1 })) as isize, EINVAL));
        s::mq_close(q);
        s::mq_unlink(c"/ts".as_ptr());
    });
}

#[test]
fn sym_mq_timedreceive() {
    in_ipc_ns("sym_mq_timedreceive", || {
        let q = queue(c"/tr", 1);
        let mut b = [0u8; 8];
        let past = Timespec { sec: 0, nsec: 0 };
        errno::set_raw(0);
        assert!(fails(s::mq_timedreceive(q, b.as_mut_ptr().cast(), 8, None, Some(&past)), ETIMEDOUT), "empty");
        s::mq_send(q, c"m".as_ptr(), 1, 4);
        let mut p = 0;
        assert_eq!(s::mq_timedreceive(q, b.as_mut_ptr().cast(), 8, Some(&mut p), Some(&past)), 1, "a message waiting: no wait");
        assert_eq!(p, 4);
        assert!(fails(s::mq_timedreceive(q, b.as_mut_ptr().cast(), 8, None, Some(&Timespec { sec: -1, nsec: 0 })), EINVAL));
        s::mq_close(q);
        s::mq_unlink(c"/tr".as_ptr());
    });
}

#[test]
fn sym_mq_notify() {
    in_ipc_ns("sym_mq_notify", || {
        let q = queue(c"/n", 1);
        let none = SigEvent::new(SIGEV_NONE, 0, 0, 0);
        assert_eq!(s::mq_notify(q, Some(&none)), 0);
        errno::set_raw(0);
        assert!(fails(s::mq_notify(q, Some(&none)) as isize, EBUSY));
        assert_eq!(s::mq_notify(q, None), 0, "NULL removes");
        assert_eq!(s::mq_notify(q, Some(&none)), 0);
        assert_eq!(s::mq_notify(q, None), 0);
        assert!(fails(s::mq_notify(q, Some(&SigEvent::new(99, 0, 0, 0))) as isize, EINVAL), "an unknown notification");
        assert!(fails(s::mq_notify(q, Some(&SigEvent::thread(0, 0, 0))) as isize, EINVAL), "SIGEV_THREAD without a function");
        assert!(fails(s::mq_notify(-1, Some(&none)) as isize, EBADF));
        s::mq_close(q);
        s::mq_unlink(c"/n".as_ptr());
    });
}

#[test]
fn sym_mq_getattr() {
    in_ipc_ns("sym_mq_getattr", || {
        let q = mq(c"/g", O_RDWR | O_CREAT | O_NONBLOCK, Some(&MqAttr::new(5, 8)));
        s::mq_send(q, c"m".as_ptr(), 1, 0);
        let a = attr_of(q);
        assert_eq!((a.flags, a.maxmsg, a.msgsize, a.curmsgs), (i64::from(O_NONBLOCK), 5, 8, 1));
        assert_eq!(s::mq_getattr(q, None), 0, "NULL: nothing stored");
        errno::set_raw(0);
        let mut a = MqAttr::default();
        assert!(fails(s::mq_getattr(-1, Some(&mut a)) as isize, EBADF));
        s::mq_close(q);
        s::mq_unlink(c"/g".as_ptr());
    });
}

#[test]
fn sym_mq_setattr() {
    in_ipc_ns("sym_mq_setattr", || {
        let q = queue(c"/sa", 2);
        let mut new = MqAttr::new(9, 9);
        new.flags = i64::from(O_NONBLOCK);
        let mut old = MqAttr::default();
        assert_eq!(s::mq_setattr(q, Some(&new), Some(&mut old)), 0);
        assert_eq!((old.flags, old.maxmsg), (0, 2), "the old attributes");
        assert_eq!(attr_of(q).flags, i64::from(O_NONBLOCK));
        assert_eq!(s::mq_setattr(q, None, None), 0);
        new.flags = 0o2000;
        errno::set_raw(0);
        assert!(fails(s::mq_setattr(q, Some(&new), None) as isize, EINVAL), "a flag other than O_NONBLOCK");
        s::mq_close(q);
        s::mq_unlink(c"/sa".as_ptr());
    });
}
#[test]
fn sym_msgctl() {
    let q = s::msgget(IPC_PRIVATE, 0o600);
    let m = Msg { mtype: 1, text: *b"ab" };
    s::msgsnd(q, (&raw const m).cast(), 2, 0);
    let mut ds = liella_kernel::uapi::MsqidDs::default();
    assert_eq!(s::msgctl(q, IPC_STAT, (&raw mut ds).cast()), 0);
    assert_eq!(ds.qnum(), 1, "msg_qnum");
    assert_eq!(s::msgctl(q, IPC_SET, (&raw mut ds).cast()), 0);
    let mut info = [0u8; 32];
    assert!(s::msgctl(q, IPC_INFO, info.as_mut_ptr().cast()) >= 0, "struct msginfo (32 bytes): only those are written");
    assert!(s::msgctl(q, MSG_INFO | 0x100, info.as_mut_ptr().cast()) >= 0, "IPC_64 in the command is accepted");
    errno::set_raw(0);
    assert!(fails(s::msgctl(q, IPC_STAT, null_mut()) as isize, EFAULT));
    assert!(fails(s::msgctl(q, 77, (&raw mut ds).cast()) as isize, EINVAL));
    assert_eq!(s::msgctl(q, IPC_RMID, null_mut()), 0, "IPC_RMID ignores its argument");
    assert!(fails(s::msgctl(q, IPC_STAT, (&raw mut ds).cast()) as isize, EINVAL), "removed");
}

#[test]
fn sym_msgget() {
    let q = s::msgget(IPC_PRIVATE, 0o600);
    assert!(q >= 0);
    let r = s::msgget(IPC_PRIVATE, 0o600);
    assert_ne!(q, r, "IPC_PRIVATE: always a new queue");
    errno::set_raw(0);
    assert!(fails(s::msgget(0x5a5a_0001, 0) as isize, ENOENT) || s::msgget(0x5a5a_0001, 0) >= 0, "a missing key without IPC_CREAT");
    for id in [q, r] {
        assert_eq!(s::msgctl(id, IPC_RMID, null_mut()), 0);
    }
}

#[test]
fn sym_msgrcv() {
    let q = s::msgget(IPC_PRIVATE, 0o600);
    let a = Msg { mtype: 1, text: *b"one" };
    let b = Msg { mtype: 2, text: *b"two" };
    s::msgsnd(q, (&raw const a).cast(), 3, 0);
    s::msgsnd(q, (&raw const b).cast(), 3, 0);
    let mut out = Msg { mtype: 0, text: [0u8; 8] };
    assert_eq!(s::msgrcv(q, (&raw mut out).cast(), 8, 2, 0), 3, "by type");
    assert_eq!((out.mtype, &out.text[..3]), (2, &b"two"[..]));
    let mut small = Msg { mtype: 0, text: [0u8; 1] };
    errno::set_raw(0);
    assert!(fails(s::msgrcv(q, (&raw mut small).cast(), 1, 0, 0), E2BIG), "too long without MSG_NOERROR");
    assert_eq!(s::msgrcv(q, (&raw mut out).cast(), 8, 0, 0), 3);
    assert!(fails(s::msgrcv(q, (&raw mut out).cast(), 8, 0, IPC_NOWAIT), ENOMSG), "empty, IPC_NOWAIT");
    s::msgctl(q, IPC_RMID, null_mut());
}

#[test]
fn sym_msgsnd() {
    let q = s::msgget(IPC_PRIVATE, 0o600);
    let m = Msg { mtype: 3, text: *b"hello" };
    assert_eq!(s::msgsnd(q, (&raw const m).cast(), 5, 0), 0);
    errno::set_raw(0);
    let bad = Msg { mtype: 0, text: *b"x" };
    assert!(fails(s::msgsnd(q, (&raw const bad).cast(), 1, 0) as isize, EINVAL), "mtype must be positive");
    assert!(fails(s::msgsnd(q, null(), 1, 0) as isize, EFAULT));
    assert!(fails(s::msgsnd(q, (&raw const m).cast(), usize::MAX, 0) as isize, EINVAL), "a size that cannot be");
    s::msgctl(q, IPC_RMID, null_mut());
}

#[test]
fn sym___pipe() {
    let mut fds = [-1; 2];
    assert_eq!(s::__pipe(Some(&mut fds)), 0);
    assert_eq!(io::write(fds[1], b"through".as_ptr().cast(), 7), 7, "[1] is the write end");
    let mut b = [0u8; 7];
    assert_eq!(io::read(fds[0], b.as_mut_ptr().cast(), 7), 7);
    assert_eq!(&b, b"through");
    assert_eq!(io::liella_fcntl3(fds[0], 1, 0), 0, "no FD_CLOEXEC");
    io::close(fds[0]);
    io::close(fds[1]);
    errno::set_raw(0);
    assert!(fails(s::__pipe(None) as isize, EFAULT));
}

#[test]
fn sym_pipe() {
    let mut fds = [-1; 2];
    assert_eq!(s::pipe(Some(&mut fds)), 0);
    assert_eq!(io::write(fds[1], b"through".as_ptr().cast(), 7), 7, "[1] is the write end");
    let mut b = [0u8; 7];
    assert_eq!(io::read(fds[0], b.as_mut_ptr().cast(), 7), 7);
    assert_eq!(&b, b"through");
    assert_eq!(io::liella_fcntl3(fds[0], 1, 0), 0, "no FD_CLOEXEC");
    io::close(fds[0]);
    io::close(fds[1]);
    errno::set_raw(0);
    assert!(fails(s::pipe(None) as isize, EFAULT));
}

#[test]
fn sym_pipe2() {
    let mut fds = [-1; 2];
    assert_eq!(s::pipe2(Some(&mut fds), 0o2000000 | 0o4000), 0, "O_CLOEXEC | O_NONBLOCK");
    assert_eq!(io::liella_fcntl3(fds[0], 1, 0), 1);
    let mut b = [0u8; 1];
    errno::set_raw(0);
    assert!(fails(io::read(fds[0], b.as_mut_ptr().cast(), 1), EAGAIN));
    io::close(fds[0]);
    io::close(fds[1]);
    assert!(fails(s::pipe2(Some(&mut fds), 0o1) as isize, EINVAL));
}

#[test]
fn sym_semctl() {
    let id = s::semget(IPC_PRIVATE, 3, 0o600);
    assert_eq!(s::liella_semctl4(id, 1, SETVAL, 0xdead_beef_0000_0007), 0, "SETVAL: the int member (the upper half is not part of it)");
    assert_eq!(s::liella_semctl4(id, 1, GETVAL, 0), 7);
    let mut all = [5u16, 6, 7];
    assert_eq!(s::liella_semctl4(id, 0, SETALL, all.as_mut_ptr() as usize), 0);
    let mut got = [0u16; 3];
    assert_eq!(s::liella_semctl4(id, 0, GETALL, got.as_mut_ptr() as usize), 0, "one entry per semaphore");
    assert_eq!(got, [5, 6, 7]);
    let mut ds = liella_kernel::uapi::SemidDs::default();
    assert_eq!(s::liella_semctl4(id, 0, IPC_STAT, (&raw mut ds) as usize), 0);
    assert_eq!(ds.nsems(), 3);
    let mut info = [0u8; 40];
    assert!(s::liella_semctl4(id, 0, SEM_INFO, info.as_mut_ptr() as usize) >= 0, "struct seminfo (40 bytes)");
    errno::set_raw(0);
    assert!(fails(s::liella_semctl4(id, 0, 99, 0) as isize, EINVAL));
    assert!(fails(s::liella_semctl4(id, 0, IPC_STAT, 0) as isize, EFAULT));
    assert_eq!(s::liella_semctl4(id, 0, IPC_RMID, 0), 0);
    conformance(&["s25_semctl"]);
}

#[test]
fn sym_semget() {
    let id = s::semget(IPC_PRIVATE, 3, 0o600);
    assert!(id >= 0);
    errno::set_raw(0);
    assert!(fails(s::semget(IPC_PRIVATE, -1, 0o600) as isize, EINVAL));
    let key = 0x5a5a_0000 | (std::process::id() as i32 & 0xffff);
    let k = s::semget(key, 1, IPC_CREAT | 0o600);
    assert!(k >= 0);
    assert!(fails(s::semget(key, 1, IPC_CREAT | IPC_EXCL | 0o600) as isize, EEXIST));
    assert_eq!(s::liella_semctl4(k, 0, IPC_RMID, 0), 0);
    assert_eq!(s::liella_semctl4(id, 0, IPC_RMID, 0), 0);
}

#[test]
fn sym_semop() {
    let id = s::semget(IPC_PRIVATE, 2, 0o600);
    let mut up = [Sembuf { num: 0, op: 2, flg: 0 }, Sembuf { num: 1, op: 1, flg: 0 }];
    assert_eq!(s::semop(id, up.as_mut_ptr().cast(), 2), 0, "applied atomically");
    assert_eq!(s::liella_semctl4(id, 0, GETVAL, 0), 2);
    let mut down = [Sembuf { num: 1, op: -2, flg: IPC_NOWAIT as i16 }];
    errno::set_raw(0);
    assert!(fails(s::semop(id, down.as_mut_ptr().cast(), 1) as isize, EAGAIN), "would block");
    assert!(fails(s::semop(id, up.as_mut_ptr().cast(), 0) as isize, EINVAL), "no operations");
    s::liella_semctl4(id, 0, IPC_RMID, 0);
}

#[test]
fn sym_semtimedop() {
    let id = s::semget(IPC_PRIVATE, 1, 0o600);
    let mut down = [Sembuf { num: 0, op: -1, flg: 0 }];
    let ts = Timespec { sec: 0, nsec: 20_000_000 };
    errno::set_raw(0);
    assert!(fails(s::semtimedop(id, down.as_mut_ptr().cast(), 1, Some(&ts)) as isize, EAGAIN), "the timeout");
    let bad = Timespec { sec: 0, nsec: 2_000_000_000 };
    assert!(fails(s::semtimedop(id, down.as_mut_ptr().cast(), 1, Some(&bad)) as isize, EINVAL));
    s::liella_semctl4(id, 0, IPC_RMID, 0);
}

// ------------------------------------------------------------------------------ POSIX shared memory

/// `body` with `shm_open`'s directory a fresh scratch directory under `target/` (its path with
/// the final `/`), one test at a time (the directory is process-wide); removed afterwards.
fn with_shm_dir(tag: &str, body: impl FnOnce(&std::path::Path)) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/test-scratch/s25sym-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let mut d = dir.as_os_str().as_encoded_bytes().to_vec();
    d.push(b'/');
    liella_boundary::selftest::shm_dir(Some(&d));
    body(&dir);
    liella_boundary::selftest::shm_dir(None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sym_shm_open() {
    with_shm_dir("open", |dir| {
        let fd = s::shm_open(c"/obj".as_ptr(), O_RDWR | O_CREAT, 0o640);
        assert!(fd >= 0, "a descriptor");
        let m = std::fs::metadata(dir.join("obj")).unwrap();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o7777, 0o640 & !umask(), "the mode, less the umask");
        assert_eq!(io::liella_fcntl3(fd, 1, 0), 1, "close-on-exec");
        assert_eq!(io::liella_fcntl3(fd, 3, 0) & 3, O_RDWR, "the access mode");
        let nb = s::shm_open(c"obj".as_ptr(), O_RDONLY | O_NONBLOCK, 0);
        assert!(nb >= 0, "no slash is the same name");
        assert_eq!(io::liella_fcntl3(nb, 3, 0) & O_NONBLOCK, O_NONBLOCK, "O_NONBLOCK kept when asked");
        errno::set_raw(0);
        assert!(fails(s::shm_open(c"/obj".as_ptr(), O_RDWR | O_CREAT | 0o200, 0o600) as isize, EEXIST), "O_EXCL");
        assert!(fails(s::shm_open(c"/a/b".as_ptr(), O_RDWR | O_CREAT, 0o600) as isize, EINVAL), "a further slash");
        assert!(fails(s::shm_open(c"/missing".as_ptr(), O_RDWR, 0) as isize, ENOENT));
        assert!(fails(s::shm_open(null(), O_RDWR, 0) as isize, EFAULT), "NULL");
        let long = CString::new(format!("/{}", "s".repeat(256))).unwrap();
        assert!(fails(s::shm_open(long.as_ptr(), O_RDWR | O_CREAT, 0o600) as isize, 36), "ENAMETOOLONG");
        io::close(fd);
        io::close(nb);
    });
}

#[test]
fn sym_shm_unlink() {
    with_shm_dir("unlink", |dir| {
        let fd = s::shm_open(c"/u".as_ptr(), O_RDWR | O_CREAT, 0o600);
        assert_eq!(s::shm_unlink(c"/u".as_ptr()), 0);
        assert!(!dir.join("u").exists(), "the name is gone");
        errno::set_raw(0);
        assert!(fails(s::shm_unlink(c"/u".as_ptr()) as isize, ENOENT));
        assert!(fails(s::shm_unlink(c"/".as_ptr()) as isize, ENOENT), "no object has that name");
        assert!(fails(s::shm_unlink(c"/..".as_ptr()) as isize, ENOENT));
        assert!(fails(s::shm_unlink(null()) as isize, EFAULT), "NULL");
        io::close(fd);
    });
}

/// The process's umask, as `/proc/self/status` shows it (changing it would race with the
/// other tests).
fn umask() -> u32 {
    let st = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = st.lines().find(|l| l.starts_with("Umask:")).unwrap();
    u32::from_str_radix(line.trim_start_matches("Umask:").trim(), 8).unwrap()
}
#[test]
fn sym_shmat() {
    let id = s::shmget(IPC_PRIVATE, 4096, 0o600);
    let a = s::shmat(id, null(), 0);
    assert_ne!(a as isize, -1);
    let b = s::shmat(id, null(), 0o10000);
    assert_ne!(b as isize, -1, "SHM_RDONLY");
    liella_boundary::posix::s04_strings::memcpy(a, b"shared".as_ptr().cast(), 6);
    let mut v = [0u8; 6];
    liella_boundary::posix::s04_strings::memcpy(v.as_mut_ptr().cast(), b, 6);
    assert_eq!(&v, b"shared", "two attachments of one segment");
    s::shmdt(a);
    s::shmdt(b);
    errno::set_raw(0);
    assert!(fails(s::shmat(id, core::ptr::without_provenance(1), 0) as isize, EINVAL), "an unaligned address without SHM_RND");
    s::shmctl(id, IPC_RMID, null_mut());
}

#[test]
fn sym_shmctl() {
    let id = s::shmget(IPC_PRIVATE, 12288, 0o600);
    let mut ds = liella_kernel::uapi::ShmidDs::default();
    assert_eq!(s::shmctl(id, IPC_STAT, (&raw mut ds).cast()), 0);
    assert_eq!(ds.segsz(), 12288);
    let mut info = [0u8; 72];
    assert!(s::shmctl(id, IPC_INFO, info.as_mut_ptr().cast()) >= 0, "struct shminfo (72 bytes)");
    let mut sinfo = [0u8; 48];
    assert!(s::shmctl(id, SHM_INFO, sinfo.as_mut_ptr().cast()) >= 0, "struct shm_info (48 bytes)");
    errno::set_raw(0);
    assert!(fails(s::shmctl(id, 99, null_mut()) as isize, EINVAL));
    assert_eq!(s::shmctl(id, IPC_RMID, null_mut()), 0);
}

#[test]
fn sym_shmdt() {
    let id = s::shmget(IPC_PRIVATE, 4096, 0o600);
    let a = s::shmat(id, null(), 0);
    let mut ds = liella_kernel::uapi::ShmidDs::default();
    s::shmctl(id, IPC_STAT, (&raw mut ds).cast());
    assert!(ds.nattch() >= 1, "ours is counted (a concurrently forked child may add its inherited one)");
    assert_eq!(s::shmdt(a), 0);
    // A child that another test forks while the segment is attached inherits the attachment
    // (fork copies them) until it exits: wait up to 2 s for those to go, then require none left.
    for _ in 0..200 {
        s::shmctl(id, IPC_STAT, (&raw mut ds).cast());
        if ds.nattch() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(ds.nattch(), 0);
    // Not an attachment: an address on this thread's stack. (Detaching `a` again would race:
    // a test running concurrently may have been given the same address for its own segment.)
    let local = 0u64;
    errno::set_raw(0);
    assert!(fails(s::shmdt((&raw const local).cast()) as isize, EINVAL), "not an attachment");
    s::shmctl(id, IPC_RMID, null_mut());
}

#[test]
fn sym_shmget() {
    let id = s::shmget(IPC_PRIVATE, 8192, 0o600);
    assert!(id >= 0);
    errno::set_raw(0);
    assert!(fails(s::shmget(IPC_PRIVATE, 0, 0o600) as isize, EINVAL), "below SHMMIN");
    assert_eq!(s::shmctl(id, IPC_RMID, null_mut()), 0);
}

