// Specified from spec §25 (phase 5: §25.1–25.2; phase 13: §25.3–25.4).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §25 (pipes and System V IPC through the exports; the `*ctl`
//! argument shapes through `liella_legacy::ipc_compat`; POSIX message queues and shared
//! memory through the exports).
//!
//! POSIX queues live in the IPC namespace, so their clauses run in a private one: the test
//! re-executed under `unshare --user --ipc --map-current-user` (unprivileged, with the kernel's
//! default limits: `msg_max` 10, `msgsize_max` 8192), whose queues vanish with it; a test never
//! touches the system's `/dev/mqueue`. Without such a namespace the test says so and passes.
//! Shared-memory objects are made in a scratch directory under `target/`, which
//! `selftest::shm_dir` substitutes for `/dev/shm/` (accepted only in tests). The variadic
//! `mq_open` and the cancellation clause run as C (`conformance/c/s25_mqueue.c`, in a private
//! IPC namespace too).
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/scratch.rs"]
mod scratch;

use core::ptr::{null, null_mut};
use std::process::Command;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use liella::error::Errno;
use liella_abi::ctypes::c_int;
use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s25_ipc as x;
use liella_boundary::selftest::ChildEnd;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MqAttr, SigEvent, SigInfo, SigSet, Sembuf, SemidDs, ShmidDs, Timespec, SIGEV_NONE, SIGEV_SIGNAL, SIGUSR1, SIG_BLOCK};
use liella_legacy::ipc_compat::{self, CtlShape};

const IPC_PRIVATE: c_int = 0;
const IPC_NOWAIT: c_int = 0o4000;
const IPC_RMID: c_int = 0;
const IPC_SET: c_int = 1;
const IPC_STAT: c_int = 2;
const IPC_INFO: c_int = 3;
const GETVAL: c_int = 12;
const SETVAL: c_int = 16;
const MSG_NOERROR: c_int = 0o10000;
const MSG_EXCEPT: c_int = 0o20000;
const SEM_UNDO: i16 = 0x1000;

fn err(r: isize, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

fn pipe() -> [c_int; 2] {
    let mut fds = [-1; 2];
    assert_eq!(x::pipe(Some(&mut fds)), 0);
    fds
}

#[repr(C)]
struct Msg<const N: usize> {
    mtype: i64,
    text: [u8; N],
}

fn send(q: c_int, mtype: i64, text: &[u8; 1]) {
    let m = Msg { mtype, text: *text };
    assert_eq!(x::msgsnd(q, (&raw const m).cast(), 1, 0), 0);
}

fn child(name: &str, var: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env(var, "1")
        .output()
        .unwrap()
}

#[test]
fn s25_1_c_0() {
    let [r, w] = pipe();
    assert_eq!(io::write(w, b"one-way".as_ptr().cast(), 7), 7);
    let mut b = [0u8; 7];
    assert_eq!(io::read(r, b.as_mut_ptr().cast(), 7), 7, "fds[0] reads what fds[1] wrote");
    errno::set_raw(0);
    assert!(err(io::write(r, b"x".as_ptr().cast(), 1), Errno::EBADF), "unidirectional");
    io::close(r);
    io::close(w);
    let mut p = [-1; 2];
    assert_eq!(x::pipe2(Some(&mut p), 0o2000000 | 0o4000 | 0o40000), 0, "O_CLOEXEC | O_NONBLOCK | O_DIRECT");
    // Packet mode: each write is one read.
    io::write(p[1], b"ab".as_ptr().cast(), 2);
    io::write(p[1], b"cde".as_ptr().cast(), 3);
    let mut b = [0u8; 16];
    assert_eq!(io::read(p[0], b.as_mut_ptr().cast(), 16), 2, "O_DIRECT: packets");
    assert_eq!(io::read(p[0], b.as_mut_ptr().cast(), 16), 3);
    io::close(p[0]);
    io::close(p[1]);
}

#[test]
fn s25_1_g_1() {
    let [r, w] = pipe();
    let writers: Vec<_> = (0..3u8)
        .map(|k| {
            std::thread::spawn(move || {
                let block = [b'a' + k; 4096];
                for _ in 0..6 {
                    assert_eq!(io::write(w, block.as_ptr().cast(), 4096), 4096);
                }
            })
        })
        .collect();
    let mut all = Vec::new();
    let mut b = [0u8; 4096];
    while all.len() < 3 * 6 * 4096 {
        let n = io::read(r, b.as_mut_ptr().cast(), 4096);
        all.extend_from_slice(&b[..n as usize]);
    }
    for h in writers {
        h.join().unwrap();
    }
    assert!(all.chunks(4096).all(|c| c.iter().all(|&v| v == c[0])), "PIPE_BUF writes never interleave");
    io::close(r);
    io::close(w);
}

#[test]
fn s25_1_g_2() {
    let [r, w] = pipe();
    io::close(w);
    let mut b = [0u8; 1];
    assert_eq!(io::read(r, b.as_mut_ptr().cast(), 1), 0, "end of file, not blocking");
    io::close(r);
}

#[test]
fn s25_1_g_3() {
    if std::env::var("LIELLA_C25_PIPE").is_ok() {
        // Rust programs start with SIGPIPE ignored; a C program has the default action.
        let _ = CSys.sigaction_set(13, liella_kernel::sys::Disposition::Default, 0, liella_kernel::uapi::SigSet::empty());
        let [r, w] = pipe();
        io::close(r);
        io::write(w, b"x".as_ptr().cast(), 1);
        CSys.exit_group(0); // not reached: SIGPIPE's default action ends the process
    }
    if std::env::var("LIELLA_C25_EPIPE").is_ok() {
        let _ = CSys.sigaction_set(13, liella_kernel::sys::Disposition::Ignore, 0, liella_kernel::uapi::SigSet::empty());
        let [r, w] = pipe();
        io::close(r);
        errno::set_raw(0);
        CSys.exit_group(if err(io::write(w, b"x".as_ptr().cast(), 1), Errno::EPIPE) { 0 } else { 1 });
    }
    let o = child("s25_1_g_3", "LIELLA_C25_PIPE");
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(13), "SIGPIPE");
    assert_eq!(child("s25_1_g_3", "LIELLA_C25_EPIPE").status.code(), Some(0), "and EPIPE when it is ignored");
}

