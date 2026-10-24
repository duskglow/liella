//! The `Sys` trait: every system call of spec Appendix A.1 as a typed, safe method.
//!
//! * Arguments are `Fd`, `KStr`, integers, slices, uapi structures and the ownership types of
//!   [`crate::mem`]. Results are `KResult<T>`.
//! * **Every method is sound for every argument safe Rust can build** (decision D-X.9). Calls
//!   whose argument is an address the kernel will later write or jump to are not here; they
//!   are in `liella_boundary::raw` for C callers: `brk`, `munmap`/`mprotect`/`mremap` of
//!   arbitrary ranges, `MAP_FIXED`, `clone`/`fork`, `exit` of one thread, handler
//!   installation, `sigaltstack` set, `set_robust_list`, `arch_prctl(ARCH_SET_FS)`,
//!   `process_vm_writev`, `modify_ldt` writes, generic `ioctl`/`prctl`/`fcntl`/`ptrace`.
//! * Production implementation: `liella_boundary::sys::CSys`. Tests: [`crate::testing::Faulty`]
//!   wraps any `Sys` and injects errors.
//!
//! The method list is written once, in [`sys_api!`], which generates both the trait and the
//! `Faulty` wrapper, so the two cannot drift.

use core::sync::atomic::AtomicU32;

use crate::errno::KResult;
use crate::fd::Fd;
use crate::mem::{IoVec, IoVecMut, KPtrArray, MMsgHdr, MMsgHdrMut, MsgHdr, MsgHdrMut, Region, RemoteIoVec};
use crate::ops::*;
use crate::path::KStr;
use crate::uapi::*;

/// The fourth `futex` argument: a timeout, a second value, or nothing (it depends on the
/// operation; the kernel reads a timeout, never writes one).
#[derive(Clone, Copy, Debug)]
pub enum FutexArg<'a> {
    /// No timeout / unused.
    None,
    /// A relative or absolute timeout.
    Timeout(&'a Timespec),
    /// `val2` (requeue count, `FUTEX_WAKE_OP` count).
    Val2(u32),
}

/// The value argument of `fsconfig`.
#[derive(Clone, Copy, Debug)]
pub enum FsValue<'a> {
    /// No value.
    None,
    /// A string (`FSCONFIG_SET_STRING`, `FSCONFIG_SET_PATH`).
    Str(KStr<'a>),
    /// A binary blob (`FSCONFIG_SET_BINARY`; `aux` is its length).
    Binary(&'a [u8]),
}

/// A signal disposition `Sys` may install: only the kernel's own. Handler functions are
/// installed by the boundary (`raw`), which owns the restorer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// `SIG_DFL`.
    Default,
    /// `SIG_IGN`.
    Ignore,
}

