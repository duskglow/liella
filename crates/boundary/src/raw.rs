//! Kernel operations on addresses that safe code does not own (decision D-X.9).
//!
//! These are the operations `liella_kernel::Sys` deliberately leaves out: forking without
//! the runtime's cooperation, installing a handler function, exiting one thread. Their
//! soundness depends on a contract no type can express, so the module is **crate-private**:
//! only the POSIX layer (on behalf of a C caller, whose contract it is) and the fixed
//! sequences in `selftest` reach it. Safe library code cannot.

#![allow(dead_code)] // most callers arrive with the POSIX exports (phases 5, 18, 20)
#![allow(clippy::as_conversions)] // register-width reinterpretation is the kernel ABI (⚑ KERN)

use core::ptr;

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::uapi::{CloneArgs, KSigAction, SigSet, CLONE_PIDFD, SA_RESTORER, SIGCHLD};

use crate::ffi;
use crate::ffi_sys::*;

/// A handler as the kernel calls it with `SA_SIGINFO` clear.
pub(crate) type Handler = extern "C" fn(i32);

/// `rt_sigaction` installing `handler` with the boundary's restorer; returns the previous
/// action. Contract: `handler` is async-signal-safe for the state it touches.
pub(crate) fn install_handler(sig: i32, handler: Handler, flags: u64, mask: SigSet) -> KResult<KSigAction> {
    let new = KSigAction { handler: handler as usize, flags: flags | SA_RESTORER, restorer: crate::sys::restorer_addr(), mask };
    restore_action(sig, &new)
}

/// `rt_sigaction` with a complete action (for example one saved earlier); returns the
/// previous action. Contract: `action.handler` is `SIG_DFL`, `SIG_IGN` or a valid handler.
pub(crate) fn restore_action(sig: i32, action: &KSigAction) -> KResult<KSigAction> {
    let mut old = KSigAction::default();
    let rc = liella_sys_rt_sigaction(i64::from(sig), ptr::from_ref(action).cast(), ptr::from_mut(&mut old).cast(), 8);
    Errno::decode(rc).map(|_| old)
}

/// `clone(SIGCHLD)`: fork without `atfork` handlers. Returns the child's PID in the parent
/// and 0 in the child. Contract: the child is single-threaded and must not take locks
/// another thread may have held (no allocation), and must end with `exit_group`.
pub(crate) fn fork() -> KResult<i32> {
    let rc = liella_sys_clone(i64::from(SIGCHLD), 0, ptr::null_mut(), ptr::null_mut(), 0);
    Errno::decode(rc).map(|v| v as i32)
}

/// `clone3(CLONE_PIDFD, exit_signal = SIGCHLD)`: like [`fork`], also returning a pidfd in
/// the parent (`(pid, pidfd)`; `(0, -1)` in the child). Same contract.
pub(crate) fn fork_pidfd() -> KResult<(i32, i32)> {
    let mut pidfd = -1i32;
    let args = CloneArgs {
        flags: CLONE_PIDFD,
        pidfd: ptr::from_mut(&mut pidfd).expose_provenance() as u64,
        exit_signal: SIGCHLD as u64,
        ..CloneArgs::default()
    };
    let rc = liella_sys_clone3(ptr::from_ref(&args).cast_mut().cast(), size_of::<CloneArgs>() as i64);
    Errno::decode(rc).map(|v| (v as i32, if v == 0 { -1 } else { pidfd }))
}

/// The two sides of [`spawn_clone`].
pub(crate) enum Spawned {
    /// In the parent: the child's PID and, when asked for, its pidfd (-1 otherwise).
    Parent(i32, i32),
    /// In the child; `true` when its caught signals are already at their default action.
    Child(bool),
}