#[test]
fn s25_1_g_4() {
    let [r, w] = pipe();
    assert_eq!(io::liella_fcntl3(w, 1031, 1 << 16), 1 << 16, "F_SETPIPE_SZ");
    io::liella_fcntl3(w, 4, 0o4000);
    let big = vec![0u8; 1 << 20];
    assert_eq!(io::write(w, big.as_ptr().cast(), big.len()), 1 << 16, "the capacity is what was set");
    io::close(r);
    io::close(w);
}

#[test]
fn s25_2_c_1() {
    let p = scratch::Scratch::path("c25-ftok");
    std::fs::write(&p, b"").unwrap();
    let c = p.c();
    let a = x::ftok(c.as_ptr(), 1);
    let b = x::ftok(c.as_ptr(), 2);
    assert_eq!(a ^ b, 3 << 24, "the id is the top 8 bits");
    assert_eq!(x::ftok(c.as_ptr(), 1 | 0x100), a, "only 8 bits of the id: collisions are possible");
    let m = std::fs::metadata(&p).unwrap();
    assert_eq!(a as u32 & 0xffff, (std::os::unix::fs::MetadataExt::ino(&m) & 0xffff) as u32, "the inode's low 16 bits");
}

#[test]
fn s25_2_c_2() {
    let q = x::msgget(IPC_PRIVATE, 0o600);
    for (ty, c) in [(5, b"a"), (2, b"b"), (9, b"c"), (2, b"d")] {
        send(q, ty, c);
    }
    let mut m = Msg { mtype: 0, text: [0u8; 4] };
    assert_eq!(x::msgrcv(q, (&raw mut m).cast(), 4, 2, 0), 1);
    assert_eq!((m.mtype, m.text[0]), (2, b'b'), "first of type 2");
    assert_eq!(x::msgrcv(q, (&raw mut m).cast(), 4, -6, 0), 1);
    assert_eq!((m.mtype, m.text[0]), (2, b'd'), "the lowest type <= 6");
    assert_eq!(x::msgrcv(q, (&raw mut m).cast(), 4, 5, MSG_EXCEPT), 1);
    assert_eq!(m.mtype, 9, "MSG_EXCEPT: first not of type 5");
    assert_eq!(x::msgrcv(q, (&raw mut m).cast(), 4, 0, 0), 1);
    assert_eq!(m.mtype, 5, "0: any");
    let long = Msg { mtype: 1, text: *b"truncated" };
    x::msgsnd(q, (&raw const long).cast(), 9, 0);
    let mut small = Msg { mtype: 0, text: [0u8; 3] };
    assert_eq!(x::msgrcv(q, (&raw mut small).cast(), 3, 0, MSG_NOERROR), 3, "MSG_NOERROR truncates");
    assert_eq!(&small.text, b"tru");
    errno::set_raw(0);
    assert!(err(x::msgrcv(q, (&raw mut m).cast(), 4, 0, IPC_NOWAIT), Errno::ENOMSG), "IPC_NOWAIT");
    send(q, 3, b"z");
    assert_eq!(x::msgrcv(q, (&raw mut m).cast(), 4, 0, IPC_NOWAIT | 0o40000), 1, "MSG_COPY: a copy by index");
    assert_eq!(x::msgrcv(q, (&raw mut m).cast(), 4, 0, 0), 1, "the message is still there");
    x::msgctl(q, IPC_RMID, null_mut());
}

#[test]
fn s25_2_c_3() {
    // SEM_UNDO: a child's adjustment is reversed when it exits. The child (this test re-run) does only this: it makes no
    // semaphore set of its own, which nothing would remove (SEMTIMEDOP-LOAD).
    if std::env::var("LIELLA_C25_UNDO").is_ok_and(|v| v.parse::<i32>().is_ok()) {
        let id: i32 = std::env::var("LIELLA_C25_UNDO").unwrap().parse().unwrap();
        let mut up = [Sembuf { num: 0, op: 3, flg: SEM_UNDO }];
        CSys.exit_group(x::semop(id, up.as_mut_ptr().cast(), 1));
    }
    let id = x::semget(IPC_PRIVATE, 2, 0o600);
    // Atomic: an array whose second operation cannot proceed changes nothing.
    let mut ops = [Sembuf { num: 0, op: 1, flg: 0 }, Sembuf { num: 1, op: -1, flg: IPC_NOWAIT as i16 }];
    errno::set_raw(0);
    assert!(err(x::semop(id, ops.as_mut_ptr().cast(), 2) as isize, Errno::EAGAIN));
    assert_eq!(x::liella_semctl4(id, 0, GETVAL, 0), 0, "all or nothing");
    let mut wait = [Sembuf { num: 1, op: -1, flg: 0 }];
    let ts = Timespec { sec: 0, nsec: 10_000_000 };
    // A wait that a signal or the kernel cuts short ends with EINTR (observed under load with no handler installed,
    // after 1-6 ms of a 10 ms timeout), so the timeout is awaited again; only the final result is the clause.
    let (mut r, mut e, mut tries) = (0isize, 0, 0);
    let t0 = Instant::now();
    while tries < 1000 {
        tries += 1;
        r = x::semtimedop(id, wait.as_mut_ptr().cast(), 1, Some(&ts)) as isize;
        e = errno::get();
        if !(r == -1 && e == Errno::EINTR.get()) {
            break;
        }
    }
    assert!(r == -1 && e == Errno::EAGAIN.get(), "semtimedop's timeout: r={r} errno={e} after {:?}, {tries} tries (id {id})", t0.elapsed());
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["s25_2_c_3", "--exact", "--test-threads=1"])
        .env("LIELLA_C25_UNDO", id.to_string())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(x::liella_semctl4(id, 0, GETVAL, 0), 0, "undone at exit");
    // The union semun's members by command: an int (SETVAL), a pointer (IPC_STAT).
    assert_eq!(ipc_compat::sem_shape(SETVAL), Some(CtlShape::Val));
    assert_eq!(ipc_compat::sem_shape(IPC_STAT), Some(CtlShape::Ds));
    assert_eq!(ipc_compat::sem_shape(17), Some(CtlShape::Values));
    x::liella_semctl4(id, 0, IPC_RMID, 0);
}