/// Generate the `Sys` trait and its fault-injecting wrapper from one list.
///
/// Two sections: `fallible` methods return `KResult<T>` and can have an error injected;
/// `infallible` methods pass straight through.
macro_rules! sys_api {
    (
        fallible { $( $(#[$fm:meta])* fn $fname:ident $(<$($flt:lifetime),*>)? (&self $(, $fa:ident : $ft:ty)* $(,)?) -> KResult<$fr:ty>; )* }
        infallible { $( $(#[$im:meta])* fn $iname:ident (&self $(, $ia:ident : $it:ty)* $(,)?) -> $ir:ty; )* }
    ) => {
        /// The kernel, as a value (rule X5). See the module documentation.
        pub trait Sys {
            $( $(#[$fm])* fn $fname $(<$($flt),*>)? (&self $(, $fa: $ft)*) -> KResult<$fr>; )*
            $( $(#[$im])* fn $iname (&self $(, $ia: $it)*) -> $ir; )*
            /// `exit_group`: end the process with `status & 0xff`. Never returns.
            fn exit_group(&self, status: i32) -> !;
            /// View a region as bytes. `None` unless [`Region::is_plain_memory`]: other kinds can
            /// change or fault underneath a reference. Not a system call.
            fn region_bytes<'a>(&self, region: &'a mut Region) -> Option<&'a mut [u8]>;
            /// View a region as 32-bit atomic words: memory other processes may change, so
            /// only atomic access is sound (futex words, process-shared locks). `None` unless
            /// the region is anonymous or System V shared memory, readable and writable, with the
            /// default protection key. Not a system call.
            fn region_words<'a>(&self, region: &'a Region) -> Option<&'a [AtomicU32]>;
        }

        /// The names of all fallible methods, for [`crate::testing::Faulty`] plans.
        pub const FALLIBLE: &[&str] = &[$(stringify!($fname)),*];

        impl<S: Sys> Sys for crate::testing::Faulty<S> {
            $( fn $fname $(<$($flt),*>)? (&self $(, $fa: $ft)*) -> KResult<$fr> {
                match self.take(stringify!($fname)) {
                    Some(e) => Err(e),
                    None => self.inner().$fname($($fa),*),
                }
            } )*
            $( fn $iname (&self $(, $ia: $it)*) -> $ir { self.inner().$iname($($ia),*) } )*
            fn exit_group(&self, status: i32) -> ! { self.inner().exit_group(status) }
            fn region_bytes<'a>(&self, region: &'a mut Region) -> Option<&'a mut [u8]> { self.inner().region_bytes(region) }
            fn region_words<'a>(&self, region: &'a Region) -> Option<&'a [AtomicU32]> { self.inner().region_words(region) }
        }
    };
}

sys_api! {
fallible {
    // ---------------------------------------------------------------- process lifecycle
    /// `execve`. Returns only on failure (`Ok` is never produced).
    fn execve(&self, path: KStr<'_>, argv: &KPtrArray<'_>, envp: &KPtrArray<'_>) -> KResult<core::convert::Infallible>;
    /// `execveat`. Returns only on failure.
    fn execveat(&self, dir: Fd, path: KStr<'_>, argv: &KPtrArray<'_>, envp: &KPtrArray<'_>, flags: u32) -> KResult<core::convert::Infallible>;
    /// `wait4`: `(pid, status)`.
    fn wait4(&self, pid: i32, options: u32, usage: Option<&mut Rusage>) -> KResult<(i32, i32)>;
    /// `waitid`. `info.signo == 0` after `WNOHANG` with no child ready.
    fn waitid(&self, idtype: u32, id: i32, info: &mut SigInfo, options: u32, usage: Option<&mut Rusage>) -> KResult<()>;
    /// `pidfd_open`.
    fn pidfd_open(&self, pid: i32, flags: u32) -> KResult<Fd>;
    /// `pidfd_getfd`.
    fn pidfd_getfd(&self, pidfd: Fd, target: i32, flags: u32) -> KResult<Fd>;
    /// `pidfd_send_signal`.
    fn pidfd_send_signal(&self, pidfd: Fd, sig: i32, info: Option<&SigInfo>, flags: u32) -> KResult<()>;
    /// `set_tid_address`: the kernel clears `word` and wakes its futex when this thread exits.
    /// Returns the caller's TID.
    fn set_tid_address(&self, word: &'static AtomicU32) -> KResult<i32>;

    // ---------------------------------------------------------------- credentials
    /// `getresuid`: (real, effective, saved).
    fn getresuid(&self) -> KResult<(u32, u32, u32)>;
    /// `getresgid`.
    fn getresgid(&self) -> KResult<(u32, u32, u32)>;
    /// `setuid` (calling thread only, ⚑ KERN).
    fn setuid(&self, uid: u32) -> KResult<()>;
    /// `setgid`.
    fn setgid(&self, gid: u32) -> KResult<()>;
    /// `setreuid` (`u32::MAX` = unchanged).
    fn setreuid(&self, ruid: u32, euid: u32) -> KResult<()>;
    /// `setregid`.
    fn setregid(&self, rgid: u32, egid: u32) -> KResult<()>;
    /// `setresuid`.
    fn setresuid(&self, r: u32, e: u32, s: u32) -> KResult<()>;
    /// `setresgid`.
    fn setresgid(&self, r: u32, e: u32, s: u32) -> KResult<()>;
    /// `getgroups`: fills `list`, returns the count; an empty `list` asks for the count.
    fn getgroups(&self, list: &mut [u32]) -> KResult<usize>;
    /// `setgroups`.
    fn setgroups(&self, list: &[u32]) -> KResult<()>;
    /// `capget`. The kernel may rewrite `hdr.version` to its preferred version.
    fn capget(&self, hdr: &mut CapHeader, data: Option<&mut [CapData; 2]>) -> KResult<()>;
    /// `capset`.
    fn capset(&self, hdr: &mut CapHeader, data: &[CapData; 2]) -> KResult<()>;

    // ---------------------------------------------------------------- sessions and groups
    /// `setsid`: the new session ID.
    fn setsid(&self) -> KResult<i32>;
    /// `getsid`.
    fn getsid(&self, pid: i32) -> KResult<i32>;
    /// `setpgid`.
    fn setpgid(&self, pid: i32, pgid: i32) -> KResult<()>;
    /// `getpgid`.
    fn getpgid(&self, pid: i32) -> KResult<i32>;

    // ---------------------------------------------------------------- signals
    /// `rt_sigaction` query: the current action for `sig`.
    fn sigaction_get(&self, sig: i32) -> KResult<KSigAction>;
    /// `rt_sigaction` with `SIG_DFL` or `SIG_IGN`; returns the previous action.
    fn sigaction_set(&self, sig: i32, disposition: Disposition, flags: u64, mask: SigSet) -> KResult<KSigAction>;
    /// `rt_sigprocmask`; returns the previous mask.
    fn sigprocmask(&self, how: u32, set: Option<&SigSet>) -> KResult<SigSet>;
    /// `rt_sigsuspend`. Always fails (`EINTR` after a handler ran).
    fn sigsuspend(&self, mask: &SigSet) -> KResult<()>;
    /// `rt_sigpending`.
    fn sigpending(&self) -> KResult<SigSet>;
    /// `rt_sigtimedwait`: the accepted signal.
    fn sigtimedwait(&self, set: &SigSet, info: Option<&mut SigInfo>, timeout: Option<&Timespec>) -> KResult<i32>;
    /// `rt_sigqueueinfo`.
    fn sigqueueinfo(&self, pid: i32, sig: i32, info: &SigInfo) -> KResult<()>;
    /// `rt_tgsigqueueinfo`.
    fn tgsigqueueinfo(&self, tgid: i32, tid: i32, sig: i32, info: &SigInfo) -> KResult<()>;
    /// `kill`.
    fn kill(&self, pid: i32, sig: i32) -> KResult<()>;
    /// `tgkill`.
    fn tgkill(&self, tgid: i32, tid: i32, sig: i32) -> KResult<()>;
    /// `sigaltstack` query.
    fn sigaltstack_get(&self) -> KResult<StackT>;
    /// `sigaltstack(SS_DISABLE)`; returns the previous stack. `EPERM` while running on it.
    fn sigaltstack_disable(&self) -> KResult<StackT>;
    /// `signalfd4`: a new descriptor (`fd` = `None`) or an update of an existing one.
    fn signalfd(&self, fd: Option<Fd>, mask: &SigSet, flags: u32) -> KResult<Fd>;
    /// `pause`. Always fails (`EINTR`).
    fn pause(&self) -> KResult<()>;

    // ---------------------------------------------------------------- memory
    /// `mmap` of new memory at a kernel-chosen address (`hint` is only a hint; `MAP_FIXED` is
    /// refused with `EINVAL`). `fd` = `None` maps anonymous memory.
    fn mmap(&self, hint: usize, len: usize, prot: u32, flags: u32, fd: Option<Fd>, offset: i64) -> KResult<Region>;
    /// `munmap` of a whole region.
    fn munmap(&self, region: Region) -> KResult<()>;
    /// `mprotect` of a whole region.
    fn mprotect(&self, region: &mut Region, prot: u32) -> KResult<()>;
    /// `mremap`: grow or shrink; with `may_move` the region may be relocated.
    fn mremap(&self, region: &mut Region, new_len: usize, may_move: bool) -> KResult<()>;
    /// `madvise` on a whole region.
    fn madvise(&self, region: &mut Region, advice: Advice) -> KResult<()>;
    /// `msync`.
    fn msync(&self, region: &Region, flags: u32) -> KResult<()>;
    /// `mincore`: one byte per page into `vec` (which needs one byte per page, else `EINVAL`).
    fn mincore(&self, region: &Region, vec: &mut [u8]) -> KResult<()>;
    /// `mlock`.
    fn mlock(&self, region: &Region) -> KResult<()>;
    /// `mlock2`.
    fn mlock2(&self, region: &Region, flags: u32) -> KResult<()>;
    /// `munlock`.
    fn munlock(&self, region: &Region) -> KResult<()>;
    /// `mlockall`.
    fn mlockall(&self, flags: u32) -> KResult<()>;
    /// `munlockall`.
    fn munlockall(&self) -> KResult<()>;
    /// `remap_file_pages` (deprecated; shared file mappings only, which are never viewed as
    /// bytes).
    fn remap_file_pages(&self, region: &Region, pgoff: usize) -> KResult<()>;
    /// `memfd_create`.
    fn memfd_create(&self, name: KStr<'_>, flags: u32) -> KResult<Fd>;
    /// `pkey_alloc`.
    fn pkey_alloc(&self, flags: u32, access: u32) -> KResult<i32>;
    /// `pkey_free`.
    fn pkey_free(&self, key: i32) -> KResult<()>;
    /// `pkey_mprotect` of a whole region.
    fn pkey_mprotect(&self, region: &mut Region, prot: u32, key: i32) -> KResult<()>;
    /// `process_madvise`: advice on another process's memory; bytes advised.
    fn process_madvise(&self, pidfd: Fd, ranges: &[RemoteIoVec], advice: RemoteAdvice, flags: u32) -> KResult<usize>;
    /// `process_mrelease`.
    fn process_mrelease(&self, pidfd: Fd, flags: u32) -> KResult<()>;
    /// `process_vm_readv`: copy from another process into local buffers; bytes copied.
    fn process_vm_readv(&self, pid: i32, local: &mut [IoVecMut<'_>], remote: &[RemoteIoVec], flags: u32) -> KResult<usize>;
    /// `map_shadow_stack`.
    fn map_shadow_stack(&self, hint: usize, size: usize, flags: u32) -> KResult<Region>;

    // ---------------------------------------------------------------- futexes, threads
    /// `futex`. The kernel reads and atomically changes only `word` and `word2`.
    fn futex(&self, word: &AtomicU32, op: u32, val: u32, arg: FutexArg<'_>, word2: Option<&AtomicU32>, val3: u32) -> KResult<i64>;
    /// `rseq` registration (`flags` = 0) or unregistration (`RSEQ_FLAG_UNREGISTER`).
    fn rseq(&self, area: &'static Rseq, flags: u32, sig: u32) -> KResult<()>;
    /// `arch_prctl(ARCH_GET_FS / ARCH_GET_GS)`.
    // ARCH(x86_64): arch_prctl in the Kernel trait, x86_64 only (A-3)
    fn arch_prctl_get(&self, which: ArchGet) -> KResult<u64>;
    /// `arch_prctl(ARCH_GET_CPUID)`: 1 if `cpuid` is enabled.
    fn arch_prctl_get_cpuid(&self) -> KResult<i32>;
    /// `arch_prctl(ARCH_SET_CPUID)`.
    // ARCH(x86_64): arch_prctl in the Kernel trait, x86_64 only (A-3)
    fn arch_prctl_set_cpuid(&self, enable: bool) -> KResult<()>;

    // ---------------------------------------------------------------- descriptors
    /// `openat`.
    fn openat(&self, dir: Fd, path: KStr<'_>, flags: u32, mode: u32) -> KResult<Fd>;
    /// `openat2`.
    fn openat2(&self, dir: Fd, path: KStr<'_>, how: &OpenHow) -> KResult<Fd>;
    /// `open_by_handle_at`.
    fn open_by_handle_at(&self, mount_fd: Fd, handle: &FileHandle, flags: u32) -> KResult<Fd>;
    /// `name_to_handle_at`: fills `handle`, returns the mount ID.
    fn name_to_handle_at(&self, dir: Fd, path: KStr<'_>, handle: &mut FileHandle, flags: u32) -> KResult<i32>;
    /// `close`.
    fn close(&self, fd: Fd) -> KResult<()>;
    /// `close_range`.
    fn close_range(&self, first: u32, last: u32, flags: u32) -> KResult<()>;
    /// `dup`.
    fn dup(&self, fd: Fd) -> KResult<Fd>;
    /// `dup2`.
    fn dup2(&self, old: Fd, new: Fd) -> KResult<Fd>;
    /// `dup3`.
    fn dup3(&self, old: Fd, new: Fd, flags: u32) -> KResult<Fd>;
    /// `fcntl` with an integer argument and result.
    fn fcntl_int(&self, fd: Fd, cmd: FcntlInt, arg: i64) -> KResult<i64>;
    /// `fcntl` record locks.
    fn fcntl_lock(&self, fd: Fd, cmd: FcntlLock, lock: &mut Flock) -> KResult<()>;
    /// `fcntl(F_GETOWN_EX / F_SETOWN_EX)`.
    fn fcntl_owner_ex(&self, fd: Fd, set: bool, owner: &mut FOwnerEx) -> KResult<()>;
    /// `fcntl` read/write hints.
    fn fcntl_hint(&self, fd: Fd, cmd: FcntlHint, hint: &mut u64) -> KResult<()>;
    /// `ioctl` without an argument.
    fn ioctl_none(&self, fd: Fd, req: IoctlNone) -> KResult<i64>;
    /// `ioctl` with an integer argument.
    fn ioctl_int(&self, fd: Fd, req: IoctlIntArg, arg: i64) -> KResult<i64>;
    /// `ioctl` writing one `int`.
    fn ioctl_get_int(&self, fd: Fd, req: IoctlIntOut) -> KResult<i32>;
    /// `ioctl` reading one `int`.
    fn ioctl_set_int(&self, fd: Fd, req: IoctlIntIn, value: i32) -> KResult<i64>;
    /// `ioctl(TCGETS)`.
    fn tcgets(&self, fd: Fd) -> KResult<KTermios>;
    /// `ioctl(TCSETS / TCSETSW / TCSETSF)`.
    fn tcsets(&self, fd: Fd, when: TcSet, t: &KTermios) -> KResult<()>;
    /// `ioctl(TCGETS2)`.
    fn tcgets2(&self, fd: Fd) -> KResult<KTermios2>;
    /// `ioctl(TCSETS2 / TCSETSW2 / TCSETSF2)`.
    fn tcsets2(&self, fd: Fd, when: TcSet2, t: &KTermios2) -> KResult<()>;
    /// `ioctl(TIOCGWINSZ)`.
    fn tiocgwinsz(&self, fd: Fd) -> KResult<Winsize>;
    /// `ioctl(TIOCSWINSZ)`.
    fn tiocswinsz(&self, fd: Fd, ws: &Winsize) -> KResult<()>;
    /// `pipe2`: `[read end, write end]`.
    fn pipe2(&self, flags: u32) -> KResult<[Fd; 2]>;

    // ---------------------------------------------------------------- data transfer
    /// `read`.
    fn read(&self, fd: Fd, buf: &mut [u8]) -> KResult<usize>;
    /// `write`.
    fn write(&self, fd: Fd, buf: &[u8]) -> KResult<usize>;
    /// `pread64`.
    fn pread(&self, fd: Fd, buf: &mut [u8], offset: i64) -> KResult<usize>;
    /// `pwrite64`.
    fn pwrite(&self, fd: Fd, buf: &[u8], offset: i64) -> KResult<usize>;
    /// `readv`.
    fn readv(&self, fd: Fd, iov: &mut [IoVecMut<'_>]) -> KResult<usize>;
    /// `writev`.
    fn writev(&self, fd: Fd, iov: &[IoVec<'_>]) -> KResult<usize>;
    /// `preadv`.
    fn preadv(&self, fd: Fd, iov: &mut [IoVecMut<'_>], offset: i64) -> KResult<usize>;
    /// `pwritev`.
    fn pwritev(&self, fd: Fd, iov: &[IoVec<'_>], offset: i64) -> KResult<usize>;
    /// `preadv2` (`offset` -1 = current position).
    fn preadv2(&self, fd: Fd, iov: &mut [IoVecMut<'_>], offset: i64, flags: u32) -> KResult<usize>;
    /// `pwritev2`.
    fn pwritev2(&self, fd: Fd, iov: &[IoVec<'_>], offset: i64, flags: u32) -> KResult<usize>;
    /// `lseek`: the new offset.
    fn lseek(&self, fd: Fd, offset: i64, whence: u32) -> KResult<u64>;
    /// `sendfile`.
    fn sendfile(&self, out: Fd, input: Fd, offset: Option<&mut i64>, count: usize) -> KResult<usize>;
    /// `copy_file_range`.
    fn copy_file_range(&self, input: Fd, in_off: Option<&mut i64>, out: Fd, out_off: Option<&mut i64>, len: usize, flags: u32) -> KResult<usize>;
    /// `splice`.
    fn splice(&self, input: Fd, in_off: Option<&mut i64>, out: Fd, out_off: Option<&mut i64>, len: usize, flags: u32) -> KResult<usize>;
    /// `tee`.
    fn tee(&self, input: Fd, out: Fd, len: usize, flags: u32) -> KResult<usize>;
    /// `vmsplice`. Takes exclusive buffers in both directions: on a pipe's read end the kernel
    /// writes into them.
    fn vmsplice(&self, fd: Fd, iov: &mut [IoVecMut<'_>], flags: u32) -> KResult<usize>;
    /// `readahead`.
    fn readahead(&self, fd: Fd, offset: i64, count: usize) -> KResult<()>;

    // ---------------------------------------------------------------- readiness, events
    /// `pselect6`. `nfds` above 1,024 is `EINVAL` (the sets hold 1,024 bits). The kernel writes
    /// the remaining time back into `timeout`.
    fn pselect6(&self, nfds: u32, read: Option<&mut FdSet>, write: Option<&mut FdSet>, except: Option<&mut FdSet>, timeout: Option<&mut Timespec>, mask: Option<&SigSet>) -> KResult<usize>;
    /// `ppoll`. The kernel writes the remaining time back into `timeout`.
    fn ppoll(&self, fds: &mut [PollFd], timeout: Option<&mut Timespec>, mask: Option<&SigSet>) -> KResult<usize>;
    /// `epoll_create1`.
    fn epoll_create1(&self, flags: u32) -> KResult<Fd>;
    /// `epoll_ctl`.
    fn epoll_ctl(&self, ep: Fd, op: u32, fd: Fd, event: Option<&EpollEvent>) -> KResult<()>;
    /// `epoll_pwait` (`timeout_ms` -1 = forever).
    fn epoll_pwait(&self, ep: Fd, events: &mut [EpollEvent], timeout_ms: i32, mask: Option<&SigSet>) -> KResult<usize>;
    /// `epoll_pwait2`.
    fn epoll_pwait2(&self, ep: Fd, events: &mut [EpollEvent], timeout: Option<&Timespec>, mask: Option<&SigSet>) -> KResult<usize>;
    /// `eventfd2`.
    fn eventfd2(&self, initval: u32, flags: u32) -> KResult<Fd>;
    /// `inotify_init1`.
    fn inotify_init1(&self, flags: u32) -> KResult<Fd>;
    /// `inotify_add_watch`: the watch descriptor.
    fn inotify_add_watch(&self, fd: Fd, path: KStr<'_>, mask: u32) -> KResult<i32>;
    /// `inotify_rm_watch`.
    fn inotify_rm_watch(&self, fd: Fd, wd: i32) -> KResult<()>;
    /// `fanotify_init`.
    fn fanotify_init(&self, flags: u32, event_flags: u32) -> KResult<Fd>;
    /// `fanotify_mark`.
    fn fanotify_mark(&self, fd: Fd, flags: u32, mask: u64, dir: Fd, path: Option<KStr<'_>>) -> KResult<()>;

    // ---------------------------------------------------------------- durability, space
    /// `fsync`.
    fn fsync(&self, fd: Fd) -> KResult<()>;
    /// `fdatasync`.
    fn fdatasync(&self, fd: Fd) -> KResult<()>;
    /// `syncfs`.
    fn syncfs(&self, fd: Fd) -> KResult<()>;
    /// `sync_file_range`.
    fn sync_file_range(&self, fd: Fd, offset: i64, len: i64, flags: u32) -> KResult<()>;
    /// `fadvise64`.
    fn fadvise(&self, fd: Fd, offset: i64, len: i64, advice: u32) -> KResult<()>;
    /// `fallocate`.
    fn fallocate(&self, fd: Fd, mode: u32, offset: i64, len: i64) -> KResult<()>;
    /// `ftruncate`.
    fn ftruncate(&self, fd: Fd, len: i64) -> KResult<()>;
    /// `truncate`.
    fn truncate(&self, path: KStr<'_>, len: i64) -> KResult<()>;
    /// `flock`.
    fn flock(&self, fd: Fd, op: u32) -> KResult<()>;

    // ---------------------------------------------------------------- namespace and metadata
    /// `newfstatat`.
    fn fstatat(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<Stat>;
    /// `fstat`.
    fn fstat(&self, fd: Fd) -> KResult<Stat>;
    /// `statx`.
    fn statx(&self, dir: Fd, path: KStr<'_>, flags: u32, mask: u32) -> KResult<Statx>;
    /// `statfs`.
    fn statfs(&self, path: KStr<'_>) -> KResult<Statfs>;
    /// `fstatfs`.
    fn fstatfs(&self, fd: Fd) -> KResult<Statfs>;
    /// `getdents64`: bytes of records written into `buf`.
    fn getdents64(&self, fd: Fd, buf: &mut [u8]) -> KResult<usize>;
    /// `getcwd`: bytes written including the terminator.
    fn getcwd(&self, buf: &mut [u8]) -> KResult<usize>;
    /// `chdir`.
    fn chdir(&self, path: KStr<'_>) -> KResult<()>;
    /// `fchdir`.
    fn fchdir(&self, fd: Fd) -> KResult<()>;
    /// `chroot`.
    fn chroot(&self, path: KStr<'_>) -> KResult<()>;
    /// `mkdirat`.
    fn mkdirat(&self, dir: Fd, path: KStr<'_>, mode: u32) -> KResult<()>;
    /// `mknodat`.
    fn mknodat(&self, dir: Fd, path: KStr<'_>, mode: u32, dev: u32) -> KResult<()>;
    /// `linkat`.
    fn linkat(&self, old_dir: Fd, old: KStr<'_>, new_dir: Fd, new: KStr<'_>, flags: u32) -> KResult<()>;
    /// `symlinkat`.
    fn symlinkat(&self, target: KStr<'_>, dir: Fd, path: KStr<'_>) -> KResult<()>;
    /// `readlinkat`: bytes written (no terminator; truncation is silent, ⚑ KERN).
    fn readlinkat(&self, dir: Fd, path: KStr<'_>, buf: &mut [u8]) -> KResult<usize>;
    /// `unlinkat`.
    fn unlinkat(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<()>;
    /// `renameat2`.
    fn renameat2(&self, old_dir: Fd, old: KStr<'_>, new_dir: Fd, new: KStr<'_>, flags: u32) -> KResult<()>;
    /// `fchmodat` (no flags: the system call has none).
    fn fchmodat(&self, dir: Fd, path: KStr<'_>, mode: u32) -> KResult<()>;
    /// `fchmodat2` (Linux 6.6).
    fn fchmodat2(&self, dir: Fd, path: KStr<'_>, mode: u32, flags: u32) -> KResult<()>;
    /// `fchmod`.
    fn fchmod(&self, fd: Fd, mode: u32) -> KResult<()>;
    /// `fchownat` (`u32::MAX` = unchanged).
    fn fchownat(&self, dir: Fd, path: KStr<'_>, uid: u32, gid: u32, flags: u32) -> KResult<()>;
    /// `fchown`.
    fn fchown(&self, fd: Fd, uid: u32, gid: u32) -> KResult<()>;
    /// `faccessat` (no flags).
    fn faccessat(&self, dir: Fd, path: KStr<'_>, mode: u32) -> KResult<()>;
    /// `faccessat2`.
    fn faccessat2(&self, dir: Fd, path: KStr<'_>, mode: u32, flags: u32) -> KResult<()>;
    /// `utimensat`. `path` = `None` operates on `dir` itself; `times` = `None` means now.
    fn utimensat(&self, dir: Fd, path: Option<KStr<'_>>, times: Option<&[Timespec; 2]>, flags: u32) -> KResult<()>;
    /// `setxattr` (`nofollow` selects `lsetxattr`).
    fn setxattr(&self, path: KStr<'_>, nofollow: bool, name: KStr<'_>, value: &[u8], flags: u32) -> KResult<()>;
    /// `fsetxattr`.
    fn fsetxattr(&self, fd: Fd, name: KStr<'_>, value: &[u8], flags: u32) -> KResult<()>;
    /// `getxattr` / `lgetxattr`: value size (an empty `buf` asks for the size).
    fn getxattr(&self, path: KStr<'_>, nofollow: bool, name: KStr<'_>, buf: &mut [u8]) -> KResult<usize>;
    /// `fgetxattr`.
    fn fgetxattr(&self, fd: Fd, name: KStr<'_>, buf: &mut [u8]) -> KResult<usize>;
    /// `listxattr` / `llistxattr`.
    fn listxattr(&self, path: KStr<'_>, nofollow: bool, buf: &mut [u8]) -> KResult<usize>;
    /// `flistxattr`.
    fn flistxattr(&self, fd: Fd, buf: &mut [u8]) -> KResult<usize>;
    /// `removexattr` / `lremovexattr`.
    fn removexattr(&self, path: KStr<'_>, nofollow: bool, name: KStr<'_>) -> KResult<()>;
    /// `fremovexattr`.
    fn fremovexattr(&self, fd: Fd, name: KStr<'_>) -> KResult<()>;

    // ---------------------------------------------------------------- mounting
    /// `mount`.
    fn mount(&self, source: Option<KStr<'_>>, target: KStr<'_>, fstype: Option<KStr<'_>>, flags: u64, data: Option<KStr<'_>>) -> KResult<()>;
    /// `umount2`.
    fn umount2(&self, target: KStr<'_>, flags: u32) -> KResult<()>;
    /// `pivot_root`.
    fn pivot_root(&self, new_root: KStr<'_>, put_old: KStr<'_>) -> KResult<()>;
    /// `fsopen`.
    fn fsopen(&self, fstype: KStr<'_>, flags: u32) -> KResult<Fd>;
    /// `fsconfig`.
    fn fsconfig(&self, fd: Fd, cmd: FsconfigCmd, key: Option<KStr<'_>>, value: FsValue<'_>, aux: i32) -> KResult<()>;
    /// `fsmount`.
    fn fsmount(&self, fd: Fd, flags: u32, attrs: u32) -> KResult<Fd>;
    /// `fspick`.
    fn fspick(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<Fd>;
    /// `move_mount`.
    fn move_mount(&self, from_dir: Fd, from: KStr<'_>, to_dir: Fd, to: KStr<'_>, flags: u32) -> KResult<()>;
    /// `open_tree`.
    fn open_tree(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<Fd>;
    /// `mount_setattr`.
    fn mount_setattr(&self, dir: Fd, path: KStr<'_>, flags: u32, attr: &MountAttr) -> KResult<()>;
    /// `swapon`.
    fn swapon(&self, path: KStr<'_>, flags: u32) -> KResult<()>;
    /// `swapoff`.
    fn swapoff(&self, path: KStr<'_>) -> KResult<()>;
    /// `quotactl` with a fixed-size buffer (`EINVAL` if `buf` is shorter than the command
    /// needs).
    fn quotactl(&self, cmd: QuotaCmd, qtype: u32, special: Option<KStr<'_>>, id: i32, buf: &mut [u8]) -> KResult<()>;
    /// `statmount` (Linux 6.8): fills `buf` with a `struct statmount` and its strings.
    fn statmount(&self, req: &MntIdReq, buf: &mut [u8], flags: u32) -> KResult<()>;
    /// `listmount` (Linux 6.8): mount IDs under `req.mnt_id`; the count written.
    fn listmount(&self, req: &MntIdReq, ids: &mut [u64], flags: u32) -> KResult<usize>;
    /// `quotactl(Q_QUOTAON)`.
    fn quotactl_on(&self, qtype: u32, special: KStr<'_>, format: i32, quota_file: KStr<'_>) -> KResult<()>;

    // ---------------------------------------------------------------- sockets
    /// `socket`.
    fn socket(&self, domain: u32, stype: u32, protocol: u32) -> KResult<Fd>;
    /// `socketpair`.
    fn socketpair(&self, domain: u32, stype: u32, protocol: u32) -> KResult<[Fd; 2]>;
    /// `bind` to an encoded address.
    fn bind(&self, fd: Fd, addr: &[u8]) -> KResult<()>;
    /// `listen`.
    fn listen(&self, fd: Fd, backlog: i32) -> KResult<()>;
    /// `accept4`.
    fn accept4(&self, fd: Fd, peer: Option<&mut SockAddrBuf>, flags: u32) -> KResult<Fd>;
    /// `connect`.
    fn connect(&self, fd: Fd, addr: &[u8]) -> KResult<()>;
    /// `shutdown`.
    fn shutdown(&self, fd: Fd, how: u32) -> KResult<()>;
    /// `getsockname`.
    fn getsockname(&self, fd: Fd, addr: &mut SockAddrBuf) -> KResult<()>;
    /// `getpeername`.
    fn getpeername(&self, fd: Fd, addr: &mut SockAddrBuf) -> KResult<()>;
    /// `getsockopt`: bytes written.
    fn getsockopt(&self, fd: Fd, level: u32, name: u32, value: &mut [u8]) -> KResult<usize>;
    /// `setsockopt` at a level that never registers memory with the kernel.
    fn setsockopt(&self, fd: Fd, level: SockLevel, name: u32, value: &[u8]) -> KResult<()>;
    /// `sendto` (`addr` = `None` for a connected socket).
    fn sendto(&self, fd: Fd, buf: &[u8], flags: u32, addr: Option<&[u8]>) -> KResult<usize>;
    /// `recvfrom`.
    fn recvfrom(&self, fd: Fd, buf: &mut [u8], flags: u32, from: Option<&mut SockAddrBuf>) -> KResult<usize>;
    /// `sendmsg`.
    fn sendmsg(&self, fd: Fd, msg: &MsgHdr<'_>, flags: u32) -> KResult<usize>;
    /// `recvmsg`.
    fn recvmsg(&self, fd: Fd, msg: &mut MsgHdrMut<'_>, flags: u32) -> KResult<usize>;
    /// `sendmmsg`: messages sent.
    fn sendmmsg(&self, fd: Fd, msgs: &mut [MMsgHdr<'_>], flags: u32) -> KResult<usize>;
    /// `recvmmsg`: messages received. The kernel writes the remaining time into `timeout`.
    fn recvmmsg(&self, fd: Fd, msgs: &mut [MMsgHdrMut<'_>], flags: u32, timeout: Option<&mut Timespec>) -> KResult<usize>;

    // ---------------------------------------------------------------- time
    /// `clock_gettime` (through the vDSO where the kernel provides it, P3).
    fn clock_gettime(&self, clock: i32) -> KResult<Timespec>;
    /// `clock_getres`.
    fn clock_getres(&self, clock: i32) -> KResult<Timespec>;
    /// `clock_settime`.
    fn clock_settime(&self, clock: i32, ts: &Timespec) -> KResult<()>;
    /// `clock_adjtime`: the clock state (`TIME_OK`, ...).
    fn clock_adjtime(&self, clock: i32, tx: &mut Timex) -> KResult<i32>;
    /// `clock_nanosleep`. With `EINTR` and a relative sleep, `rem` receives the remainder.
    fn clock_nanosleep(&self, clock: i32, flags: u32, req: &Timespec, rem: Option<&mut Timespec>) -> KResult<()>;
    /// `gettimeofday` (through the vDSO where available).
    fn gettimeofday(&self) -> KResult<(Timeval, Timezone)>;
    /// `settimeofday`.
    fn settimeofday(&self, tv: Option<&Timeval>, tz: Option<&Timezone>) -> KResult<()>;
    /// `time` (through the vDSO where available).
    fn time(&self) -> KResult<i64>;
    /// `times`: clock ticks since an arbitrary point.
    fn times(&self, buf: &mut Tms) -> KResult<u64>;
    /// `setitimer`; returns the previous timer.
    fn setitimer(&self, which: u32, new: &Itimerval) -> KResult<Itimerval>;
    /// `getitimer`.
    fn getitimer(&self, which: u32) -> KResult<Itimerval>;
    /// `alarm`: seconds left on the previous alarm.
    fn alarm(&self, seconds: u32) -> KResult<u32>;
    /// `timer_create`: the kernel timer ID.
    fn timer_create(&self, clock: i32, event: Option<&SigEvent>) -> KResult<i32>;
    /// `timer_settime`; returns the previous setting.
    fn timer_settime(&self, id: i32, flags: u32, new: &Itimerspec) -> KResult<Itimerspec>;
    /// `timer_gettime`.
    fn timer_gettime(&self, id: i32) -> KResult<Itimerspec>;
    /// `timer_getoverrun`.
    fn timer_getoverrun(&self, id: i32) -> KResult<i32>;
    /// `timer_delete`.
    fn timer_delete(&self, id: i32) -> KResult<()>;
    /// `timerfd_create`.
    fn timerfd_create(&self, clock: i32, flags: u32) -> KResult<Fd>;
    /// `timerfd_settime`; returns the previous setting.
    fn timerfd_settime(&self, fd: Fd, flags: u32, new: &Itimerspec) -> KResult<Itimerspec>;
    /// `timerfd_gettime`.
    fn timerfd_gettime(&self, fd: Fd) -> KResult<Itimerspec>;

    // ---------------------------------------------------------------- scheduling, resources
    /// `sched_setscheduler`.
    fn sched_setscheduler(&self, pid: i32, policy: u32, param: &SchedParam) -> KResult<()>;
    /// `sched_getscheduler`.
    fn sched_getscheduler(&self, pid: i32) -> KResult<u32>;
    /// `sched_setparam`.
    fn sched_setparam(&self, pid: i32, param: &SchedParam) -> KResult<()>;
    /// `sched_getparam`.
    fn sched_getparam(&self, pid: i32) -> KResult<SchedParam>;
    /// `sched_get_priority_min`.
    fn sched_get_priority_min(&self, policy: u32) -> KResult<i32>;
    /// `sched_get_priority_max`.
    fn sched_get_priority_max(&self, policy: u32) -> KResult<i32>;
    /// `sched_rr_get_interval`.
    fn sched_rr_get_interval(&self, pid: i32) -> KResult<Timespec>;
    /// `sched_setaffinity`.
    fn sched_setaffinity(&self, pid: i32, mask: &[u8]) -> KResult<()>;
    /// `sched_getaffinity`: bytes the kernel wrote (its mask size).
    fn sched_getaffinity(&self, pid: i32, mask: &mut [u8]) -> KResult<usize>;
    /// `getcpu`: (cpu, node).
    fn getcpu(&self) -> KResult<(u32, u32)>;
    /// `getpriority`: the kernel's biased value `20 - nice` (1..=40, ⚑ KERN).
    fn getpriority(&self, which: u32, who: u32) -> KResult<i32>;
    /// `setpriority`.
    fn setpriority(&self, which: u32, who: u32, prio: i32) -> KResult<()>;
    /// `prlimit64`: sets `new` if given; returns the previous limit.
    fn prlimit(&self, pid: i32, resource: u32, new: Option<&Rlimit>) -> KResult<Rlimit>;
    /// `getrusage`.
    fn getrusage(&self, who: i32) -> KResult<Rusage>;
    /// `sysinfo`.
    fn sysinfo(&self) -> KResult<Sysinfo>;
    /// `sched_setattr` (H17: a modern call the POSIX layer does not export).
    fn sched_setattr(&self, pid: i32, attr: &SchedAttr, flags: u32) -> KResult<()>;
    /// `sched_getattr`.
    fn sched_getattr(&self, pid: i32, flags: u32) -> KResult<SchedAttr>;

    // ---------------------------------------------------------------- System V and POSIX IPC
    /// `msgget`.
    fn msgget(&self, key: i32, flags: u32) -> KResult<i32>;
    /// `msgsnd`: `msg` is the 8-byte type followed by the text (`EINVAL` if shorter than 8).
    fn msgsnd(&self, id: i32, msg: &[u8], flags: u32) -> KResult<()>;
    /// `msgrcv` into `buf` (type + text): text bytes received.
    fn msgrcv(&self, id: i32, buf: &mut [u8], mtype: i64, flags: u32) -> KResult<usize>;
    /// `msgctl`.
    fn msgctl(&self, id: i32, cmd: MsgCtl, buf: &mut MsqidDs) -> KResult<i32>;
    /// `semget`.
    fn semget(&self, key: i32, nsems: i32, flags: u32) -> KResult<i32>;
    /// `semtimedop` (`timeout` = `None` for `semop`).
    fn semtimedop(&self, id: i32, ops: &[Sembuf], timeout: Option<&Timespec>) -> KResult<()>;
    /// `semctl` with an integer argument.
    fn semctl_int(&self, id: i32, num: i32, cmd: SemCtlInt, value: i32) -> KResult<i32>;
    /// `semctl` with a structure argument.
    fn semctl_ds(&self, id: i32, cmd: SemCtlDs, buf: &mut SemidDs) -> KResult<i32>;
    /// `semctl(GETALL)`: `values` needs one slot per semaphore (else `EINVAL`).
    fn semctl_getall(&self, id: i32, values: &mut [u16]) -> KResult<()>;
    /// `semctl(SETALL)`.
    fn semctl_setall(&self, id: i32, values: &[u16]) -> KResult<()>;
    /// `shmget`.
    fn shmget(&self, key: i32, size: usize, flags: u32) -> KResult<i32>;
    /// `shmat` at a kernel-chosen address (`SHM_REMAP` is refused).
    fn shmat(&self, id: i32, flags: u32) -> KResult<Region>;
    /// `shmdt` of an attachment made by `shmat`.
    fn shmdt(&self, region: Region) -> KResult<()>;
    /// `shmctl`.
    fn shmctl(&self, id: i32, cmd: ShmCtl, buf: &mut ShmidDs) -> KResult<i32>;
    /// `mq_open` (`name` without the leading `/`, ⚑ KERN).
    fn mq_open(&self, name: KStr<'_>, flags: u32, mode: u32, attr: Option<&MqAttr>) -> KResult<Fd>;
    /// `mq_unlink`.
    fn mq_unlink(&self, name: KStr<'_>) -> KResult<()>;
    /// `mq_timedsend`.
    fn mq_timedsend(&self, fd: Fd, msg: &[u8], prio: u32, timeout: Option<&Timespec>) -> KResult<()>;
    /// `mq_timedreceive`: bytes received.
    fn mq_timedreceive(&self, fd: Fd, buf: &mut [u8], prio: Option<&mut u32>, timeout: Option<&Timespec>) -> KResult<usize>;
    /// `mq_notify`.
    fn mq_notify(&self, fd: Fd, event: Option<&SigEvent>) -> KResult<()>;
    /// `mq_getsetattr`; returns the previous attributes.
    fn mq_getsetattr(&self, fd: Fd, new: Option<&MqAttr>) -> KResult<MqAttr>;

    // ---------------------------------------------------------------- randomness
    /// `getrandom`: bytes written.
    fn getrandom(&self, buf: &mut [u8], flags: u32) -> KResult<usize>;

    // ---------------------------------------------------------------- system
    /// `uname`.
    fn uname(&self) -> KResult<Utsname>;
    /// `sethostname`.
    fn sethostname(&self, name: &[u8]) -> KResult<()>;
    /// `setdomainname`.
    fn setdomainname(&self, name: &[u8]) -> KResult<()>;
    /// `reboot`. The magic numbers are part of the call (⚑ KERN).
    fn reboot(&self, magic1: u32, magic2: u32, cmd: u32, arg: Option<KStr<'_>>) -> KResult<()>;
    /// `syslog` (glibc: `klogctl`): bytes read, or the action's result.
    fn syslog(&self, action: i32, buf: &mut [u8]) -> KResult<usize>;
    /// `acct` (`None` turns accounting off).
    fn acct(&self, path: Option<KStr<'_>>) -> KResult<()>;
    /// `ioperm`.
    fn ioperm(&self, from: u64, num: u64, on: bool) -> KResult<()>;
    /// `iopl`.
    fn iopl(&self, level: u32) -> KResult<()>;
    /// `modify_ldt` read (function 0 or 2): bytes read.
    fn modify_ldt_read(&self, func: u32, buf: &mut [u8]) -> KResult<usize>;
    /// `init_module`.
    fn init_module(&self, image: &[u8], params: KStr<'_>) -> KResult<()>;
    /// `finit_module`.
    fn finit_module(&self, fd: Fd, params: KStr<'_>, flags: u32) -> KResult<()>;
    /// `delete_module`.
    fn delete_module(&self, name: KStr<'_>, flags: u32) -> KResult<()>;
    /// `vhangup`.
    fn vhangup(&self) -> KResult<()>;
    /// `personality`: the previous persona.
    fn personality(&self, persona: u32) -> KResult<u32>;
    /// `prctl` with integer arguments.
    fn prctl_int(&self, opt: PrctlInt, a2: u64, a3: u64) -> KResult<i64>;
    /// `prctl` writing one `int`.
    fn prctl_get_int(&self, opt: PrctlIntOut) -> KResult<i32>;
    /// `prctl(PR_SET_NAME)`.
    fn prctl_set_name(&self, name: &[u8; 16]) -> KResult<()>;
    /// `prctl(PR_GET_NAME)`.
    fn prctl_get_name(&self) -> KResult<[u8; 16]>;
    /// `unshare`.
    fn unshare(&self, flags: u64) -> KResult<()>;
    /// `setns`.
    fn setns(&self, fd: Fd, nstype: u32) -> KResult<()>;
    /// `ptrace` with integer arguments.
    fn ptrace(&self, op: PtraceOp, pid: i32, addr: u64, data: u64) -> KResult<i64>;
    /// `ptrace(PTRACE_PEEK*)` / `PTRACE_GETEVENTMSG`: the word read.
    fn ptrace_peek(&self, op: PtracePeek, pid: i32, addr: u64) -> KResult<u64>;
}
infallible {
    /// The CPU time-stamp counter, or 0 where there is none (not x86_64, or a test double): a coarse
    /// elapsed-time gate only (D-7.12, GETTEXT-6).
    fn tsc(&self) -> u64;
    /// `getpid`.
    fn getpid(&self) -> i32;
    /// `getppid`.
    fn getppid(&self) -> i32;
    /// `gettid`.
    fn gettid(&self) -> i32;
    /// `getuid`.
    fn getuid(&self) -> u32;
    /// `geteuid`.
    fn geteuid(&self) -> u32;
    /// `getgid`.
    fn getgid(&self) -> u32;
    /// `getegid`.
    fn getegid(&self) -> u32;
    /// `setfsuid`: the previous fsuid (the call reports no errors, ⚑ KERN).
    fn setfsuid(&self, uid: u32) -> u32;
    /// `setfsgid`.
    fn setfsgid(&self, gid: u32) -> u32;
    /// `getpgrp`.
    fn getpgrp(&self) -> i32;
    /// `sched_yield`.
    fn sched_yield(&self) -> ();
    /// `sync`.
    fn sync(&self) -> ();
    /// `umask`: the previous mask.
    fn umask(&self, mask: u32) -> u32;
}
}