/// Create a spawn child (spec §18.4, D-18.3): `clone3(CLONE_VFORK | CLONE_CLEAR_SIGHAND)` with
/// `SIGCHLD` as exit signal, `CLONE_PIDFD` with `pidfd`, `CLONE_INTO_CGROUP` with `cgroup`.
/// The child has its own copy-on-write memory (no `CLONE_VM`; the copy-on-write fallback of D-18.3); the caller
/// is suspended until the child executes a program or exits. Where `clone3` is refused
/// (`ENOSYS`, as some sandboxes do), `clone(CLONE_VFORK)`, which cannot place the child in a
/// cgroup (the `ENOSYS` stands then). Contract: as for [`fork`]; the child ends with `execve`
/// or `exit_group`.
pub(crate) fn spawn_clone(pidfd: bool, cgroup: Option<i32>) -> KResult<Spawned> {
    use liella_kernel::uapi::{CLONE_CLEAR_SIGHAND, CLONE_INTO_CGROUP, CLONE_VFORK};
    let mut fd = -1i32;
    let fd_addr = ptr::from_mut(&mut fd).expose_provenance() as u64;
    let mut flags = CLONE_VFORK | CLONE_CLEAR_SIGHAND;
    if pidfd {
        flags |= CLONE_PIDFD;
    }
    if cgroup.is_some() {
        flags |= CLONE_INTO_CGROUP;
    }
    let args = CloneArgs {
        flags,
        pidfd: if pidfd { fd_addr } else { 0 },
        exit_signal: SIGCHLD as u64,
        cgroup: cgroup.map_or(0, |c| u64::from(c as u32)),
        ..CloneArgs::default()
    };
    let rc = liella_sys_clone3(ptr::from_ref(&args).cast_mut().cast(), size_of::<CloneArgs>() as i64);
    let (v, cleared) = match Errno::decode(rc) {
        Err(Errno::ENOSYS) if cgroup.is_none() => {
            let flags = (CLONE_VFORK | if pidfd { CLONE_PIDFD } else { 0 }) as i64 | i64::from(SIGCHLD);
            let ptid = if pidfd { fd_addr as *mut core::ffi::c_void } else { ptr::null_mut() };
            (Errno::decode(liella_sys_clone(flags, 0, ptid, ptr::null_mut(), 0))?, false)
        }
        r => (r?, true),
    };
    Ok(if v == 0 { Spawned::Child(cleared) } else { Spawned::Parent(v as i32, fd) })
}

/// Create a spawn child that shares the caller's memory (spec §18.4, D-18.3):
/// `clone3(CLONE_VM | CLONE_VFORK | CLONE_CLEAR_SIGHAND)` with `SIGCHLD` as exit signal,
/// `CLONE_PIDFD` with `pidfd`, `CLONE_INTO_CGROUP` with `cgroup`. The child runs `entry(arg)`
/// on the stack `stack..stack + size` and never returns to this call; the caller is suspended
/// until the child executes a program or exits, and gets the child's PID and its pidfd (-1
/// when not asked for). Contract: `stack + size` is 16-aligned, the stack is writable, private
/// to the child and not unmapped before the caller resumes; `arg` outlives the child; `entry`
/// never returns and touches nothing the suspended caller relies on (see `spawn`). `ENOSYS`
/// (a sandbox that refuses `clone3`) is the caller's cue to use [`spawn_clone`].
pub(crate) fn spawn_clone_vm(pidfd: bool, cgroup: Option<i32>, stack: usize, size: usize, entry: ffi::SpawnEntry, arg: usize) -> KResult<(i32, i32)> {
    use liella_kernel::uapi::{CLONE_CLEAR_SIGHAND, CLONE_INTO_CGROUP, CLONE_VFORK, CLONE_VM};
    let mut fd = -1i32;
    let fd_addr = ptr::from_mut(&mut fd).expose_provenance() as u64;
    let mut flags = CLONE_VM | CLONE_VFORK | CLONE_CLEAR_SIGHAND;
    if pidfd {
        flags |= CLONE_PIDFD;
    }
    if cgroup.is_some() {
        flags |= CLONE_INTO_CGROUP;
    }
    let args = CloneArgs {
        flags,
        pidfd: if pidfd { fd_addr } else { 0 },
        exit_signal: SIGCHLD as u64,
        stack: stack as u64,
        stack_size: size as u64,
        cgroup: cgroup.map_or(0, |c| u64::from(c as u32)),
        ..CloneArgs::default()
    };
    let rc = ffi::liella_machine_clone3_run(ptr::from_ref(&args).cast(), size_of::<CloneArgs>() as i64, entry, arg);
    Errno::decode(rc).map(|pid| (pid as i32, fd))
}