#[test]
fn s25_2_c_4() {
    let id = x::shmget(IPC_PRIVATE, 4096, 0o600);
    let a = x::shmat(id, null(), 0);
    assert_ne!(a as isize, -1, "a kernel-chosen address");
    let want = a as usize + (1 << 30);
    let b = x::shmat(id, (want + 123) as *const _, 0o20000);
    assert_eq!(b as usize, want, "SHM_RND rounds the requested address down to SHMLBA");
    let ro = x::shmat(id, null(), 0o10000);
    let r = x::shmat(id, b, 0o40000);
    assert_eq!(r, b, "SHM_REMAP replaces the mapping at the address");
    for p in [a, b, ro] {
        assert_eq!(x::shmdt(p), 0);
    }
    errno::set_raw(0);
    assert!(err(x::shmdt(a) as isize, Errno::EINVAL));
    x::shmctl(id, IPC_RMID, null_mut());
}

#[test]
fn s25_2_c_5() {
    let q = x::msgget(IPC_PRIVATE, 0o600);
    let mut ds = liella_kernel::uapi::MsqidDs::default();
    assert_eq!(x::msgctl(q, IPC_STAT, (&raw mut ds).cast()), 0);
    assert_eq!(x::msgctl(q, IPC_SET, (&raw mut ds).cast()), 0, "IPC_SET (the owner may set what IPC_STAT read)");
    let mut info = [0u8; 32];
    assert!(x::msgctl(q, IPC_INFO, info.as_mut_ptr().cast()) >= 0, "Linux-specific info");
    assert_eq!(x::msgctl(q, IPC_RMID, null_mut()), 0, "IPC_RMID");
    let s = x::semget(IPC_PRIVATE, 1, 0o600);
    let mut sds = SemidDs::default();
    assert_eq!(x::liella_semctl4(s, 0, IPC_STAT, (&raw mut sds) as usize), 0);
    assert_eq!(x::liella_semctl4(s, 0, IPC_SET, (&raw mut sds) as usize), 0);
    assert_eq!(x::liella_semctl4(s, 0, IPC_RMID, 0), 0);
    let m = x::shmget(IPC_PRIVATE, 4096, 0o600);
    let mut mds = ShmidDs::default();
    assert_eq!(x::shmctl(m, IPC_STAT, (&raw mut mds).cast()), 0);
    assert_eq!(x::shmctl(m, IPC_SET, (&raw mut mds).cast()), 0);
    assert_eq!(x::shmctl(m, IPC_RMID, null_mut()), 0);
}

