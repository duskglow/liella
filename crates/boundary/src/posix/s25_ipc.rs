//! Translation for spec §25: pipes (§25.1) and System V IPC (§25.2) (phase 5); POSIX message
//! queues and shared memory (§25.3–25.4) (phase 13).
//!
//! The `*ctl` arguments are decoded by command (`liella_legacy::ipc_compat::*_shape`, then
//! `conv`). `semctl` is variadic: its C entry point is an adapter
//! (`csys/src/adapter/variadic.c`) forwarding to `liella_semctl4` (OD-C1). `shmat` and `shmdt`
//! take the caller's addresses through `crate::raw` (D-11.7, as `mmap`).
//!
//! POSIX queues and objects are `liella::ipc`; `mq_notify`'s `SIGEV_THREAD` and the
//! shared-memory directory are `crate::mqueue` (D-25.2, D-25.3). `mq_open` is variadic: its C
//! entry point is an adapter forwarding to `liella_mq_open4`, which reads the attributes only
//! with `O_CREAT`. Sending and receiving are cancellation points (§25.3). `__mq_open_2`, the
//! fortified form for a call without the optional arguments, refuses `O_CREAT` with `EINVAL`
//! (it creates nothing; the reference aborts).

use liella::ipc;
use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, c_void, size_t, ssize_t};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MqAttr, Sembuf, SigEvent, Timespec};
use liella_legacy::ipc_compat;

use crate::cancel;
use crate::conv;
use crate::export;
use crate::mqueue;
use crate::raw;
use crate::ret;
use crate::strz;
use crate::sys::{CSys, CP};
use crate::timer;

