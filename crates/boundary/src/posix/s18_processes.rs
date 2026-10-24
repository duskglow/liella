//! Translation for spec §18: processes. §18.1 (identity) is phase 5; `fork`, `clone` and
//! `atfork` (§18.2) phase 6; the rest phase 11 (`crate::spawn`, `liella::process`).
//!
//! The identity calls cannot fail. `gettid` is a load from the thread block (0x84, D-22.9) in a
//! thread Liella runs, and the system call elsewhere (D-22.10).
//!
//! The exec forms return only on failure (-1 and `errno`). The variadic forms (`execl`,
//! `execle`, `execlp`, `prctl`, `ptrace`) arrive through the C layer's adapters. `popen` and
//! `pclose` (listed in §12 for `<stdio.h>`) and `unshare`/`setns` (listed in §24) are specified
//! by §18.7 and §18.9 and live here. The waits are cancellation points (§18.6), as is `system`.

use liella_abi::ctypes::{c_char, c_int, c_long, c_short, c_uint, c_ulong, c_void, mode_t, pid_t};
use liella_abi::layout::{sigset_t, FILE};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rusage, SchedParam, SigInfo};
use liella_legacy::spawn_compat::{CFileActions, CSpawnAttr};
use liella::process::{self as lp, FileAction, Program};

use crate::cancel;
use crate::conv;
use crate::export;
use crate::fork;
use crate::raw;
use crate::ret;
use crate::spawn::{self, ListForm};
use crate::strz;
use crate::sys::CSys;