/// `execve(path, argv, envp)` with a C caller's argument and environment arrays; returns only
/// on failure, with the error. Contract: `argv` and `envp` are NULL or NULL-terminated arrays
/// of C strings (the exec and spawn contracts, spec §18.3–18.4), which the kernel reads.
pub(crate) fn execve(path: liella_kernel::path::KStr<'_>, argv: usize, envp: usize) -> Errno {
    let rc = liella_sys_execve(path.as_ptr().cast(), ptr::with_exposed_provenance(argv), ptr::with_exposed_provenance(envp));
    Errno::decode(rc).err().unwrap_or(Errno::EINVAL)
}

/// `execveat(dirfd, path, argv, envp, flags)`: as [`execve`].
pub(crate) fn execveat(dirfd: i32, path: liella_kernel::path::KStr<'_>, argv: usize, envp: usize, flags: i32) -> Errno {
    let rc = liella_sys_execveat(i64::from(dirfd), path.as_ptr().cast(), ptr::with_exposed_provenance(argv), ptr::with_exposed_provenance(envp), i64::from(flags));
    Errno::decode(rc).err().unwrap_or(Errno::EINVAL)
}

/// `prctl(option, a2, a3, a4, a5)` for a C caller (spec §18.9): the arguments are integers or
/// the caller's addresses, as the option says, and pass as given.
pub(crate) fn prctl(option: i32, args: [u64; 4]) -> KResult<i64> {
    let [a, b, c, d] = args.map(|v| v as i64);
    Errno::decode(liella_sys_prctl(i64::from(option), a, b, c, d)).map(|v| v as i64)
}

/// `ptrace(request, pid, addr, data)` for a C caller (spec §18.9): passes as given (the
/// request decides whether `addr` and `data` are addresses, which the kernel may write).
pub(crate) fn ptrace(request: i64, pid: i64, addr: u64, data: u64) -> KResult<i64> {
    Errno::decode(liella_sys_ptrace(request, pid, addr as i64, data as i64)).map(|v| v as i64)
}

/// A `PTRACE_PEEK*` request: the kernel stores the word it read in the tracee at a local,
/// which is the result.
pub(crate) fn ptrace_peek(request: i64, pid: i64, addr: u64) -> KResult<i64> {
    let mut word = 0i64;
    let at = ptr::from_mut(&mut word).expose_provenance() as i64;
    Errno::decode(liella_sys_ptrace(request, pid, addr as i64, at)).map(|_| word)
}

/// `arch_prctl(code, addr)` for a C caller (spec §18.9): `addr` is an integer or the caller's
/// address, as `code` says. Contract: a C caller that moves the FS base owns the consequences
/// (the thread pointer names the library's thread block, D-22.9).
// ARCH(x86_64): the arch_prctl system call, x86_64 only (A-3)
pub(crate) fn arch_prctl(code: i32, addr: u64) -> KResult<i64> {
    Errno::decode(liella_sys_arch_prctl(i64::from(code), addr as i64)).map(|v| v as i64)
}

/// `exit`: end the calling thread only. Contract: no other thread holds references into this
/// thread's stack or thread-local storage.
pub(crate) fn exit_thread(status: i32) -> ! {
    loop {
        let _ = liella_sys_exit(i64::from(status));
    }
}

/// `brk`: move the program break; returns the new break (the old one if the request failed,
/// ⚑ KERN). Contract: nothing Rust references lies in the range being released.
pub(crate) fn brk(addr: usize) -> usize {
    liella_sys_brk(addr as i64) as usize
}

/// `munmap` of `[addr, addr + len)`. Contract: the range is a mapping the caller made (the
/// heap's own segments and large blocks) and nothing Rust references lies in it.
pub(crate) fn munmap(addr: usize, len: usize) -> KResult<()> {
    Errno::decode(liella_sys_munmap(addr as i64, len as i64)).map(|_| ())
}