export! { "GLIBC_2.2.5";
pub extern "C" fn pipe(fds: Option<&mut [c_int; 2]>) -> c_int {
    ret::pair(fds, CSys.pipe2(0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pipe(fds: Option<&mut [c_int; 2]>) -> c_int {
    ret::pair(fds, CSys.pipe2(0))
}}

export! { "GLIBC_2.9";
pub extern "C" fn pipe2(fds: Option<&mut [c_int; 2]>, flags: c_int) -> c_int {
    ret::pair(fds, CSys.pipe2(conv::bits(flags)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftok(path: *const c_char, id: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§25.2).
    // A key is a signed `key_t` and a negative one (project id >= 128) is a valid result, so it
    // must not go through the non-negative `ret::int32`; only a failure is -1 with errno.
    match unsafe { strz::path(path) }.and_then(|p| ipc_compat::ftok(&CSys, p, id)) {
        Ok(key) => key,
        Err(e) => ret::value_with(-1, Some(e)),
    }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn msgget(key: c_int, flags: c_int) -> c_int {
    ret::int32(CSys.msgget(key, conv::bits(flags)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn msgsnd(id: c_int, msgp: *const c_void, n: size_t, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: msgp is a struct msgbuf with n text bytes (§25.2).
    ret::zero(unsafe { conv::msg(msgp, n) }.and_then(|b| CSys.msgsnd(id, b, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn msgrcv(id: c_int, msgp: *mut c_void, n: size_t, mtype: c_long, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: msgp is a writable struct msgbuf with room for n text bytes (§25.2).
    ret::ssize(unsafe { conv::msg_mut(msgp, n) }.and_then(|b| CSys.msgrcv(id, b, mtype, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn msgctl(id: c_int, cmd: c_int, buf: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: buf is what cmd takes: ignored, or the caller's msqid_ds or msginfo (§25.2).
    ret::int(unsafe { conv::ipc_ctl_arg(ipc_compat::msg_shape(cmd), buf) }.and_then(|a| ipc_compat::msgctl(&CSys, id, cmd, a)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn semget(key: c_int, nsems: c_int, flags: c_int) -> c_int {
    ret::int32(CSys.semget(key, nsems, conv::bits(flags)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn semop(id: c_int, ops: *mut c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: ops holds n sembuf entries (§25.2).
    ret::zero(unsafe { conv::array::<Sembuf>(ops, conv::count_sz(n)) }.and_then(|o| CSys.semtimedop(id, o, None)))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn semtimedop(id: c_int, ops: *mut c_void, n: size_t, timeout: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: ops holds n sembuf entries (§25.2).
    ret::zero(unsafe { conv::array::<Sembuf>(ops, conv::count_sz(n)) }.and_then(|o| CSys.semtimedop(id, o, timeout)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_semctl4(id: c_int, num: c_int, cmd: c_int, arg: usize) -> c_int {
    // UNSAFE(slice) SAFETY: arg is the union semun member cmd takes (§25.2).
    ret::int(unsafe { conv::semctl_arg(cmd, arg, || ipc_compat::nsems(&CSys, id)) }.and_then(|a| ipc_compat::semctl(&CSys, id, num, cmd, a)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn shmget(key: c_int, size: size_t, flags: c_int) -> c_int {
    ret::int32(CSys.shmget(key, size, conv::bits(flags)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn shmat(id: c_int, addr: *const c_void, flags: c_int) -> *mut c_void {
    ret::addr_or_neg1(raw::shmat(id, conv::addr(addr), flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn shmdt(addr: *const c_void) -> c_int {
    ret::zero(raw::shmdt(conv::addr(addr)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn shmctl(id: c_int, cmd: c_int, buf: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: buf is what cmd takes: ignored, or the caller's shmid_ds, shminfo or shm_info (§25.2).
    ret::int(unsafe { conv::ipc_ctl_arg(ipc_compat::shm_shape(cmd), buf) }.and_then(|a| ipc_compat::shmctl(&CSys, id, cmd, a)))
}}

// ------------------------------------------------------------------------------ POSIX queues (§25.3)

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_mq_open4(name: *const c_char, oflag: c_int, mode: c_uint, attr: *const MqAttr) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string; with O_CREAT, attr is NULL or a struct mq_attr, and without it attr is not read (§25.3).
    ret::fd(unsafe { strz::path(name) }.and_then(|n| unsafe { conv::array::<MqAttr>(attr.cast(), mqueue::creation_attrs(conv::bits(oflag), attr)) }.and_then(|a| ipc::mq_open(&CSys, n, conv::bits(oflag), mode, a.first()))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn __mq_open_2(name: *const c_char, oflag: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§25.3).
    ret::fd(unsafe { strz::path(name) }.and_then(|n| mqueue::open_2(n, conv::bits(oflag))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_close(q: c_int) -> c_int {
    ret::zero(CSys.close(conv::fd(q)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_unlink(name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§25.3).
    ret::zero(unsafe { strz::path(name) }.and_then(|n| ipc::mq_unlink(&CSys, n)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_send(q: c_int, msg: *const c_char, len: size_t, prio: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: msg holds len readable bytes (§25.3).
    cancel::point(ret::zero(unsafe { conv::buf(msg.cast(), len) }.and_then(|m| CP.mq_timedsend(conv::fd(q), m, prio, None))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_timedsend(q: c_int, msg: *const c_char, len: size_t, prio: c_uint, timeout: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: msg holds len readable bytes (§25.3).
    cancel::point(ret::zero(unsafe { conv::buf(msg.cast(), len) }.and_then(|m| CP.mq_timedsend(conv::fd(q), m, prio, timeout))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_receive(q: c_int, buf: *mut c_char, len: size_t, prio: Option<&mut c_uint>) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf has room for len bytes (§25.3).
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf.cast(), len) }.and_then(|b| CP.mq_timedreceive(conv::fd(q), b, prio, None))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_timedreceive(q: c_int, buf: *mut c_char, len: size_t, prio: Option<&mut c_uint>, timeout: Option<&Timespec>) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf has room for len bytes (§25.3).
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf.cast(), len) }.and_then(|b| CP.mq_timedreceive(conv::fd(q), b, prio, timeout))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_notify(q: c_int, event: Option<&SigEvent>) -> c_int {
    // UNSAFE(slice) SAFETY: a SIGEV_THREAD event's sigev_notify_attributes is NULL or an initialised pthread_attr_t (§22.2).
    ret::zero(mqueue::notify(conv::fd(q), event, unsafe { conv::opt_obj64(timer::attributes(event)) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_getattr(q: c_int, attr: Option<&mut MqAttr>) -> c_int {
    ret::store_if(attr, CSys.mq_getsetattr(conv::fd(q), None))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mq_setattr(q: c_int, new: Option<&MqAttr>, old: Option<&mut MqAttr>) -> c_int {
    ret::store_if(old, CSys.mq_getsetattr(conv::fd(q), new))
}}

// ------------------------------------------------------------------------------ POSIX shared memory (§25.4)

export! { "GLIBC_2.34";
pub extern "C" fn shm_open(name: *const c_char, oflag: c_int, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§25.4).
    ret::fd(unsafe { strz::strz(name) }.and_then(|n| ipc::shm_open(&CSys, mqueue::shm_dir(), n, conv::bits(oflag), mode)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn shm_unlink(name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§25.4).
    ret::zero(unsafe { strz::strz(name) }.and_then(|n| ipc::shm_unlink(&CSys, mqueue::shm_dir(), n)))
}}