/// Waits until this process's thread `tid` is asleep inside syscall `nr` (`/proc/self/task/<tid>/syscall`
/// names the syscall of a blocked thread, `stat` its state), so a test acts only once the waiter is
/// really waiting. Bounded at 60 s, never a fixed sleep.
fn wait_blocked_in_syscall(tid: &str, nr: u32) {
    let start = std::time::Instant::now();
    let prefix = format!("{nr} ");
    let blocked = loop {
        if start.elapsed() >= std::time::Duration::from_secs(60) {
            break false;
        }
        let sc = std::fs::read_to_string(format!("/proc/self/task/{tid}/syscall")).unwrap_or_default();
        let st = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).unwrap_or_default();
        let state = st.rsplit_once(')').and_then(|(_, r)| r.trim_start().chars().next());
        if sc.starts_with(&prefix) && state == Some('S') {
            break true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(blocked, "thread {tid} not blocked in syscall {nr} after {:?}", start.elapsed());
}

#[test]
fn s25_2_f_0() {
    errno::set_raw(0);
    assert!(err(x::msgget(0x7e57_0000 | (std::process::id() as i32 & 0xffff), 0) as isize, Errno::ENOENT), "ENOENT");
    let q = x::msgget(IPC_PRIVATE, 0o600);
    let mut m = Msg { mtype: 0, text: [0u8; 1] };
    assert!(err(x::msgrcv(q, (&raw mut m).cast(), 1, 0, IPC_NOWAIT), Errno::ENOMSG), "ENOMSG");
    send(q, 1, b"x");
    let mut z = Msg { mtype: 0, text: [0u8; 0] };
    assert!(err(x::msgrcv(q, (&raw mut z).cast(), 0, 0, 0), Errno::E2BIG), "E2BIG");
    // EIDRM: removed while a receiver waits.
    let mut m2 = Msg { mtype: 0, text: [0u8; 1] };
    x::msgrcv(q, (&raw mut m2).cast(), 1, 0, 0);
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let waiter = std::thread::spawn(move || {
        let mut m = Msg { mtype: 0, text: [0u8; 1] };
        let me = std::fs::read_link("/proc/thread-self").unwrap();
        tx.send(me.file_name().unwrap().to_string_lossy().into_owned()).unwrap();
        let r = x::msgrcv(q, (&raw mut m).cast(), 1, 0, 0);
        (r, errno::get())
    });
    wait_blocked_in_syscall(&rx.recv().unwrap(), 70 /* msgrcv */);
    x::msgctl(q, IPC_RMID, null_mut());
    assert_eq!(waiter.join().unwrap(), (-1, Errno::EIDRM.get()), "EIDRM");
    let s = x::semget(IPC_PRIVATE, 1, 0o600);
    assert!(err(x::liella_semctl4(s, 0, SETVAL, 40000) as isize, Errno::ERANGE), "ERANGE: above SEMVMX");
    x::liella_semctl4(s, 0, IPC_RMID, 0);
}

// ------------------------------------------------------------------------------ POSIX queues (§25.3)

const O_RDONLY: c_int = 0;
const O_WRONLY: c_int = 1;
const O_RDWR: c_int = 2;
const O_CREAT: c_int = 0o100;
const O_EXCL: c_int = 0o200;
const O_NONBLOCK: c_int = 0o4000;
const IPCNS: &str = "LIELLA_S25_IPCNS";

/// Run `body` in a private IPC namespace: this test re-executed under `unshare --user --ipc
/// --map-current-user` (see the module documentation). In the namespace, `body` runs and the
/// test passes when it returns.
fn in_ipc_ns(test: &str, body: impl FnOnce()) {
    if std::env::var(IPCNS).as_deref() == Ok(test) {
        body();
        return;
    }
    let ns = ["--user", "--ipc", "--map-current-user"];
    if !Command::new("unshare").args(ns).arg("true").status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no private IPC namespace (unshare --user --ipc --map-current-user)");
        return;
    }
    let o = Command::new("unshare").args(ns).arg(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(IPCNS, test).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("1 passed"), "{test} in its namespace: {:?}\n{out}{}", o.status, String::from_utf8_lossy(&o.stderr));
}

/// `mq_open(name, flags, mode, attr)` through the variadic adapter's target.
fn mq(name: &core::ffi::CStr, flags: c_int, mode: u32, attr: Option<&MqAttr>) -> c_int {
    x::liella_mq_open4(name.as_ptr(), flags, mode, attr.map_or(null(), |a| a as *const MqAttr))
}

fn mq_put(q: c_int, m: &[u8], prio: u32) -> c_int {
    x::mq_send(q, m.as_ptr().cast(), m.len(), prio)
}

/// A received message and its priority (`None`: -1, the error in `errno`).
fn recv(q: c_int, room: usize) -> Option<(Vec<u8>, u32)> {
    let mut b = vec![0u8; room];
    let mut prio = 0;
    let n = x::mq_receive(q, b.as_mut_ptr().cast(), room, Some(&mut prio));
    (n >= 0).then(|| (b[..n as usize].to_vec(), prio))
}

fn attr_of(q: c_int) -> MqAttr {
    let mut a = MqAttr::default();
    assert_eq!(x::mq_getattr(q, Some(&mut a)), 0);
    a
}

fn fails(r: isize, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

#[test]
fn s25_3_c_0() {
    in_ipc_ns("s25_3_c_0", || {
        let q = mq(c"/c0", O_RDWR | O_CREAT | O_EXCL, 0o600, Some(&MqAttr::new(4, 16)));
        assert!(q >= 0, "created");
        assert_eq!(mq_put(q, b"hello", 1), 0);
        let other = mq(c"/c0", O_RDONLY, 0, None);
        assert!(other >= 0, "the same queue by name");
        assert_eq!(recv(other, 16), Some((b"hello".to_vec(), 1)), "a message and its priority");
        assert_eq!(x::mq_unlink(c"/c0".as_ptr()), 0);
        errno::set_raw(0);
        assert!(fails(mq(c"/c0", O_RDONLY, 0, None) as isize, Errno::ENOENT), "the name is gone");
        assert_eq!(mq_put(q, b"still", 0), 0, "open descriptors keep working");
        assert_eq!(x::mq_close(q), 0);
        assert_eq!(x::mq_close(other), 0);
    });
}

#[test]
fn s25_3_c_1() {
    in_ipc_ns("s25_3_c_1", || {
        let q = mq(c"/name", O_RDWR | O_CREAT, 0o600, None);
        assert!(q >= 0, "\"/name\"");
        x::mq_close(q);
        errno::set_raw(0);
        assert!(fails(mq(c"name", O_RDWR | O_CREAT, 0o600, None) as isize, Errno::EINVAL), "no leading slash");
        assert!(fails(mq(c"", O_RDWR | O_CREAT, 0o600, None) as isize, Errno::EINVAL), "empty");
        // What follows the slash is the kernel's to judge (mq_open(3)).
        assert!(fails(mq(c"//name", O_RDWR | O_CREAT, 0o600, None) as isize, Errno::EACCES), "a second slash");
        assert!(fails(mq(c"/a/b", O_RDWR | O_CREAT, 0o600, None) as isize, Errno::EACCES), "a further slash");
        assert!(fails(mq(c"/", O_RDWR | O_CREAT, 0o600, None) as isize, Errno::ENOENT), "the slash alone");
        let long = std::ffi::CString::new(format!("/{}", "n".repeat(256))).unwrap();
        assert!(fails(mq(&long, O_RDWR | O_CREAT, 0o600, None) as isize, Errno::ENAMETOOLONG), "beyond NAME_MAX");
        assert!(fails(x::mq_unlink(c"name".as_ptr()) as isize, Errno::EINVAL));
        assert_eq!(x::mq_unlink(c"/name".as_ptr()), 0);
    });
}

#[test]
fn s25_3_c_2() {
    in_ipc_ns("s25_3_c_2", || {
        let q = mq(c"/c2", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(8, 16)));
        assert_eq!(mq_put(q, &[b'x'; 16], 0), 0, "mq_msgsize bytes");
        errno::set_raw(0);
        assert!(fails(mq_put(q, &[b'x'; 17], 0) as isize, Errno::EMSGSIZE), "one more");
        recv(q, 16).unwrap();
        for (m, p) in [(&b"a1"[..], 1), (b"b5", 5), (b"c3", 3), (b"d5", 5), (b"e0", 0), (b"f32767", 32767)] {
            assert_eq!(mq_put(q, m, p), 0);
        }
        let order: Vec<(Vec<u8>, u32)> = (0..6).map(|_| recv(q, 16).unwrap()).collect();
        let want: Vec<(Vec<u8>, u32)> = [(&b"f32767"[..], 32767), (b"b5", 5), (b"d5", 5), (b"c3", 3), (b"a1", 1), (b"e0", 0)].iter().map(|(m, p)| (m.to_vec(), *p)).collect();
        assert_eq!(order, want, "highest priority first, in order of sending within one");
        assert!(fails(mq_put(q, b"x", 32768) as isize, Errno::EINVAL), "priority MQ_PRIO_MAX");
        x::mq_close(q);
        x::mq_unlink(c"/c2".as_ptr());
    });
}