/// `madvise(MADV_DONTNEED)`: the pages of the range read as zero afterwards. Contract: as for
/// [`munmap`]; the contents are no longer needed.
pub(crate) fn madvise_dontneed(addr: usize, len: usize) -> KResult<()> {
    Errno::decode(liella_sys_madvise(addr as i64, len as i64, i64::from(liella_kernel::uapi::MADV_DONTNEED))).map(|_| ())
}

/// `madvise(MADV_DONTDUMP)`: the pages of the range are left out of core dumps (a secure heap
/// block, D-X.11). Contract: as for [`munmap`]; the contents are unchanged.
pub(crate) fn madvise_dontdump(addr: usize, len: usize) -> KResult<()> {
    Errno::decode(liella_sys_madvise(addr as i64, len as i64, i64::from(liella_kernel::uapi::MADV_DONTDUMP))).map(|_| ())
}

/// `mremap` with `MREMAP_MAYMOVE`; returns the (possibly new) address. Contract: as for
/// [`munmap`]; references into the old range are not used afterwards.
pub(crate) fn mremap(addr: usize, old_len: usize, new_len: usize) -> KResult<usize> {
    Errno::decode(liella_sys_mremap(addr as i64, old_len as i64, new_len as i64, i64::from(liella_kernel::uapi::MREMAP_MAYMOVE), 0)).map(|v| v as usize)
}

// ---- Memory operations on behalf of a C caller (spec §11.10). The C contract makes the caller
// answerable for the range: it is memory the caller mapped (or, for `MAP_FIXED`, chose to
// replace), never memory Rust references. These are called only from `crate::posix`.