export! { "GLIBC_2.2.5";
pub extern "C" fn getpid() -> c_int {
    CSys.getpid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __getpid() -> c_int {
    CSys.getpid()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getppid() -> c_int {
    CSys.getppid()
}}

export! { "GLIBC_2.30";
pub extern "C" fn gettid() -> c_int {
    crate::pthread::gettid()
}}

// ------------------------------------------------------------------------------ process creation (§18.2)
// `fork` and its forms are exported by the products only (the runtime form): a test process the
// host C library started keeps the host's, which keeps the host's own state right in the child.

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn fork() -> c_int {
    ret::int32(fork::fork())
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn __fork() -> c_int {
    ret::int32(fork::fork())
}}

export! { runtime "GLIBC_2.34";
#[allow(non_snake_case)] // the C name
pub extern "C" fn _Fork() -> c_int {
    ret::int32(fork::fork_raw())
}}

export! { local "liella_vfork_fail";
/// `vfork`'s error path: `vfork` and `__vfork` are assembly (csys/src/machine/vfork.S, D-18.1),
/// which jumps here in the parent with the raw result of a failed `SYS_vfork` (-4095..-1).
/// Sets errno; returns -1.
pub extern "C" fn vfork_fail(raw: i64) -> c_int {
    ret::int32(Err(liella_kernel::Errno::decode(raw).err().unwrap_or(liella_kernel::Errno::EAGAIN)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_clone7(func: usize, stack: usize, flags: c_int, arg: usize, ptid: usize, tls: usize, ctid: usize) -> c_int {
    ret::int32(fork::clone(func, stack, flags, arg, ptid, tls, ctid))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn __register_atfork(prepare: usize, parent: usize, child: usize, dso: *mut c_void) -> c_int {
    ret::errnum(fork::register_dso(prepare, parent, child, conv::addr(dso)))
}}

// ------------------------------------------------------------------------------ exec (§18.3)

export! { "GLIBC_2.2.5";
pub extern "C" fn execve(path: *const c_char, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.3).
    ret::zero(Err(spawn::execve(unsafe { strz::path(path) }, conv::addr(argv), conv::addr(envp))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn execv(path: *const c_char, argv: *const *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.3).
    ret::zero(Err(spawn::execve(unsafe { strz::path(path) }, conv::addr(argv), spawn::environ())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn execvp(file: *const c_char, argv: *const *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: file is a NUL-terminated string (§18.3).
    ret::zero(Err(spawn::execvpe(unsafe { strz::path(file) }, conv::addr(argv), spawn::environ())))
}}

export! { "GLIBC_2.11";
pub extern "C" fn execvpe(file: *const c_char, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: file is a NUL-terminated string (§18.3).
    ret::zero(Err(spawn::execvpe(unsafe { strz::path(file) }, conv::addr(argv), conv::addr(envp))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_execl4(form: c_int, path: *const c_char, arg0: *const c_char, ap: *mut c_void) -> c_int {
    let form = match form { 1 => ListForm::Env, 2 => ListForm::Search, _ => ListForm::Plain };
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.3).
    ret::zero(Err(spawn::execl(form, unsafe { strz::path(path) }, conv::addr(arg0), conv::addr(ap))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fexecve(fd: c_int, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    ret::zero(Err(spawn::fexecve(fd, conv::addr(argv), conv::addr(envp))))
}}

export! { "GLIBC_2.34";
pub extern "C" fn execveat(dirfd: c_int, path: *const c_char, argv: *const *mut c_char, envp: *const *mut c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.3).
    ret::zero(Err(unsafe { strz::path(path) }.map_or_else(|e| e, |p| raw::execveat(dirfd, p, conv::addr(argv), conv::addr(envp), flags))))
}}

// ------------------------------------------------------------------------------ spawn (§18.4)

export! { "GLIBC_2.15";
pub extern "C" fn posix_spawn(pid: Option<&mut pid_t>, path: *const c_char, fa: Option<&CFileActions>, attr: Option<&CSpawnAttr>, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.4).
    ret::errnum_if(pid, unsafe { strz::path(path) }.and_then(|p| spawn::spawn_c(Program::Path(p), fa, attr.map(CSpawnAttr::values), conv::addr(argv), conv::addr(envp), false)))
}}

export! { "GLIBC_2.15";
pub extern "C" fn posix_spawnp(pid: Option<&mut pid_t>, file: *const c_char, fa: Option<&CFileActions>, attr: Option<&CSpawnAttr>, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: file is a NUL-terminated string (§18.4).
    ret::errnum_if(pid, unsafe { strz::path(file) }.and_then(|f| spawn::spawn_c(spawn::search_program(f), fa, attr.map(CSpawnAttr::values), conv::addr(argv), conv::addr(envp), false)))
}}

export! { "GLIBC_2.39";
pub extern "C" fn pidfd_spawn(pidfd: Option<&mut c_int>, path: *const c_char, fa: Option<&CFileActions>, attr: Option<&CSpawnAttr>, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    let want = pidfd.is_some();
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.4).
    ret::errnum_if(pidfd, unsafe { strz::path(path) }.and_then(|p| spawn::spawn_c(Program::Path(p), fa, attr.map(CSpawnAttr::values), conv::addr(argv), conv::addr(envp), want)))
}}

export! { "GLIBC_2.39";
pub extern "C" fn pidfd_spawnp(pidfd: Option<&mut c_int>, file: *const c_char, fa: Option<&CFileActions>, attr: Option<&CSpawnAttr>, argv: *const *mut c_char, envp: *const *mut c_char) -> c_int {
    let want = pidfd.is_some();
    // UNSAFE(slice) SAFETY: file is a NUL-terminated string (§18.4).
    ret::errnum_if(pidfd, unsafe { strz::path(file) }.and_then(|f| spawn::spawn_c(spawn::search_program(f), fa, attr.map(CSpawnAttr::values), conv::addr(argv), conv::addr(envp), want)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawn_file_actions_init(fa: Option<&mut CFileActions>) -> c_int {
    ret::errnum(spawn::actions_init(fa))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawn_file_actions_destroy(fa: Option<&mut CFileActions>) -> c_int {
    ret::errnum(spawn::actions_destroy(fa))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawn_file_actions_addopen(fa: Option<&mut CFileActions>, fd: c_int, path: *const c_char, oflag: c_int, mode: mode_t) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.4).
    ret::errnum(unsafe { strz::path(path) }.and_then(|p| spawn::add_action(fa, FileAction::Open { fd, path: p, flags: conv::bits(oflag), mode })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawn_file_actions_addclose(fa: Option<&mut CFileActions>, fd: c_int) -> c_int {
    ret::errnum(spawn::add_action(fa, FileAction::Close(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawn_file_actions_adddup2(fa: Option<&mut CFileActions>, fd: c_int, newfd: c_int) -> c_int {
    ret::errnum(spawn::add_action(fa, FileAction::Dup2(fd, newfd)))
}}

export! { "GLIBC_2.29";
pub extern "C" fn posix_spawn_file_actions_addchdir_np(fa: Option<&mut CFileActions>, path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§18.4).
    ret::errnum(unsafe { strz::path(path) }.and_then(|p| spawn::add_action(fa, FileAction::Chdir(p))))
}}

export! { "GLIBC_2.29";
pub extern "C" fn posix_spawn_file_actions_addfchdir_np(fa: Option<&mut CFileActions>, fd: c_int) -> c_int {
    ret::errnum(spawn::add_action(fa, FileAction::Fchdir(fd)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn posix_spawn_file_actions_addclosefrom_np(fa: Option<&mut CFileActions>, from: c_int) -> c_int {
    ret::errnum(spawn::add_action(fa, FileAction::Closefrom(from)))
}}

export! { "GLIBC_2.35";
pub extern "C" fn posix_spawn_file_actions_addtcsetpgrp_np(fa: Option<&mut CFileActions>, fd: c_int) -> c_int {
    ret::errnum(spawn::add_action(fa, FileAction::Tcsetpgrp(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_init(attr: Option<&mut CSpawnAttr>) -> c_int {
    ret::errnum(spawn::object(attr).map(|a| *a = CSpawnAttr::new()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_destroy(attr: Option<&mut CSpawnAttr>) -> c_int {
    ret::errnum(spawn::object(attr).map(|_| ()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_getflags(attr: Option<&CSpawnAttr>, flags: Option<&mut c_short>) -> c_int {
    ret::errnum_store(flags, spawn::object(attr).map(|a| a.flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_setflags(attr: Option<&mut CSpawnAttr>, flags: c_short) -> c_int {
    ret::errnum(spawn::object(attr).and_then(|a| lp::spawn_flags(c_int::from(flags)).map(|f| a.set_flags(f))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_getpgroup(attr: Option<&CSpawnAttr>, pgroup: Option<&mut pid_t>) -> c_int {
    ret::errnum_store(pgroup, spawn::object(attr).map(|a| a.pgrp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_setpgroup(attr: Option<&mut CSpawnAttr>, pgroup: pid_t) -> c_int {
    ret::errnum(spawn::object(attr).map(|a| a.pgrp = pgroup))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_getsigdefault(attr: Option<&CSpawnAttr>, set: Option<&mut sigset_t>) -> c_int {
    ret::errnum_store(set, spawn::object(attr).map(|a| CSpawnAttr::c_set(&a.sd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_setsigdefault(attr: Option<&mut CSpawnAttr>, set: Option<&sigset_t>) -> c_int {
    ret::errnum(spawn::object(attr).and_then(|a| spawn::object(set).map(|s| a.sd = CSpawnAttr::c_set(s))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_getsigmask(attr: Option<&CSpawnAttr>, set: Option<&mut sigset_t>) -> c_int {
    ret::errnum_store(set, spawn::object(attr).map(|a| CSpawnAttr::c_set(&a.ss)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_setsigmask(attr: Option<&mut CSpawnAttr>, set: Option<&sigset_t>) -> c_int {
    ret::errnum(spawn::object(attr).and_then(|a| spawn::object(set).map(|s| a.ss = CSpawnAttr::c_set(s))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_getschedpolicy(attr: Option<&CSpawnAttr>, policy: Option<&mut c_int>) -> c_int {
    ret::errnum_store(policy, spawn::object(attr).map(|a| a.policy))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_setschedpolicy(attr: Option<&mut CSpawnAttr>, policy: c_int) -> c_int {
    ret::errnum(spawn::object(attr).and_then(|a| lp::spawn_policy(policy).map(|_| a.policy = policy)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_getschedparam(attr: Option<&CSpawnAttr>, param: Option<&mut SchedParam>) -> c_int {
    ret::errnum_store(param, spawn::object(attr).map(|a| SchedParam { priority: a.priority }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_spawnattr_setschedparam(attr: Option<&mut CSpawnAttr>, param: Option<&SchedParam>) -> c_int {
    ret::errnum(spawn::object(attr).and_then(|a| spawn::object(param).map(|p| a.priority = p.priority)))
}}

export! { "GLIBC_2.39";
pub extern "C" fn posix_spawnattr_getcgroup_np(attr: Option<&CSpawnAttr>, cgroup: Option<&mut c_int>) -> c_int {
    ret::errnum_store(cgroup, spawn::object(attr).map(|a| a.cgroup))
}}

export! { "GLIBC_2.39";
pub extern "C" fn posix_spawnattr_setcgroup_np(attr: Option<&mut CSpawnAttr>, cgroup: c_int) -> c_int {
    ret::errnum(spawn::object(attr).map(|a| a.cgroup = cgroup))
}}

// ------------------------------------------------------------------------------ pidfd (§18.5)

export! { "GLIBC_2.36";
pub extern "C" fn pidfd_open(pid: pid_t, flags: c_uint) -> c_int {
    ret::fd(CSys.pidfd_open(pid, flags))
}}

export! { "GLIBC_2.36";
pub extern "C" fn pidfd_getfd(pidfd: c_int, targetfd: c_int, flags: c_uint) -> c_int {
    ret::fd(CSys.pidfd_getfd(Fd::new(pidfd), targetfd, flags))
}}

export! { "GLIBC_2.36";
pub extern "C" fn pidfd_send_signal(pidfd: c_int, sig: c_int, info: Option<&SigInfo>, flags: c_uint) -> c_int {
    ret::zero(CSys.pidfd_send_signal(Fd::new(pidfd), sig, info, flags))
}}

export! { "GLIBC_2.39";
pub extern "C" fn pidfd_getpid(pidfd: c_int) -> pid_t {
    ret::int32(spawn::pidfd_getpid(pidfd))
}}

// ------------------------------------------------------------------------------ waiting (§18.6)

export! { "GLIBC_2.2.5";
pub extern "C" fn wait(status: Option<&mut c_int>) -> pid_t {
    cancel::point(ret::int32(spawn::wait4(-1, status, 0, None)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wait(status: Option<&mut c_int>) -> pid_t {
    cancel::point(ret::int32(spawn::wait4(-1, status, 0, None)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn waitpid(pid: pid_t, status: Option<&mut c_int>, options: c_int) -> pid_t {
    cancel::point(ret::int32(spawn::wait4(pid, status, options, None)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __waitpid(pid: pid_t, status: Option<&mut c_int>, options: c_int) -> pid_t {
    cancel::point(ret::int32(spawn::wait4(pid, status, options, None)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wait3(status: Option<&mut c_int>, options: c_int, usage: Option<&mut Rusage>) -> pid_t {
    cancel::point(ret::int32(spawn::wait4(-1, status, options, usage)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wait4(pid: pid_t, status: Option<&mut c_int>, options: c_int, usage: Option<&mut Rusage>) -> pid_t {
    cancel::point(ret::int32(spawn::wait4(pid, status, options, usage)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn waitid(idtype: c_int, id: c_uint, info: Option<&mut SigInfo>, options: c_int) -> c_int {
    cancel::point(ret::zero(spawn::waitid(idtype, id, info, options)))
}}

// ------------------------------------------------------------------------------ shell (§18.7)

export! { "GLIBC_2.2.5";
pub extern "C" fn system(command: *const c_char) -> c_int {
    cancel::point(ret::int32(spawn::system(conv::addr(command))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn popen(command: *const c_char, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: mode is a NUL-terminated string (§18.7).
    ret::null(spawn::popen(conv::addr(command), unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
#[allow(non_snake_case)] // the C name
pub extern "C" fn _IO_popen(command: *const c_char, mode: *const c_char) -> *mut FILE {
    popen(command, mode)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pclose(stream: *mut FILE) -> c_int {
    ret::int32(spawn::pclose(conv::addr(stream)))
}}

// ------------------------------------------------------------------------------ groups, sessions (§18.8)

export! { "GLIBC_2.2.5";
pub extern "C" fn getpgid(pid: pid_t) -> pid_t {
    ret::int32(CSys.getpgid(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __getpgid(pid: pid_t) -> pid_t {
    ret::int32(CSys.getpgid(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __bsd_getpgrp(pid: pid_t) -> pid_t {
    ret::int32(CSys.getpgid(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpgrp() -> pid_t {
    CSys.getpgrp()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setpgid(pid: pid_t, pgid: pid_t) -> c_int {
    ret::zero(CSys.setpgid(pid, pgid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __setpgid(pid: pid_t, pgid: pid_t) -> c_int {
    ret::zero(CSys.setpgid(pid, pgid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setpgrp() -> c_int {
    ret::zero(CSys.setpgid(0, 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getsid(pid: pid_t) -> pid_t {
    ret::int32(CSys.getsid(pid))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setsid() -> pid_t {
    ret::int32(CSys.setsid())
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn daemon(nochdir: c_int, noclose: c_int) -> c_int {
    ret::zero(spawn::daemon(conv::flag(nochdir), conv::flag(noclose)))
}}

// ------------------------------------------------------------------------------ attributes, namespaces (§18.9)

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_prctl5(option: c_int, a2: c_ulong, a3: c_ulong, a4: c_ulong, a5: c_ulong) -> c_int {
    // The result is an `int` (prctl(2)): a wider value is truncated as the C prototype does.
    ret::signed(raw::prctl(option, [a2, a3, a4, a5]).map(|v| i64::from(i32::from_ne_bytes(u32::try_from(v & 0xffff_ffff).unwrap_or(0).to_ne_bytes()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn personality(persona: c_ulong) -> c_int {
    // The previous persona, as the `int` the prototype returns.
    ret::signed(CSys.personality(u32::try_from(persona & 0xffff_ffff).unwrap_or(u32::MAX)).map(|p| i64::from(i32::from_ne_bytes(p.to_ne_bytes()))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn unshare(flags: c_int) -> c_int {
    ret::zero(crate::nscache::unshare(u64::from(conv::bits(flags))))
}}

export! { "GLIBC_2.14";
pub extern "C" fn setns(fd: c_int, nstype: c_int) -> c_int {
    ret::zero(crate::nscache::setns(Fd::new(fd), conv::bits(nstype)))
}}

export! { "GLIBC_2.2.5";
// ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8; A-5 reads the aarch64 export list)
pub extern "C" fn arch_prctl(code: c_int, addr: c_ulong) -> c_int {
    ret::int(raw::arch_prctl(code, addr))
}}

export! { "GLIBC_2.2.5";
// ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8; A-5 reads the aarch64 export list)
pub extern "C" fn __arch_prctl(code: c_int, addr: c_ulong) -> c_int {
    ret::int(raw::arch_prctl(code, addr))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_ptrace4(request: c_int, pid: pid_t, addr: c_ulong, data: c_ulong) -> c_long {
    match request {
        // PTRACE_PEEKTEXT, PTRACE_PEEKDATA, PTRACE_PEEKUSER: the word read is the result, and
        // errno is left alone on success (§18.9).
        1..=3 => ret::long(raw::ptrace_peek(i64::from(request), i64::from(pid), addr)),
        _ => ret::long(raw::ptrace(i64::from(request), i64::from(pid), addr, data)),
    }
}}