static N_CALLS: AtomicU32 = AtomicU32::new(0);
static N_VALUE: AtomicUsize = AtomicUsize::new(0);
static N_TID: AtomicI32 = AtomicI32::new(0);

extern "C" fn notified(v: usize) {
    N_VALUE.store(v, Ordering::SeqCst);
    N_TID.store(CSys.gettid(), Ordering::SeqCst);
    N_CALLS.fetch_add(1, Ordering::SeqCst);
}

fn until(limit: Duration, done: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while !done() && t.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(2));
    }
    done()
}

/// In a forked child (process-directed signals): `SIGEV_SIGNAL` on an arrival at an empty queue.
fn signal_child(k: &CSys) -> i32 {
    let usr1 = SigSet::empty().with(SIGUSR1);
    if k.sigprocmask(SIG_BLOCK, Some(&usr1)).is_err() {
        return 1;
    }
    let q = mq(c"/sig", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(4, 8)));
    let ev = SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 42, 0);
    if q < 0 || x::mq_notify(q, Some(&ev)) != 0 {
        return 2;
    }
    if x::mq_notify(q, Some(&ev)) != -1 || errno::get() != Errno::EBUSY.get() {
        return 3;
    }
    if x::mq_send(q, c"m".as_ptr(), 1, 0) != 0 {
        return 4;
    }
    let mut info = SigInfo::default();
    let zero = Timespec { sec: 2, nsec: 0 };
    match k.sigtimedwait(&usr1, Some(&mut info), Some(&zero)) {
        Ok(s) if s == SIGUSR1 && info.code == -3 && info.fields[1] == 42 => {}
        _ => return 5,
    }
    // One-shot: the next arrival (on a non-empty queue, and then on an emptied one) is silent.
    let _ = x::mq_send(q, c"n".as_ptr(), 1, 0);
    let none = Timespec { sec: 0, nsec: 100_000_000 };
    if k.sigtimedwait(&usr1, None, Some(&none)).is_ok() {
        return 6;
    }
    x::mq_close(q);
    x::mq_unlink(c"/sig".as_ptr());
    0
}

#[test]
fn s25_3_c_3() {
    in_ipc_ns("s25_3_c_3", || {
        assert_eq!(liella_boundary::selftest::in_child(signal_child), Ok(ChildEnd::Exited(0)), "SIGEV_SIGNAL");
        // SIGEV_THREAD: a new thread calls the function with sigev_value, once per registration.
        let q = mq(c"/c3", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(4, 8)));
        let f = notified as extern "C" fn(usize) as usize as u64;
        let ev = SigEvent::thread(0xfeed, f, 0);
        assert_eq!(x::mq_notify(q, Some(&ev)), 0);
        errno::set_raw(0);
        assert!(fails(x::mq_notify(q, Some(&ev)) as isize, Errno::EBUSY), "a single registrant");
        assert!(fails(x::mq_notify(q, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))) as isize, Errno::EBUSY));
        assert_eq!(mq_put(q, b"m", 0), 0);
        assert!(until(Duration::from_secs(5), || N_CALLS.load(Ordering::SeqCst) == 1), "notified");
        assert_eq!(N_VALUE.load(Ordering::SeqCst), 0xfeed);
        assert_ne!(N_TID.load(Ordering::SeqCst), CSys.gettid(), "on another thread");
        recv(q, 8).unwrap();
        assert_eq!(mq_put(q, b"m", 0), 0);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(N_CALLS.load(Ordering::SeqCst), 1, "one-shot");
        recv(q, 8).unwrap();
        // Registered again, then removed: nothing.
        assert_eq!(x::mq_notify(q, Some(&ev)), 0);
        assert_eq!(x::mq_notify(q, None), 0, "removal");
        assert_eq!(mq_put(q, b"m", 0), 0);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(N_CALLS.load(Ordering::SeqCst), 1);
        // A message to a non-empty queue does not notify.
        assert_eq!(x::mq_notify(q, Some(&ev)), 0);
        assert_eq!(mq_put(q, b"m", 0), 0);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(N_CALLS.load(Ordering::SeqCst), 1, "the queue was not empty");
        x::mq_close(q);
        x::mq_unlink(c"/c3".as_ptr());
    });
}