/// `mmap(addr, len, prot, flags, fd, off)`: the new mapping's address.
pub(crate) fn mmap(addr: usize, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> KResult<usize> {
    Errno::decode(liella_sys_mmap(addr as i64, len as i64, i64::from(prot), i64::from(flags), i64::from(fd), off)).map(|v| v as usize)
}

/// `mprotect(addr, len, prot)`.
pub(crate) fn mprotect(addr: usize, len: usize, prot: i32) -> KResult<()> {
    Errno::decode(liella_sys_mprotect(addr as i64, len as i64, i64::from(prot))).map(|_| ())
}

/// `pkey_mprotect(addr, len, prot, key)`.
pub(crate) fn pkey_mprotect(addr: usize, len: usize, prot: i32, key: i32) -> KResult<()> {
    Errno::decode(liella_sys_pkey_mprotect(addr as i64, len as i64, i64::from(prot), i64::from(key))).map(|_| ())
}

/// `madvise(addr, len, advice)`, any advice (the C caller owns the consequences).
pub(crate) fn madvise(addr: usize, len: usize, advice: i32) -> KResult<()> {
    Errno::decode(liella_sys_madvise(addr as i64, len as i64, i64::from(advice))).map(|_| ())
}

/// `msync(addr, len, flags)`.
pub(crate) fn msync(addr: usize, len: usize, flags: i32) -> KResult<()> {
    Errno::decode(liella_sys_msync(addr as i64, len as i64, i64::from(flags))).map(|_| ())
}

/// `mincore(addr, len, vec)`: `vec` receives one byte per page of the range.
pub(crate) fn mincore(addr: usize, len: usize, vec: &mut [u8]) -> KResult<()> {
    if vec.len() < len.div_ceil(4096) {
        return Err(Errno::EFAULT);
    }
    Errno::decode(liella_sys_mincore(addr as i64, len as i64, vec.as_mut_ptr().cast())).map(|_| ())
}

/// `mlock(addr, len)`.
pub(crate) fn mlock(addr: usize, len: usize) -> KResult<()> {
    Errno::decode(liella_sys_mlock(addr as i64, len as i64)).map(|_| ())
}

/// `mlock2(addr, len, flags)`.
pub(crate) fn mlock2(addr: usize, len: usize, flags: u32) -> KResult<()> {
    Errno::decode(liella_sys_mlock2(addr as i64, len as i64, i64::from(flags))).map(|_| ())
}

/// `munlock(addr, len)`.
pub(crate) fn munlock(addr: usize, len: usize) -> KResult<()> {
    Errno::decode(liella_sys_munlock(addr as i64, len as i64)).map(|_| ())
}

/// `remap_file_pages(addr, size, prot, pgoff, flags)`.
pub(crate) fn remap_file_pages(addr: usize, size: usize, prot: i32, pgoff: usize, flags: i32) -> KResult<()> {
    Errno::decode(liella_sys_remap_file_pages(addr as i64, size as i64, i64::from(prot), pgoff as i64, i64::from(flags))).map(|_| ())
}

/// `mremap(old, old_len, new_len, flags, new_addr)`: the mapping's address afterwards.
pub(crate) fn mremap_c(old: usize, old_len: usize, new_len: usize, flags: i32, new_addr: usize) -> KResult<usize> {
    Errno::decode(liella_sys_mremap(old as i64, old_len as i64, new_len as i64, i64::from(flags), new_addr as i64)).map(|v| v as usize)
}

/// `name_to_handle_at(dirfd, path, handle, mount_id, flags)`: the caller's `struct file_handle`
/// is variable-length (its `handle_bytes` gives the room), so it passes as given.
pub(crate) fn name_to_handle_at(dirfd: i32, path: liella_kernel::path::KStr<'_>, handle: usize, mount_id: usize, flags: i32) -> KResult<()> {
    let rc = liella_sys_name_to_handle_at(i64::from(dirfd), path.as_ptr().cast(), ptr::with_exposed_provenance_mut(handle), ptr::with_exposed_provenance_mut(mount_id), i64::from(flags));
    Errno::decode(rc).map(|_| ())
}

/// `open_by_handle_at(mount_fd, handle, flags)`, with the caller's `struct file_handle`.
pub(crate) fn open_by_handle_at(mount_fd: i32, handle: usize, flags: i32) -> KResult<liella_kernel::fd::Fd> {
    let rc = liella_sys_open_by_handle_at(i64::from(mount_fd), ptr::with_exposed_provenance(handle), i64::from(flags));
    Errno::decode(rc).map(|v| liella_kernel::fd::Fd::new(v as i32))
}

/// `ioctl(fd, request, arg)` for a C caller: the request space is unstructured and the
/// argument's meaning is the request's (§11.6), so it passes as given.
pub(crate) fn ioctl(fd: i32, request: u64, arg: usize) -> KResult<i64> {
    Errno::decode(liella_sys_ioctl(i64::from(fd), request as i64, arg as i64)).map(|v| v as i64)
}

/// `arch_prctl(ARCH_SET_FS, tp)`: make `tp` this thread's thread pointer. Contract: `tp` is the
/// address of a thread block that lives as long as the thread (`crate::runtime`).
pub(crate) fn set_fs(tp: usize) -> KResult<()> {
    // ARCH(x86_64): sets the thread pointer with ARCH_SET_FS; aarch64 writes TPIDR_EL0 (A-4)
    Errno::decode(liella_sys_arch_prctl(i64::from(liella_kernel::uapi::ARCH_SET_FS), tp as i64)).map(|_| ())
}

/// The NUMA nodes `mbind` can name: a 1024-bit mask (the x86-64 `MAX_NUMNODES` of
/// distribution kernels).
const NODE_MASK_WORDS: usize = 16;

/// `mbind(MPOL_PREFERRED)`: prefer node `node` for the pages of `[addr, addr + len)`, faulted
/// in from now on (D-3.10). A preference, not a binding: the kernel falls back to other nodes
/// when `node` is full. `ENOSYS` without `CONFIG_NUMA`. Contract: the range is a mapping the
/// caller owns.
pub(crate) fn mbind_preferred(addr: usize, len: usize, node: u32) -> KResult<()> {
    let mut mask = [0u64; NODE_MASK_WORDS];
    let word = mask.get_mut(node as usize / 64).ok_or(Errno::EINVAL)?;
    *word = 1u64 << (node % 64);
    let maxnode = (NODE_MASK_WORDS * 64 + 1) as i64;
    let rc = liella_sys_mbind(addr as i64, len as i64, i64::from(liella_kernel::uapi::MPOL_PREFERRED), mask.as_ptr().cast(), maxnode, 0);
    Errno::decode(rc).map(|_| ())
}

/// The NUMA node holding the page at `addr` (`get_mempolicy(MPOL_F_NODE | MPOL_F_ADDR)`); the
/// page is faulted in if it was not. Contract: `addr` is mapped.
pub(crate) fn node_of(addr: usize) -> KResult<u32> {
    let mut node: i32 = -1;
    let flags = liella_kernel::uapi::MPOL_F_NODE | liella_kernel::uapi::MPOL_F_ADDR;
    let rc = liella_sys_get_mempolicy(ptr::from_mut(&mut node).cast(), ptr::null_mut(), 0, addr as i64, i64::from(flags));
    Errno::decode(rc).and_then(|_| u32::try_from(node).map_err(|_| Errno::EINVAL))
}

/// The memory policy in force at `addr` (`get_mempolicy(MPOL_F_ADDR)`): the mode and the first
/// word of the node mask. Contract: `addr` is mapped.
pub(crate) fn policy_of(addr: usize) -> KResult<(i32, u64)> {
    let mut mode: i32 = -1;
    let mut mask = [0u64; NODE_MASK_WORDS];
    let maxnode = (NODE_MASK_WORDS * 64 + 1) as i64;
    let rc = liella_sys_get_mempolicy(ptr::from_mut(&mut mode).cast(), mask.as_mut_ptr().cast(), maxnode, addr as i64, i64::from(liella_kernel::uapi::MPOL_F_ADDR));
    Errno::decode(rc).map(|_| (mode, mask[0]))
}

/// `set_robust_list`: register this thread's robust-futex list head. Contract: `head` is the
/// address of a `struct robust_list_head` that lives until the thread exits or re-registers,
/// and every entry it links names a futex word the kernel may write at exit.
pub(crate) fn set_robust_list(head: usize, len: usize) -> KResult<()> {
    Errno::decode(liella_sys_set_robust_list(ptr::with_exposed_provenance_mut(head), len as i64)).map(|_| ())
}

/// `process_vm_writev`: copy local buffers into another process. Contract: the remote ranges
/// are not memory this process's Rust code references (a target sharing this address space
/// would make this an arbitrary write).
pub(crate) fn process_vm_writev(pid: i32, local: &[liella_kernel::mem::IoVec<'_>], remote: &[liella_kernel::mem::RemoteIoVec], flags: u32) -> KResult<usize> {
    let rc = liella_sys_process_vm_writev(i64::from(pid), local.as_ptr().cast(), local.len() as i64, remote.as_ptr().cast(), remote.len() as i64, i64::from(flags));
    Errno::decode(rc).map(|v| v as usize)
}

/// `sigaltstack` installing `[addr, addr + size)`; returns the previous stack. Contract: the
/// range is memory the kernel may write at any signal delivery, referenced by nothing else,
/// until the stack is disabled.
pub(crate) fn sigaltstack_set(addr: usize, size: usize) -> KResult<liella_kernel::uapi::StackT> {
    let mut new = liella_kernel::uapi::StackT::default();
    new.sp = addr;
    new.size = size;
    let mut old = liella_kernel::uapi::StackT::default();
    let rc = liella_sys_sigaltstack(ptr::from_ref(&new).cast(), ptr::from_mut(&mut old).cast());
    Errno::decode(rc).map(|_| old)
}

/// `shmat(id, addr, flags)`: the attached segment's address. The caller names the address
/// (`SHM_RND`, `SHM_REMAP`) and owns the attachment, which `Sys::shmat`'s `Region` cannot
/// express (D-11.7, as `mmap`).
pub(crate) fn shmat(id: i32, addr: usize, flags: i32) -> KResult<usize> {
    Errno::decode(liella_sys_shmat(i64::from(id), addr as i64, i64::from(flags))).map(|v| v as usize)
}

/// `shmdt(addr)`: detaches the caller's attachment at `addr`.
pub(crate) fn shmdt(addr: usize) -> KResult<()> {
    Errno::decode(liella_sys_shmdt(addr as i64)).map(|_| ())
}

/// `ustat(dev)` (compat, §13.10): the kernel's 32-byte `struct ustat` for the file system on
/// device `dev` (the kernel's 32-bit encoding).
pub(crate) fn ustat(dev: u32) -> KResult<[u8; 32]> {
    let mut buf = [0u8; 32];
    Errno::decode(liella_sys_ustat(i64::from(dev), buf.as_mut_ptr().cast())).map(|_| buf)
}

/// `setsockopt(fd, level, name, value, len)` for a level whose options may register the
/// caller's memory with the kernel for later writes (`SOL_XDP`, `SOL_RDS`, ...; §15.1), which
/// `Sys::setsockopt` does not type: the caller's address passes as given.
pub(crate) fn setsockopt(fd: i32, level: i32, name: i32, value: usize, len: u32) -> KResult<()> {
    let rc = liella_sys_setsockopt(i64::from(fd), i64::from(level), i64::from(name), ptr::with_exposed_provenance(value), i64::from(len));
    Errno::decode(rc).map(|_| ())
}

/// `ioctl(fd, SIOCATMARK, &v)` (§15.1 `sockatmark`): 1 at the out-of-band mark, else 0.
pub(crate) fn sockatmark(fd: i32) -> KResult<i64> {
    let mut v: i32 = 0;
    let rc = liella_sys_ioctl(i64::from(fd), 0x8905, ptr::from_mut(&mut v).expose_provenance() as i64);
    Errno::decode(rc).map(|_| i64::from(v))
}

/// `syscall(number, ...)` (§24.6, D-24.5): the caller names the call and passes six
/// register-sized arguments as given (addresses included), as glibc's `syscall` does.
pub(crate) fn syscall(nr: i64, args: [i64; 6]) -> KResult<i64> {
    let [a, b, c, d, e, f] = args;
    Errno::decode(liella_sys_syscall(nr, a, b, c, d, e, f)).map(|v| v as i64)
}

/// `rt_sigaction(sig, new, &old)` for a C caller's action (§20.2): `new` names the caller's
/// handler and the library's restorer (`signal_compat` builds it); `None` only reads.
pub(crate) fn sigaction(sig: i32, new: Option<&KSigAction>) -> KResult<KSigAction> {
    let mut old = KSigAction::default();
    let p = new.map_or(ptr::null(), |n| ptr::from_ref(n).cast());
    let rc = liella_sys_rt_sigaction(i64::from(sig), p, ptr::from_mut(&mut old).cast(), 8);
    Errno::decode(rc).map(|_| old)
}

/// `sigaltstack(ss, &old)` for a C caller's stack (§20.7): `ss` names memory the caller owns
/// and the kernel may write at any signal delivery; `None` only reads.
pub(crate) fn sigaltstack(ss: Option<&liella_kernel::uapi::StackT>) -> KResult<liella_kernel::uapi::StackT> {
    let mut old = liella_kernel::uapi::StackT::default();
    let p = ss.map_or(ptr::null(), |s| ptr::from_ref(s).cast());
    let rc = liella_sys_sigaltstack(p, ptr::from_mut(&mut old).cast());
    Errno::decode(rc).map(|_| old)
}

/// `rt_sigaction` installing the three-argument handler at `handler` (`SA_SIGINFO`) with the
/// library's restorer. Contract: `handler` is an `extern "C" fn(i32, *mut SigInfo, *mut c_void)`
/// that is async-signal-safe for what it touches (the library's internal signal, D-20.1).
pub(crate) fn install_action(sig: i32, handler: usize, flags: u64, mask: SigSet) -> KResult<KSigAction> {
    let new = KSigAction { handler, flags: flags | SA_RESTORER | liella_kernel::uapi::SA_SIGINFO, restorer: crate::sys::restorer_addr(), mask };
    restore_action(sig, &new)
}

// ---- POSIX asynchronous I/O (spec §11.11, D-11.8). The transfers name the buffer of a C
// caller's `aiocb` (`aio_buf`, `aio_nbytes`), which the AIO contract keeps valid and untouched
// until the request completes; these are called only from `crate::aio`.

/// `pread64(fd, buf, n, off)` into an AIO request's buffer. Contract: `buf` names `n` bytes the
/// caller handed over for the request (the kernel checks the range itself: `EFAULT`).
pub(crate) fn aio_pread(fd: i32, buf: usize, n: usize, off: i64) -> KResult<usize> {
    Errno::decode(liella_sys_pread64(i64::from(fd), ptr::with_exposed_provenance_mut(buf), n as i64, off)).map(|v| v as usize)
}

/// `pwrite64(fd, buf, n, off)` from an AIO request's buffer. Contract: as [`aio_pread`].
pub(crate) fn aio_pwrite(fd: i32, buf: usize, n: usize, off: i64) -> KResult<usize> {
    Errno::decode(liella_sys_pwrite64(i64::from(fd), ptr::with_exposed_provenance(buf), n as i64, off)).map(|v| v as usize)
}

/// `read(fd, buf, n)` into an AIO request's buffer (a descriptor without positions: the offset is
/// ignored). Contract: as [`aio_pread`].
pub(crate) fn aio_read(fd: i32, buf: usize, n: usize) -> KResult<usize> {
    Errno::decode(liella_sys_read(i64::from(fd), ptr::with_exposed_provenance_mut(buf), n as i64)).map(|v| v as usize)
}

/// `write(fd, buf, n)` from an AIO request's buffer. Contract: as [`aio_pread`].
pub(crate) fn aio_write(fd: i32, buf: usize, n: usize) -> KResult<usize> {
    Errno::decode(liella_sys_write(i64::from(fd), ptr::with_exposed_provenance(buf), n as i64)).map(|v| v as usize)
}

// The io_uring system calls (io_uring_setup(2), io_uring_enter(2), io_uring_register(2)), through
// their C-layer wrappers (D-11.11).

/// `io_uring_setup(entries, params)`: the new instance's descriptor. `params` is the 120-byte
/// `struct io_uring_params` as 30 words, which the kernel reads and fills in.
pub(crate) fn io_uring_setup(entries: u32, params: &mut [u32; 30]) -> KResult<i32> {
    let rc = liella_sys_io_uring_setup(i64::from(entries), ptr::from_mut(params).cast());
    Errno::decode(rc).map(|v| v as i32)
}

/// `io_uring_enter(fd, to_submit, min_complete, flags, NULL, 0)`: submit the entries the
/// submission ring holds and/or wait for completions. Contract: every submission entry in the
/// ring names only memory the operation may use until it completes: an AIO request's buffer
/// (the C caller's, kept valid by the AIO contract) or none (`crate::aio` writes the entries).
pub(crate) fn io_uring_enter(fd: i32, to_submit: u32, min_complete: u32, flags: u32) -> KResult<u32> {
    let rc = liella_sys_io_uring_enter(i64::from(fd), i64::from(to_submit), i64::from(min_complete), i64::from(flags), ptr::null(), 0);
    Errno::decode(rc).map(|v| v as u32)
}

/// `io_uring_register(fd, IORING_REGISTER_PROBE, buf, nr)`: the kernel fills `buf` with a
/// `struct io_uring_probe` of `nr` operation entries (16 bytes, then 8 per entry); `EINVAL`
/// without room for them.
pub(crate) fn io_uring_probe(fd: i32, buf: &mut [u8], nr: u32) -> KResult<()> {
    if buf.len() < (nr as usize).saturating_mul(8).saturating_add(16) {
        return Err(Errno::EINVAL);
    }
    Errno::decode(liella_sys_io_uring_register(i64::from(fd), 8, buf.as_mut_ptr().cast(), i64::from(nr))).map(|_| ())
}