/// Calls of [`recorded`] per `sigev_value` (0 to 3).
static R_SEEN: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];

extern "C" fn recorded(v: usize) {
    if let Some(s) = R_SEEN.get(v) {
        s.fetch_add(1, Ordering::SeqCst);
    }
}

/// §25.3 (D-25.5): every `SIGEV_THREAD` registration notifies its own function with its own
/// value. One removed by closing its queue never fires, and many registrations removed with
/// `mq_notify(q, NULL)` leave later ones working (their bookkeeping is reused, never confused).
#[test]
fn s25_3_c_3_records() {
    in_ipc_ns("s25_3_c_3_records", || {
        let f = recorded as extern "C" fn(usize) as usize as u64;
        let attr = MqAttr::new(4, 8);
        let (a, b, c) = (mq(c"/r1", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&attr)), mq(c"/r2", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&attr)), mq(c"/r3", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&attr)));
        assert!(a >= 0 && b >= 0 && c >= 0);
        assert_eq!(x::mq_notify(a, Some(&SigEvent::thread(1, f, 0))), 0);
        assert_eq!(x::mq_notify(b, Some(&SigEvent::thread(2, f, 0))), 0);
        assert_eq!(x::mq_close(a), 0, "removes registration 1");
        for _ in 0..200 {
            assert_eq!(x::mq_notify(c, Some(&SigEvent::thread(0, f, 0))), 0);
            assert_eq!(x::mq_notify(c, None), 0);
        }
        assert_eq!(mq_put(b, b"m", 0), 0);
        assert!(until(Duration::from_secs(5), || R_SEEN[2].load(Ordering::SeqCst) == 1), "registration 2");
        assert_eq!(x::mq_notify(c, Some(&SigEvent::thread(3, f, 0))), 0);
        assert_eq!(mq_put(c, b"m", 0), 0);
        assert!(until(Duration::from_secs(5), || R_SEEN[3].load(Ordering::SeqCst) == 1), "registration 3");
        std::thread::sleep(Duration::from_millis(100));
        let seen: Vec<u32> = R_SEEN.iter().map(|s| s.load(Ordering::SeqCst)).collect();
        assert_eq!(seen, [0, 0, 1, 1], "only the live registrations, once each");
        assert!(until(Duration::from_secs(5), || liella_boundary::selftest::mq_records_in_use() == 0), "every record given back (removed, or notified)");
        x::mq_close(b);
        x::mq_close(c);
        for n in [c"/r1", c"/r2", c"/r3"] {
            x::mq_unlink(n.as_ptr());
        }
    });
}

/// §25.3 (D-25.5): a `SIGEV_THREAD` registration the kernel refuses (`EBADF` on a closed
/// descriptor, `EBUSY` on a queue that already has a registrant) keeps no bookkeeping, and a
/// removed registration gives its bookkeeping back.
#[test]
fn s25_3_c_3_refused() {
    in_ipc_ns("s25_3_c_3_refused", || {
        let f = recorded as extern "C" fn(usize) as usize as u64;
        let q = mq(c"/u1", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(4, 8)));
        let gone = mq(c"/u2", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(4, 8)));
        assert!(q >= 0 && gone >= 0);
        assert_eq!(x::mq_close(gone), 0);
        let base = liella_boundary::selftest::mq_records_in_use();
        for _ in 0..20 {
            assert!(fails(x::mq_notify(gone, Some(&SigEvent::thread(0, f, 0))) as isize, Errno::EBADF), "closed descriptor");
        }
        assert_eq!(liella_boundary::selftest::mq_records_in_use(), base, "nothing kept after EBADF");
        assert_eq!(x::mq_notify(q, Some(&SigEvent::thread(0, f, 0))), 0);
        assert_eq!(liella_boundary::selftest::mq_records_in_use(), base + 1, "the registration");
        for _ in 0..20 {
            assert!(fails(x::mq_notify(q, Some(&SigEvent::thread(0, f, 0))) as isize, Errno::EBUSY), "a single registrant");
        }
        assert_eq!(liella_boundary::selftest::mq_records_in_use(), base + 1, "nothing kept after EBUSY");
        assert_eq!(x::mq_notify(q, None), 0);
        assert!(until(Duration::from_secs(5), || liella_boundary::selftest::mq_records_in_use() == base), "the removed registration's record freed");
        x::mq_close(q);
        for n in [c"/u1", c"/u2"] {
            x::mq_unlink(n.as_ptr());
        }
    });
}

/// Calls of [`fork_notified`] (in each process its own) and the last value it saw.
static F_SEEN: AtomicU32 = AtomicU32::new(0);
static F_VALUE: AtomicUsize = AtomicUsize::new(0);

extern "C" fn fork_notified(v: usize) {
    F_VALUE.store(v, Ordering::SeqCst);
    F_SEEN.fetch_add(1, Ordering::SeqCst);
}

/// In a child of the library's `fork` of a process with a `SIGEV_THREAD` registration: a
/// registration of its own, on another queue, notifies it (once, with its value), and none of
/// the parent's records is in use.
fn thread_child(_k: &CSys) -> i32 {
    if liella_boundary::selftest::mq_records_in_use() != 0 {
        return 5;
    }
    let q =mq(c"/k2", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(4, 8)));
    let f = fork_notified as extern "C" fn(usize) as usize as u64;
    if q < 0 || x::mq_notify(q, Some(&SigEvent::thread(7, f, 0))) != 0 {
        return 1;
    }
    if mq_put(q, b"m", 0) != 0 {
        return 2;
    }
    if !until(Duration::from_secs(5), || F_SEEN.load(Ordering::SeqCst) == 1) {
        return 3;
    }
    if F_VALUE.load(Ordering::SeqCst) != 7 {
        return 4;
    }
    0
}

/// §25.3 (D-25.5): a `fork` child inherits no `SIGEV_THREAD` machinery: its own registrations
/// notify it, nothing of its reaches the parent, and the parent's registration still notifies
/// the parent afterwards. The child is made by the library's `fork` (`in_library_child`), which
/// runs the after-fork resets; `in_child`'s raw fork does not, so it cannot test this.
#[test]
fn s25_3_c_3_fork() {
    in_ipc_ns("s25_3_c_3_fork", || {
        let q = mq(c"/k1", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(4, 8)));
        let f = fork_notified as extern "C" fn(usize) as usize as u64;
        assert_eq!(x::mq_notify(q, Some(&SigEvent::thread(5, f, 0))), 0);
        assert_eq!(liella_boundary::selftest::in_library_child(thread_child), Ok(ChildEnd::Exited(0)), "the child's own notification");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(F_SEEN.load(Ordering::SeqCst), 0, "nothing in the parent from the child");
        assert_eq!(mq_put(q, b"m", 0), 0);
        assert!(until(Duration::from_secs(5), || F_SEEN.load(Ordering::SeqCst) == 1), "the parent's");
        assert_eq!(F_VALUE.load(Ordering::SeqCst), 5, "the parent's value");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(F_SEEN.load(Ordering::SeqCst), 1, "once");
        x::mq_close(q);
        x::mq_unlink(c"/k1".as_ptr());
        x::mq_unlink(c"/k2".as_ptr());
    });
}

#[test]
fn s25_3_c_4() {
    in_ipc_ns("s25_3_c_4", || {
        let q = mq(c"/c4", O_RDWR | O_CREAT, 0o600, Some(&MqAttr::new(3, 8)));
        let a = attr_of(q);
        assert_eq!((a.flags, a.maxmsg, a.msgsize, a.curmsgs), (0, 3, 8, 0));
        mq_put(q, b"m", 0);
        assert_eq!(attr_of(q).curmsgs, 1, "mq_curmsgs");
        let mut new = MqAttr::new(99, 99);
        new.flags = i64::from(O_NONBLOCK);
        new.curmsgs = 99;
        let mut old = MqAttr::default();
        assert_eq!(x::mq_setattr(q, Some(&new), Some(&mut old)), 0);
        assert_eq!((old.flags, old.maxmsg, old.msgsize, old.curmsgs), (0, 3, 8, 1), "the old attributes");
        let a = attr_of(q);
        assert_eq!((a.flags, a.maxmsg, a.msgsize, a.curmsgs), (i64::from(O_NONBLOCK), 3, 8, 1), "only O_NONBLOCK changes");
        recv(q, 8).unwrap();
        errno::set_raw(0);
        assert!(recv(q, 8).is_none() && errno::get() == Errno::EAGAIN.get(), "non-blocking now");
        let d = mq(c"/c4", O_RDWR, 0, None);
        assert_eq!(attr_of(d).flags, 0, "O_NONBLOCK belongs to the open description");
        let dflt = mq(c"/c4d", O_RDWR | O_CREAT, 0o600, None);
        let a = attr_of(dflt);
        assert_eq!((a.maxmsg, a.msgsize), (10, 8192), "no attributes: the namespace's defaults");
        for q in [q, d, dflt] {
            x::mq_close(q);
        }
        x::mq_unlink(c"/c4".as_ptr());
        x::mq_unlink(c"/c4d".as_ptr());
    });
}

#[test]
fn s25_3_f_0() {
    in_ipc_ns("s25_3_f_0", || {
        let q = mq(c"/f0", O_RDWR | O_CREAT | O_NONBLOCK, 0o600, Some(&MqAttr::new(1, 8)));
        errno::set_raw(0);
        assert!(recv(q, 8).is_none() && errno::get() == Errno::EAGAIN.get(), "EAGAIN: empty, non-blocking");
        assert_eq!(mq_put(q, b"m", 0), 0);
        assert!(fails(mq_put(q, b"m", 0) as isize, Errno::EAGAIN), "EAGAIN: full, non-blocking");
        assert!(fails(mq_put(q, &[0; 9], 0) as isize, Errno::EMSGSIZE), "EMSGSIZE: too long");
        assert!(recv(q, 7).is_none() && errno::get() == Errno::EMSGSIZE.get(), "EMSGSIZE: buffer below mq_msgsize");
        let b = mq(c"/f0", O_RDWR, 0, None);
        let past = Timespec { sec: 0, nsec: 0 };
        assert!(fails(x::mq_timedsend(b, c"m".as_ptr(), 1, 0, Some(&past)) as isize, Errno::ETIMEDOUT), "ETIMEDOUT: full");
        let bad = Timespec { sec: 0, nsec: 1_000_000_000 };
        assert!(fails(x::mq_timedsend(b, c"m".as_ptr(), 1, 0, Some(&bad)) as isize, Errno::EINVAL), "EINVAL: a bad timeout");
        let mut buf = [0u8; 8];
        recv(q, 8).unwrap();
        assert!(fails(x::mq_timedreceive(b, buf.as_mut_ptr().cast(), 8, None, Some(&past)), Errno::ETIMEDOUT), "ETIMEDOUT: empty");
        assert!(fails(mq_put(-1, b"m", 0) as isize, Errno::EBADF), "EBADF");
        let ro = mq(c"/f0", O_RDONLY, 0, None);
        let wo = mq(c"/f0", O_WRONLY, 0, None);
        assert!(fails(mq_put(ro, b"m", 0) as isize, Errno::EBADF), "EBADF: not open for writing");
        assert!(recv(wo, 8).is_none() && errno::get() == Errno::EBADF.get(), "EBADF: not open for reading");
        assert!(fails(mq(c"f0", O_RDWR, 0, None) as isize, Errno::EINVAL), "EINVAL: a bad name");
        assert!(fails(mq(c"/f1", O_RDWR | O_CREAT, 0o600, Some(&MqAttr::new(11, 8))) as isize, Errno::EINVAL), "EINVAL: mq_maxmsg above msg_max");
        assert!(fails(mq(c"/f1", O_RDWR | O_CREAT, 0o600, Some(&MqAttr::new(1, 8193))) as isize, Errno::EINVAL), "EINVAL: mq_msgsize above msgsize_max");
        assert!(fails(mq(c"/f1", O_RDWR | O_CREAT, 0o600, Some(&MqAttr::new(0, 8))) as isize, Errno::EINVAL), "EINVAL: no messages");
        assert_eq!(x::mq_notify(q, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))), 0);
        assert!(fails(x::mq_notify(b, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))) as isize, Errno::EBUSY), "EBUSY");
        assert!(fails(x::mq_notify(q, Some(&SigEvent::thread(0, 0, 0))) as isize, Errno::EINVAL), "EINVAL: SIGEV_THREAD without a function");
        for d in [q, b, ro, wo] {
            x::mq_close(d);
        }
        assert!(fails(x::mq_close(q) as isize, Errno::EBADF), "EBADF: closed");
        x::mq_unlink(c"/f0".as_ptr());
    });
}

#[test]
fn s25_3_x_0() {
    // Send and receive act on cancellation, pending at the call or arriving while blocked:
    // C threads in a private IPC namespace.
    conformance(&["s25_mqueue"]);
}

// ------------------------------------------------------------------------------ POSIX shared memory (§25.4)

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

#[test]
fn s25_4_c_0() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/test-scratch/s25-shm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let mut d = dir.as_os_str().as_encoded_bytes().to_vec();
    d.push(b'/');
    // The only test of this file that uses shared memory: the directory is its alone.
    liella_boundary::selftest::shm_dir(Some(&d));
    let fd = x::shm_open(c"/obj".as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600);
    assert!(fd >= 0, "created");
    assert!(dir.join("obj").exists(), "a file named after the name without its slash");
    assert_eq!(io::liella_fcntl3(fd, 1, 0), 1, "close-on-exec");
    assert_eq!(io::liella_fcntl3(fd, 3, 0) & O_NONBLOCK, 0, "no O_NONBLOCK unless asked");
    assert_eq!(liella_boundary::posix::s13_filesystem::ftruncate(fd, 8192), 0, "sized with ftruncate");
    let map = io::mmap(null_mut(), 8192, 3, 1, fd, 0);
    assert_ne!(map as isize, -1, "mapped with mmap (shared, read and write)");
    assert_eq!(io::munmap(map, 8192), 0);
    let ro = x::shm_open(c"obj".as_ptr(), O_RDONLY, 0);
    assert!(ro >= 0, "opened by name");
    assert_eq!(io::pwrite(fd, b"shared".as_ptr().cast(), 6, 100), 6);
    let mut b = [0u8; 6];
    assert_eq!(io::pread(ro, b.as_mut_ptr().cast(), 6, 100), 6);
    assert_eq!(&b, b"shared", "the same object");
    errno::set_raw(0);
    assert_eq!(io::mmap(null_mut(), 8192, 2, 1, ro, 0) as isize, -1, "PROT_WRITE on a read-only descriptor");
    assert_eq!(errno::get(), Errno::EACCES.get());
    // Names: one component after the slash.
    for (name, e) in [(c"/a/b", Errno::EINVAL), (c"/", Errno::EINVAL), (c"", Errno::EINVAL), (c"/.", Errno::EINVAL), (c"/..", Errno::EINVAL)] {
        assert!(fails(x::shm_open(name.as_ptr(), O_RDWR | O_CREAT, 0o600) as isize, e), "{name:?}");
    }
    // Only regular files are objects; a FIFO is not waited on.
    std::fs::create_dir(dir.join("dir")).unwrap();
    assert!(fails(x::shm_open(c"/dir".as_ptr(), O_RDONLY, 0) as isize, Errno::EINVAL), "a directory");
    let fifo = std::ffi::CString::new(dir.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(liella_boundary::posix::s13_filesystem::mkfifo(fifo.as_ptr(), 0o600), 0);
    assert!(fails(x::shm_open(c"/fifo".as_ptr(), O_RDONLY, 0) as isize, Errno::EINVAL), "a FIFO");
    // shm_unlink removes the name; the descriptors stay usable.
    assert_eq!(x::shm_unlink(c"/obj".as_ptr()), 0);
    assert!(!dir.join("obj").exists());
    assert!(fails(x::shm_open(c"/obj".as_ptr(), O_RDONLY, 0) as isize, Errno::ENOENT), "gone");
    assert_eq!(io::pread(ro, b.as_mut_ptr().cast(), 6, 100), 6);
    assert!(fails(x::shm_unlink(c"/obj".as_ptr()) as isize, Errno::ENOENT));
    assert!(fails(x::shm_unlink(c"/a/b".as_ptr()) as isize, Errno::ENOENT), "no object has that name");
    io::close(fd);
    io::close(ro);
    liella_boundary::selftest::shm_dir(None);
    std::fs::remove_dir_all(&dir).unwrap();
}
