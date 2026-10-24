//! Kernel ABI data: x86_64 structure layouts and constants from the Linux UAPI (normative
//! reference, OD-2; ⚑ KERN). Every structure is plain data (`#[repr(C)]`, integers only, no
//! pointers), so a `&mut` to one may be handed to the kernel for it to fill.
//!
//! Layout tests: `crates/kernel/tests/uapi_layout.rs`.

#![allow(missing_docs)] // names and meanings are the kernel's; see the UAPI headers

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};

// ------------------------------------------------------------------------ time
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Timespec { pub sec: i64, pub nsec: i64 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Timeval { pub sec: i64, pub usec: i64 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Timezone { pub minuteswest: i32, pub dsttime: i32 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Itimerval { pub interval: Timeval, pub value: Timeval }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Itimerspec { pub interval: Timespec, pub value: Timespec }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Tms { pub utime: i64, pub stime: i64, pub cutime: i64, pub cstime: i64 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Timex {
    pub modes: u32, _pad0: u32, pub offset: i64, pub freq: i64, pub maxerror: i64,
    pub esterror: i64, pub status: i32, _pad1: u32, pub constant: i64, pub precision: i64,
    pub tolerance: i64, pub time: Timeval, pub tick: i64, pub ppsfreq: i64, pub jitter: i64,
    pub shift: i32, _pad2: u32, pub stabil: i64, pub jitcnt: i64, pub calcnt: i64,
    pub errcnt: i64, pub stbcnt: i64, pub tai: i32, _reserved: [i32; 11],
}

pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;
pub const CLOCK_TAI: i32 = 11;
pub const TIMER_ABSTIME: i32 = 1;
pub const ITIMER_REAL: i32 = 0;
pub const ITIMER_VIRTUAL: i32 = 1;
pub const ITIMER_PROF: i32 = 2;
pub const TFD_TIMER_ABSTIME: i32 = 1;
pub const TFD_NONBLOCK: i32 = 0o4000;
pub const TFD_CLOEXEC: i32 = 0o2000000;

// ------------------------------------------------------------------------ files
/// `struct stat` (x86_64; identical to glibc's, spec Appendix C).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Stat {
    pub dev: u64, pub ino: u64, pub nlink: u64, pub mode: u32, pub uid: u32, pub gid: u32,
    _pad0: i32, pub rdev: u64, pub size: i64, pub blksize: i64, pub blocks: i64,
    pub atime: Timespec, pub mtime: Timespec, pub ctime: Timespec, _unused: [i64; 3],
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct StatxTimestamp { pub sec: i64, pub nsec: u32, _reserved: i32 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Statx {
    pub mask: u32, pub blksize: u32, pub attributes: u64, pub nlink: u32, pub uid: u32,
    pub gid: u32, pub mode: u16, _spare0: u16, pub ino: u64, pub size: u64, pub blocks: u64,
    pub attributes_mask: u64, pub atime: StatxTimestamp, pub btime: StatxTimestamp,
    pub ctime: StatxTimestamp, pub mtime: StatxTimestamp, pub rdev_major: u32,
    pub rdev_minor: u32, pub dev_major: u32, pub dev_minor: u32, pub mnt_id: u64,
    pub dio_mem_align: u32, pub dio_offset_align: u32, _spare: [u64; 12],
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Statfs {
    pub ftype: i64, pub bsize: i64, pub blocks: u64, pub bfree: u64, pub bavail: u64,
    pub files: u64, pub ffree: u64, pub fsid: [i32; 2], pub namelen: i64, pub frsize: i64,
    pub flags: i64, _spare: [i64; 4],
}
/// `struct file_handle` with room for `MAX_HANDLE_SZ` bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct FileHandle { pub handle_bytes: u32, pub handle_type: i32, pub f_handle: [u8; 128] }
impl Default for FileHandle {
    fn default() -> Self { FileHandle { handle_bytes: 128, handle_type: 0, f_handle: [0; 128] } }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct OpenHow { pub flags: u64, pub mode: u64, pub resolve: u64 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Flock { pub ltype: i16, pub whence: i16, _pad0: i32, pub start: i64, pub len: i64, pub pid: i32, _pad1: i32 }
impl Flock {
    pub const fn new(ltype: i16, whence: i16, start: i64, len: i64) -> Flock {
        Flock { ltype, whence, _pad0: 0, start, len, pid: 0, _pad1: 0 }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct FOwnerEx { pub otype: i32, pub pid: i32 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct MountAttr { pub attr_set: u64, pub attr_clr: u64, pub propagation: u64, pub userns_fd: u64 }

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_NOCTTY: u32 = 0o400;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DSYNC: u32 = 0o10000;
pub const O_DIRECT: u32 = 0o40000;
pub const O_LARGEFILE: u32 = 0o100000;
pub const O_DIRECTORY: u32 = 0o200000;
pub const O_NOFOLLOW: u32 = 0o400000;
pub const O_NOATIME: u32 = 0o1000000;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const O_SYNC: u32 = 0o4010000;
pub const O_PATH: u32 = 0o10000000;
pub const O_TMPFILE: u32 = 0o20200000;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const AT_REMOVEDIR: u32 = 0x200;
pub const AT_EACCESS: u32 = 0x200;
pub const AT_SYMLINK_FOLLOW: u32 = 0x400;
pub const AT_NO_AUTOMOUNT: u32 = 0x800;
pub const AT_EMPTY_PATH: u32 = 0x1000;
pub const AT_RECURSIVE: u32 = 0x8000;
pub const STATX_BASIC_STATS: u32 = 0x7ff;
pub const STATX_BTIME: u32 = 0x800;
pub const S_IFMT: u32 = 0o170000;
pub const S_IFSOCK: u32 = 0o140000;
pub const S_IFLNK: u32 = 0o120000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFBLK: u32 = 0o060000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFIFO: u32 = 0o010000;
pub const RENAME_NOREPLACE: u32 = 1;
pub const RENAME_EXCHANGE: u32 = 2;
pub const F_OK: u32 = 0;
pub const X_OK: u32 = 1;
pub const W_OK: u32 = 2;
pub const R_OK: u32 = 4;
pub const UTIME_NOW: i64 = (1 << 30) - 1;
pub const UTIME_OMIT: i64 = (1 << 30) - 2;
pub const XATTR_CREATE: u32 = 1;
pub const XATTR_REPLACE: u32 = 2;
pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;
pub const SEEK_DATA: u32 = 3;
pub const SEEK_HOLE: u32 = 4;
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
pub const RESOLVE_BENEATH: u64 = 0x08;
pub const CLOSE_RANGE_CLOEXEC: u32 = 4;
pub const FD_CLOEXEC: i64 = 1;
pub const F_DUPFD: u32 = 0;
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const F_GETLK: u32 = 5;
pub const F_SETLK: u32 = 6;
pub const F_SETLKW: u32 = 7;
pub const F_SETOWN: u32 = 8;
pub const F_GETOWN: u32 = 9;
pub const F_SETSIG: u32 = 10;
pub const F_GETSIG: u32 = 11;
pub const F_SETOWN_EX: u32 = 15;
pub const F_GETOWN_EX: u32 = 16;
pub const F_OFD_GETLK: u32 = 36;
pub const F_OFD_SETLK: u32 = 37;
pub const F_OFD_SETLKW: u32 = 38;
pub const F_SETLEASE: u32 = 1024;
pub const F_GETLEASE: u32 = 1025;
pub const F_NOTIFY: u32 = 1026;
pub const F_DUPFD_CLOEXEC: u32 = 1030;
pub const F_SETPIPE_SZ: u32 = 1031;
pub const F_GETPIPE_SZ: u32 = 1032;
pub const F_ADD_SEALS: u32 = 1033;
pub const F_GET_SEALS: u32 = 1034;
pub const F_GET_RW_HINT: u32 = 1035;
pub const F_SET_RW_HINT: u32 = 1036;
pub const F_GET_FILE_RW_HINT: u32 = 1037;
pub const F_SET_FILE_RW_HINT: u32 = 1038;
pub const F_RDLCK: i16 = 0;
pub const F_WRLCK: i16 = 1;
pub const F_UNLCK: i16 = 2;
pub const F_SEAL_SEAL: i64 = 1;
pub const F_SEAL_SHRINK: i64 = 2;
pub const F_SEAL_GROW: i64 = 4;
pub const F_SEAL_WRITE: i64 = 8;
pub const LOCK_SH: u32 = 1;
pub const LOCK_EX: u32 = 2;
pub const LOCK_NB: u32 = 4;
pub const LOCK_UN: u32 = 8;
pub const SPLICE_F_NONBLOCK: u32 = 2;
pub const SPLICE_F_GIFT: u32 = 8;
pub const RWF_HIPRI: u32 = 1;
pub const RWF_DSYNC: u32 = 2;
pub const RWF_SYNC: u32 = 4;
pub const RWF_NOWAIT: u32 = 8;
pub const RWF_APPEND: u32 = 16;
pub const FALLOC_FL_KEEP_SIZE: u32 = 1;
pub const FALLOC_FL_PUNCH_HOLE: u32 = 2;
pub const POSIX_FADV_NORMAL: u32 = 0;
pub const POSIX_FADV_SEQUENTIAL: u32 = 2;
pub const POSIX_FADV_DONTNEED: u32 = 4;
pub const SYNC_FILE_RANGE_WRITE: u32 = 2;
pub const FIONREAD: u32 = 0x541B;
pub const FIOCLEX: u32 = 0x5451;
pub const FIONCLEX: u32 = 0x5450;
pub const TCGETS: u32 = 0x5401;
/// `TCGETS2`: the kernel `struct termios2` (with integer speeds).
pub const TCGETS2: u32 = 0x802C_542A;
pub const TIOCGWINSZ: u32 = 0x5413;

// ------------------------------------------------------------------------ memory
pub const PROT_NONE: u32 = 0;
pub const PROT_READ: u32 = 1;
pub const PROT_WRITE: u32 = 2;
pub const PROT_EXEC: u32 = 4;
pub const MAP_SHARED: u32 = 0x01;
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_SHARED_VALIDATE: u32 = 0x03;
pub const MAP_FIXED: u32 = 0x10;
pub const MAP_ANONYMOUS: u32 = 0x20;
pub const MAP_GROWSDOWN: u32 = 0x100;
pub const MAP_LOCKED: u32 = 0x2000;
pub const MAP_NORESERVE: u32 = 0x4000;
pub const MAP_POPULATE: u32 = 0x8000;
pub const MAP_STACK: u32 = 0x20000;
pub const MAP_HUGETLB: u32 = 0x40000;
pub const MAP_FIXED_NOREPLACE: u32 = 0x100000;
pub const MREMAP_MAYMOVE: u32 = 1;
pub const MREMAP_FIXED: u32 = 2;
pub const MADV_NORMAL: u32 = 0;
pub const MADV_RANDOM: u32 = 1;
pub const MADV_SEQUENTIAL: u32 = 2;
pub const MADV_WILLNEED: u32 = 3;
pub const MADV_DONTNEED: u32 = 4;
/// `MPOL_PREFERRED`: allocate on the given node when it has memory.
pub const MPOL_PREFERRED: u32 = 1;
/// `MPOL_F_NODE` (`get_mempolicy`): return a node instead of the policy.
pub const MPOL_F_NODE: u32 = 1;
/// `MPOL_F_ADDR` (`get_mempolicy`): the policy or node of an address.
pub const MPOL_F_ADDR: u32 = 2;
pub const MADV_FREE: u32 = 8;
pub const MADV_NOHUGEPAGE: u32 = 15;
pub const MADV_DONTDUMP: u32 = 16;
pub const MADV_WIPEONFORK: u32 = 18;
pub const MADV_COLD: u32 = 20;
pub const MADV_PAGEOUT: u32 = 21;
pub const MS_ASYNC: u32 = 1;
pub const MS_INVALIDATE: u32 = 2;
pub const MS_SYNC: u32 = 4;
pub const MCL_CURRENT: u32 = 1;
pub const MCL_FUTURE: u32 = 2;
pub const MCL_ONFAULT: u32 = 4;
pub const MLOCK_ONFAULT: u32 = 1;
pub const MFD_CLOEXEC: u32 = 1;
pub const MFD_ALLOW_SEALING: u32 = 2;
pub const PKEY_DISABLE_ACCESS: u32 = 1;
pub const PKEY_DISABLE_WRITE: u32 = 2;

// ------------------------------------------------------------------------ processes
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct CloneArgs {
    pub flags: u64, pub pidfd: u64, pub child_tid: u64, pub parent_tid: u64,
    pub exit_signal: u64, pub stack: u64, pub stack_size: u64, pub tls: u64,
    pub set_tid: u64, pub set_tid_size: u64, pub cgroup: u64,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Rusage {
    pub utime: Timeval, pub stime: Timeval, pub maxrss: i64, pub ixrss: i64, pub idrss: i64,
    pub isrss: i64, pub minflt: i64, pub majflt: i64, pub nswap: i64, pub inblock: i64,
    pub oublock: i64, pub msgsnd: i64, pub msgrcv: i64, pub nsignals: i64, pub nvcsw: i64,
    pub nivcsw: i64,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Rlimit { pub cur: u64, pub max: u64 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct CapHeader { pub version: u32, pub pid: i32 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct CapData { pub effective: u32, pub permitted: u32, pub inheritable: u32 }

pub const CLONE_VM: u64 = 0x100;
pub const CLONE_FS: u64 = 0x200;
pub const CLONE_FILES: u64 = 0x400;
pub const CLONE_SIGHAND: u64 = 0x800;
pub const CLONE_PIDFD: u64 = 0x1000;
pub const CLONE_VFORK: u64 = 0x4000;
pub const CLONE_PARENT: u64 = 0x8000;
pub const CLONE_THREAD: u64 = 0x10000;
pub const CLONE_NEWNS: u64 = 0x20000;
pub const CLONE_SYSVSEM: u64 = 0x40000;
pub const CLONE_SETTLS: u64 = 0x80000;
pub const CLONE_PARENT_SETTID: u64 = 0x100000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x200000;
pub const CLONE_CHILD_SETTID: u64 = 0x1000000;
pub const CLONE_NEWCGROUP: u64 = 0x2000000;
pub const CLONE_NEWUTS: u64 = 0x4000000;
pub const CLONE_NEWIPC: u64 = 0x8000000;
pub const CLONE_NEWUSER: u64 = 0x10000000;
pub const CLONE_NEWPID: u64 = 0x20000000;
pub const CLONE_NEWNET: u64 = 0x40000000;
pub const CLONE_CLEAR_SIGHAND: u64 = 0x100000000;
pub const CLONE_INTO_CGROUP: u64 = 0x200000000;
pub const WNOHANG: u32 = 1;
pub const WUNTRACED: u32 = 2;
pub const WSTOPPED: u32 = 2;
pub const WEXITED: u32 = 4;
pub const WCONTINUED: u32 = 8;
pub const WNOWAIT: u32 = 0x0100_0000;
pub const P_ALL: u32 = 0;
pub const P_PID: u32 = 1;
pub const P_PGID: u32 = 2;
pub const P_PIDFD: u32 = 3;
pub const PIDFD_NONBLOCK: u32 = O_NONBLOCK;
pub const PRIO_PROCESS: u32 = 0;
/// The nice value of `getpriority`'s result: the system call returns `20 - nice` (1..=40) so
/// that no success looks like an error (⚑ KERN, D-24.4).
pub const fn nice_from_priority(biased: i32) -> i32 {
    20i32.wrapping_sub(biased)
}
pub const PRIO_PGRP: u32 = 1;
pub const PRIO_USER: u32 = 2;
pub const RLIMIT_CPU: u32 = 0;
pub const RLIMIT_FSIZE: u32 = 1;
pub const RLIMIT_DATA: u32 = 2;
pub const RLIMIT_STACK: u32 = 3;
pub const RLIMIT_CORE: u32 = 4;
pub const RLIMIT_NPROC: u32 = 6;
pub const RLIMIT_NOFILE: u32 = 7;
pub const RLIMIT_MEMLOCK: u32 = 8;
pub const RLIMIT_AS: u32 = 9;
pub const RLIM_INFINITY: u64 = u64::MAX;
pub const RUSAGE_SELF: i32 = 0;
pub const RUSAGE_CHILDREN: i32 = -1;
pub const RUSAGE_THREAD: i32 = 1;
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

// ------------------------------------------------------------------------ scheduling
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct SchedParam { pub priority: i32 }
/// `struct sysinfo`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Sysinfo {
    pub uptime: i64, pub loads: [u64; 3], pub totalram: u64, pub freeram: u64,
    pub sharedram: u64, pub bufferram: u64, pub totalswap: u64, pub freeswap: u64,
    pub procs: u16, _pad: u16, pub totalhigh: u64, pub freehigh: u64, pub mem_unit: u32,
}
/// `struct sched_attr`, version 1 (56 bytes). The size field is private: the kernel reads that
/// many bytes, so it always equals the structure's own size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct SchedAttr {
    size: u32,
    pub policy: u32,
    pub flags: u64,
    pub nice: i32,
    pub priority: u32,
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
    pub util_min: u32,
    pub util_max: u32,
}
impl Default for SchedAttr {
    fn default() -> Self {
        SchedAttr { size: 56, policy: 0, flags: 0, nice: 0, priority: 0, runtime: 0, deadline: 0, period: 0, util_min: 0, util_max: 0 }
    }
}
impl SchedAttr {
    /// The size the kernel is told (and fills).
    pub fn size(&self) -> u32 { self.size }
}
/// `struct mnt_id_req` (version 0, 24 bytes) for `statmount` and `listmount`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct MntIdReq { size: u32, spare: u32, pub mnt_id: u64, pub param: u64 }
impl MntIdReq {
    /// A request for mount `mnt_id` with `param` (statmount: the request mask; listmount: the
    /// last ID already listed).
    pub const fn new(mnt_id: u64, param: u64) -> MntIdReq { MntIdReq { size: 24, spare: 0, mnt_id, param } }
}
/// `LSMT_ROOT`: list the mounts under the root.
pub const LSMT_ROOT: u64 = u64::MAX;
pub const STATMOUNT_SB_BASIC: u64 = 0x1;
pub const STATMOUNT_MNT_BASIC: u64 = 0x2;
pub const SCHED_OTHER: u32 = 0;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_BATCH: u32 = 3;
pub const SCHED_IDLE: u32 = 5;

// ------------------------------------------------------------------------ signals
/// The kernel's signal set (64 bits). glibc's `sigset_t` is 128 bytes (Appendix C.6); the
/// conversion is the boundary's (finding F19).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SigSet(pub u64);
#[allow(clippy::arithmetic_side_effects)] // `sig - 1` only after the range check
impl SigSet {
    pub const fn empty() -> SigSet { SigSet(0) }
    pub const fn with(self, sig: i32) -> SigSet {
        if sig >= 1 && sig <= 64 { SigSet(self.0 | (1u64 << (sig - 1))) } else { self }
    }
    pub const fn contains(self, sig: i32) -> bool {
        sig >= 1 && sig <= 64 && self.0 & (1u64 << (sig - 1)) != 0
    }
}
/// The kernel `struct sigaction` (x86_64). `handler` and `restorer` are addresses; the safe
/// `Sys` interface never installs one (D-X.9).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct KSigAction { pub handler: usize, pub flags: u64, pub restorer: usize, pub mask: SigSet }
/// `siginfo_t` (128 bytes).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct SigInfo { pub signo: i32, pub errno: i32, pub code: i32, _pad: i32, pub fields: [u64; 14] }
#[allow(clippy::as_conversions)] // packing the kernel's 32-bit pid/uid pair into a word (⚑ KERN)
impl SigInfo {
    /// A queued-signal payload (`SI_QUEUE`) from `pid`/`uid` carrying `value`.
    pub fn queued(signo: i32, pid: i32, uid: u32, value: u64) -> SigInfo {
        let mut s = SigInfo { signo, code: SI_QUEUE, ..SigInfo::default() };
        s.fields[0] = (pid as u32 as u64) | ((uid as u64) << 32);
        s.fields[1] = value;
        s
    }
    /// `si_pid`.
    pub fn pid(&self) -> i32 { (self.fields[0] & 0xffff_ffff) as u32 as i32 }
    /// `si_uid`.
    pub fn uid(&self) -> u32 { (self.fields[0] >> 32) as u32 }
    /// `si_status` (SIGCHLD).
    pub fn status(&self) -> i32 { (self.fields[1] & 0xffff_ffff) as u32 as i32 }
    /// `si_value` as an integer.
    pub fn value(&self) -> u64 { self.fields[1] }
}
/// `stack_t`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct StackT { pub sp: usize, pub flags: i32, _pad: i32, pub size: usize }
/// `struct signalfd_siginfo` (128 bytes).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C, align(8))]
pub struct SignalfdSiginfo { pub words: [u32; 32] }
impl SignalfdSiginfo {
    pub fn signo(&self) -> u32 { self.words[0] }
    pub fn pid(&self) -> u32 { self.words[3] }
}
/// `struct sigevent` (64 bytes; the kernel form).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct SigEvent { pub value: u64, pub signo: i32, pub notify: i32, pub tid: i32, _pad: [i32; 11] }
impl SigEvent {
    pub const fn new(notify: i32, signo: i32, value: u64, tid: i32) -> SigEvent {
        SigEvent { value, signo, notify, tid, _pad: [0; 11] }
    }
    /// The C library's `SIGEV_THREAD` form: `function` and `attributes` (addresses) in the two
    /// words [`SigEvent::thread_words`] reads.
    pub fn thread(value: u64, function: u64, attributes: u64) -> SigEvent {
        let halves = |w: u64| {
            let b = w.to_ne_bytes();
            (i32::from_ne_bytes([b[0], b[1], b[2], b[3]]), i32::from_ne_bytes([b[4], b[5], b[6], b[7]]))
        };
        let ((f0, f1), (a0, a1)) = (halves(function), halves(attributes));
        let mut e = SigEvent::new(SIGEV_THREAD, 0, value, f0);
        e._pad[0] = f1;
        e._pad[1] = a0;
        e._pad[2] = a1;
        e
    }
    /// The two 64-bit words at offsets 16 and 24 (`tid` and the padding after it), where a C
    /// `struct sigevent` keeps `SIGEV_THREAD`'s function and attribute addresses. The kernel
    /// never reads them for that notification (it does not accept it).
    pub fn thread_words(&self) -> [u64; 2] {
        let w = |lo: i32, hi: i32| {
            let (l, h) = (lo.to_ne_bytes(), hi.to_ne_bytes());
            u64::from_ne_bytes([l[0], l[1], l[2], l[3], h[0], h[1], h[2], h[3]])
        };
        [w(self.tid, self._pad[0]), w(self._pad[1], self._pad[2])]
    }
}

pub const SIGHUP: i32 = 1;
pub const SIGINT: i32 = 2;
pub const SIGQUIT: i32 = 3;
pub const SIGILL: i32 = 4;
pub const SIGTRAP: i32 = 5;
pub const SIGABRT: i32 = 6;
pub const SIGBUS: i32 = 7;
pub const SIGFPE: i32 = 8;
pub const SIGKILL: i32 = 9;
pub const SIGUSR1: i32 = 10;
pub const SIGSEGV: i32 = 11;
pub const SIGUSR2: i32 = 12;
pub const SIGPIPE: i32 = 13;
pub const SIGALRM: i32 = 14;
pub const SIGTERM: i32 = 15;
pub const SIGCHLD: i32 = 17;
pub const SIGCONT: i32 = 18;
pub const SIGSTOP: i32 = 19;
pub const SIGTSTP: i32 = 20;
pub const SIGURG: i32 = 23;
pub const SIGVTALRM: i32 = 26;
pub const SIGPROF: i32 = 27;
pub const SIGWINCH: i32 = 28;
pub const SIGIO: i32 = 29;
pub const SIGSYS: i32 = 31;
/// First real-time signal as the kernel numbers it (glibc's `SIGRTMIN` is a runtime value).
pub const SIGRTMIN_KERNEL: i32 = 32;
pub const NSIG: i32 = 64;
pub const SA_NOCLDSTOP: u64 = 1;
pub const SA_NOCLDWAIT: u64 = 2;
pub const SA_SIGINFO: u64 = 4;
pub const SA_RESTORER: u64 = 0x0400_0000;
pub const SA_ONSTACK: u64 = 0x0800_0000;
pub const SA_RESTART: u64 = 0x1000_0000;
pub const SA_NODEFER: u64 = 0x4000_0000;
pub const SA_RESETHAND: u64 = 0x8000_0000;
pub const SIG_BLOCK: u32 = 0;
pub const SIG_UNBLOCK: u32 = 1;
pub const SIG_SETMASK: u32 = 2;
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;
pub const SS_ONSTACK: i32 = 1;
pub const SS_DISABLE: i32 = 2;
pub const SI_USER: i32 = 0;
pub const SI_QUEUE: i32 = -1;
pub const SI_TKILL: i32 = -6;
pub const CLD_EXITED: i32 = 1;
pub const CLD_KILLED: i32 = 2;
pub const SIGEV_SIGNAL: i32 = 0;
pub const SIGEV_NONE: i32 = 1;
pub const SIGEV_THREAD: i32 = 2;
pub const SIGEV_THREAD_ID: i32 = 4;

// ------------------------------------------------------------------------ threads
/// The kernel's rseq area (32 bytes, 32-byte aligned; version-0 layout). The critical-section
/// pointer `rseq_cs` is private and always 0: the kernel jumps to addresses it names, so safe
/// code must not set it (D-X.9). Critical sections are the runtime's, through the boundary.
#[derive(Debug)]
#[repr(C, align(32))]
#[allow(dead_code)] // rseq_cs, flags: kernel-owned fields, never read by Rust
pub struct Rseq { cpu_id_start: AtomicU32, cpu_id: AtomicU32, rseq_cs: AtomicU64, flags: AtomicU32, _pad: [u32; 3] }
impl Default for Rseq {
    fn default() -> Self { Rseq::new() }
}
impl Rseq {
    /// A fresh, unregistered area (`cpu_id` reads as `RSEQ_CPU_ID_UNINITIALIZED`).
    pub const fn new() -> Rseq {
        Rseq { cpu_id_start: AtomicU32::new(0), cpu_id: AtomicU32::new(u32::MAX), rseq_cs: AtomicU64::new(0), flags: AtomicU32::new(0), _pad: [0; 3] }
    }
    /// The CPU the thread last ran on, as the kernel updates it; `u32::MAX` before registration.
    pub fn cpu_id(&self) -> u32 { self.cpu_id.load(core::sync::atomic::Ordering::Relaxed) }
    /// `cpu_id_start`.
    pub fn cpu_id_start(&self) -> u32 { self.cpu_id_start.load(core::sync::atomic::Ordering::Relaxed) }
}
/// `struct robust_list_head`: addresses, maintained by the thread library.
#[derive(Default, Debug)]
#[repr(C)]
pub struct RobustListHead { pub list: AtomicUsize, pub futex_offset: isize, pub list_op_pending: AtomicUsize }
/// `RSEQ_CPU_ID_UNINITIALIZED`.
pub const RSEQ_CPU_ID_UNINITIALIZED: u32 = u32::MAX;

pub const FUTEX_WAIT: u32 = 0;
pub const FUTEX_WAKE: u32 = 1;
pub const FUTEX_REQUEUE: u32 = 3;
pub const FUTEX_CMP_REQUEUE: u32 = 4;
pub const FUTEX_LOCK_PI: u32 = 6;
pub const FUTEX_UNLOCK_PI: u32 = 7;
pub const FUTEX_TRYLOCK_PI: u32 = 8;
pub const FUTEX_WAIT_BITSET: u32 = 9;
pub const FUTEX_WAKE_BITSET: u32 = 10;
pub const FUTEX_WAIT_REQUEUE_PI: u32 = 11;
pub const FUTEX_CMP_REQUEUE_PI: u32 = 12;
pub const FUTEX_LOCK_PI2: u32 = 13;
pub const FUTEX_PRIVATE_FLAG: u32 = 128;
pub const FUTEX_CLOCK_REALTIME: u32 = 256;
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xffff_ffff;
pub const FUTEX_WAITERS: u32 = 0x8000_0000;
pub const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
pub const FUTEX_TID_MASK: u32 = 0x3fff_ffff;
pub const ARCH_SET_GS: u32 = 0x1001;
pub const ARCH_SET_FS: u32 = 0x1002;
pub const ARCH_GET_FS: u32 = 0x1003;
pub const ARCH_GET_GS: u32 = 0x1004;
pub const ARCH_GET_CPUID: u32 = 0x1011;
/// The rseq signature used on x86_64 (it must match the one the thread registered with).
pub const RSEQ_SIG: u32 = 0x5305_3053;
pub const RSEQ_FLAG_UNREGISTER: u32 = 1;

// ------------------------------------------------------------------------ polling, events
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct PollFd { pub fd: i32, pub events: i16, pub revents: i16 }
/// `struct epoll_event`: packed on x86_64 (12 bytes, ⚑ KERN).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C, packed)]
pub struct EpollEvent { pub events: u32, pub data: u64 }
/// A descriptor set for `pselect6` (1,024 bits, the glibc `fd_set` size of Appendix C.6).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct FdSet { pub bits: [u64; 16] }
impl FdSet {
    pub fn set(&mut self, fd: i32) {
        if let (Ok(i), true) = (usize::try_from(fd / 64), (0..1024).contains(&fd)) {
            if let Some(w) = self.bits.get_mut(i) { *w |= 1u64 << (fd % 64); }
        }
    }
    pub fn is_set(&self, fd: i32) -> bool {
        match (usize::try_from(fd / 64), (0..1024).contains(&fd)) {
            (Ok(i), true) => self.bits.get(i).is_some_and(|w| w & (1u64 << (fd % 64)) != 0),
            _ => false,
        }
    }
}
pub const POLLIN: i16 = 0x001;
pub const POLLPRI: i16 = 0x002;
pub const POLLOUT: i16 = 0x004;
pub const POLLERR: i16 = 0x008;
pub const POLLHUP: i16 = 0x010;
pub const POLLNVAL: i16 = 0x020;
pub const EPOLL_CLOEXEC: u32 = 0o2000000;
pub const EPOLL_CTL_ADD: u32 = 1;
pub const EPOLL_CTL_DEL: u32 = 2;
pub const EPOLL_CTL_MOD: u32 = 3;
pub const EPOLLIN: u32 = 0x001;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLONESHOT: u32 = 1 << 30;
pub const EFD_SEMAPHORE: u32 = 1;
pub const EFD_NONBLOCK: u32 = 0o4000;
pub const EFD_CLOEXEC: u32 = 0o2000000;
pub const IN_NONBLOCK: u32 = 0o4000;
pub const IN_CLOEXEC: u32 = 0o2000000;
pub const IN_MODIFY: u32 = 0x002;
pub const IN_CREATE: u32 = 0x100;
pub const IN_DELETE: u32 = 0x200;
pub const FAN_CLOEXEC: u32 = 0x1;
pub const FAN_NONBLOCK: u32 = 0x2;
pub const FAN_CLASS_NOTIF: u32 = 0;
pub const FAN_REPORT_FID: u32 = 0x200;
pub const FAN_MARK_ADD: u32 = 1;
pub const FAN_ACCESS: u64 = 0x1;
pub const FAN_MODIFY: u64 = 0x2;
pub const FAN_CLOSE_WRITE: u64 = 0x8;

// ------------------------------------------------------------------------ sockets
/// Storage for any socket address plus its length (the in-out `socklen_t` protocol). The
/// fields are private so the length handed to the kernel never exceeds the storage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SockAddrBuf { bytes: [u8; 128], len: u32 }
impl Default for SockAddrBuf {
    fn default() -> Self { SockAddrBuf { bytes: [0; 128], len: 0 } }
}
impl SockAddrBuf {
    /// Capacity in bytes (`sizeof(struct sockaddr_storage)`).
    pub const CAPACITY: u32 = 128;
    /// An empty buffer.
    pub fn new() -> SockAddrBuf { SockAddrBuf::default() }
    /// The address as the kernel reported it, truncated to the storage.
    pub fn bytes(&self) -> &[u8] {
        let n = usize::try_from(self.len.min(Self::CAPACITY)).unwrap_or(0);
        self.bytes.get(..n).unwrap_or(&[])
    }
    /// The full length the kernel reported (larger than [`Self::CAPACITY`] means truncated).
    pub fn reported_len(&self) -> u32 { self.len }
    /// Capacity, for the boundary.
    pub fn capacity(&self) -> u32 { Self::CAPACITY }
    /// Address of the storage, for the boundary.
    pub fn storage_addr(&mut self) -> usize { self.bytes.as_mut_ptr().expose_provenance() }
    /// Record the length the kernel wrote back (sealed use: the `Sys` implementation).
    #[doc(hidden)]
    pub fn __set_len(&mut self, len: u32) { self.len = len; }
}
pub const AF_UNSPEC: u32 = 0;
pub const AF_UNIX: u32 = 1;
pub const AF_INET: u32 = 2;
pub const AF_INET6: u32 = 10;
pub const AF_NETLINK: u32 = 16;
pub const AF_PACKET: u32 = 17;
pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const SOCK_RAW: u32 = 3;
pub const SOCK_SEQPACKET: u32 = 5;
pub const SOCK_NONBLOCK: u32 = 0o4000;
pub const SOCK_CLOEXEC: u32 = 0o2000000;
pub const SOL_SOCKET: u32 = 1;
pub const SO_REUSEADDR: u32 = 2;
pub const SO_TYPE: u32 = 3;
pub const SO_ERROR: u32 = 4;
pub const SO_SNDBUF: u32 = 7;
pub const SO_RCVBUF: u32 = 8;
pub const SO_PASSCRED: u32 = 16;
pub const SO_PEERCRED: u32 = 17;
pub const MSG_PEEK: u32 = 0x2;
pub const MSG_TRUNC: u32 = 0x20;
pub const MSG_DONTWAIT: u32 = 0x40;
pub const MSG_WAITALL: u32 = 0x100;
pub const MSG_NOSIGNAL: u32 = 0x4000;
pub const MSG_CMSG_CLOEXEC: u32 = 0x4000_0000;
pub const SHUT_RD: u32 = 0;
pub const SHUT_WR: u32 = 1;
pub const SHUT_RDWR: u32 = 2;
pub const SCM_RIGHTS: i32 = 1;
pub const NETLINK_ROUTE: u32 = 0;
pub const RTM_NEWLINK: u16 = 16;
pub const RTM_GETLINK: u16 = 18;
pub const NLM_F_REQUEST: u16 = 1;
pub const NLM_F_DUMP: u16 = 0x300;
pub const NLMSG_ERROR: u16 = 2;
pub const NLMSG_DONE: u16 = 3;

// ------------------------------------------------------------------------ IPC
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Sembuf { pub num: u16, pub op: i16, pub flg: i16 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct MqAttr { pub flags: i64, pub maxmsg: i64, pub msgsize: i64, pub curmsgs: i64, _reserved: [i64; 4] }
impl MqAttr {
    pub const fn new(maxmsg: i64, msgsize: i64) -> MqAttr {
        MqAttr { flags: 0, maxmsg, msgsize, curmsgs: 0, _reserved: [0; 4] }
    }
}
/// `struct msqid64_ds` (120 bytes); `IPC_INFO`/`MSG_INFO` results fit too.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct MsqidDs { pub words: [u64; 15] }
impl MsqidDs { pub fn qnum(&self) -> u64 { self.words[10] } }
/// `struct semid64_ds` (104 bytes).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct SemidDs { pub words: [u64; 13] }
impl SemidDs {
    /// `sem_nsems`: word 10 (after `ipc64_perm`'s six words and four time words, ⚑ KERN).
    pub fn nsems(&self) -> u64 { self.words[10] }
}
/// `struct shmid64_ds` (112 bytes); `IPC_INFO`/`SHM_INFO` results fit too.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct ShmidDs { pub words: [u64; 14] }
impl ShmidDs {
    pub fn segsz(&self) -> u64 { self.words[6] }
    pub fn nattch(&self) -> u64 { self.words[11] }
}
pub const IPC_PRIVATE: i32 = 0;
pub const IPC_CREAT: u32 = 0o1000;
pub const IPC_EXCL: u32 = 0o2000;
pub const IPC_NOWAIT: u32 = 0o4000;
pub const IPC_RMID: u32 = 0;
pub const IPC_SET: u32 = 1;
pub const IPC_STAT: u32 = 2;
pub const IPC_INFO: u32 = 3;
pub const GETVAL: u32 = 12;
pub const GETALL: u32 = 13;
pub const SETVAL: u32 = 16;
pub const SETALL: u32 = 17;
pub const SHM_RDONLY: u32 = 0o10000;
pub const SHM_REMAP: u32 = 0o40000;
pub const MSG_NOERROR: u32 = 0o10000;

// ------------------------------------------------------------------------ system
/// `struct utsname` (six 65-byte fields).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Utsname { pub sysname: [u8; 65], pub nodename: [u8; 65], pub release: [u8; 65], pub version: [u8; 65], pub machine: [u8; 65], pub domainname: [u8; 65] }
impl Default for Utsname {
    fn default() -> Self { Utsname { sysname: [0; 65], nodename: [0; 65], release: [0; 65], version: [0; 65], machine: [0; 65], domainname: [0; 65] } }
}
/// The kernel `struct termios` (TCGETS; 36 bytes), not glibc's (Appendix C, D-19.1).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct KTermios { pub iflag: u32, pub oflag: u32, pub cflag: u32, pub lflag: u32, pub line: u8, pub cc: [u8; 19] }
/// The kernel `struct termios2` (TCGETS2; 44 bytes): [`KTermios`] and the input and output
/// speeds as integers, which count when `c_cflag`'s speed codes are `BOTHER` (D-19.1).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct KTermios2 { pub iflag: u32, pub oflag: u32, pub cflag: u32, pub lflag: u32, pub line: u8, pub cc: [u8; 19], pub ispeed: u32, pub ospeed: u32 }
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Winsize { pub row: u16, pub col: u16, pub xpixel: u16, pub ypixel: u16 }

pub const GRND_NONBLOCK: u32 = 1;
pub const GRND_RANDOM: u32 = 2;
pub const GRND_INSECURE: u32 = 4;
pub const LINUX_REBOOT_MAGIC1: u32 = 0xfee1_dead;
pub const LINUX_REBOOT_MAGIC2: u32 = 672_274_793;
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0;
pub const SYSLOG_ACTION_READ_ALL: i32 = 3;
pub const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;
pub const PR_SET_PDEATHSIG: u32 = 1;
pub const PR_GET_DUMPABLE: u32 = 3;
pub const PR_SET_DUMPABLE: u32 = 4;
pub const PR_SET_NAME: u32 = 15;
pub const PR_GET_NAME: u32 = 16;
pub const PR_SET_TIMERSLACK: u32 = 29;
pub const PR_GET_TIMERSLACK: u32 = 30;
pub const PR_SET_CHILD_SUBREAPER: u32 = 36;
pub const PR_SET_NO_NEW_PRIVS: u32 = 38;
pub const PR_GET_NO_NEW_PRIVS: u32 = 39;
pub const PERSONALITY_QUERY: u32 = 0xffff_ffff;
pub const ADDR_NO_RANDOMIZE: u32 = 0x0040000;
pub const PTRACE_TRACEME: u32 = 0;
pub const PTRACE_PEEKTEXT: u32 = 1;
pub const PTRACE_PEEKDATA: u32 = 2;
pub const PTRACE_PEEKUSER: u32 = 3;
pub const PTRACE_POKETEXT: u32 = 4;
pub const PTRACE_POKEDATA: u32 = 5;
pub const PTRACE_CONT: u32 = 7;
pub const PTRACE_KILL: u32 = 8;
pub const PTRACE_SINGLESTEP: u32 = 9;
pub const PTRACE_ATTACH: u32 = 16;
pub const PTRACE_DETACH: u32 = 17;
pub const PTRACE_SYSCALL: u32 = 24;
pub const PTRACE_SETOPTIONS: u32 = 0x4200;
pub const PTRACE_SEIZE: u32 = 0x4206;
pub const PTRACE_INTERRUPT: u32 = 0x4207;
pub const PTRACE_LISTEN: u32 = 0x4208;

// ------------------------------------------------------------------------ mounting
pub const MS_RDONLY: u64 = 1;
pub const MS_NOSUID: u64 = 2;
pub const MS_NODEV: u64 = 4;
pub const MS_NOEXEC: u64 = 8;
pub const MS_REMOUNT: u64 = 32;
pub const MS_BIND: u64 = 4096;
pub const MS_MOVE: u64 = 8192;
pub const MS_REC: u64 = 16384;
pub const MS_PRIVATE: u64 = 1 << 18;
pub const MNT_FORCE: u32 = 1;
pub const MNT_DETACH: u32 = 2;
pub const MNT_EXPIRE: u32 = 4;
pub const UMOUNT_NOFOLLOW: u32 = 8;
pub const FSOPEN_CLOEXEC: u32 = 1;
pub const FSPICK_CLOEXEC: u32 = 1;
pub const FSMOUNT_CLOEXEC: u32 = 1;
pub const FSCONFIG_SET_FLAG: u32 = 0;
pub const FSCONFIG_SET_STRING: u32 = 1;
pub const FSCONFIG_CMD_CREATE: u32 = 6;
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 4;
pub const OPEN_TREE_CLONE: u32 = 1;
pub const OPEN_TREE_CLOEXEC: u32 = O_CLOEXEC;
pub const MOUNT_ATTR_RDONLY: u64 = 1;
pub const MOUNT_ATTR_NOSUID: u64 = 2;
pub const Q_SYNC: u32 = 0x80_0001;
pub const USRQUOTA: u32 = 0;

// ------------------------------------------------------------------------ auxiliary vector
pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_FLAGS: u64 = 8;
pub const AT_ENTRY: u64 = 9;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_PLATFORM: u64 = 15;
pub const AT_HWCAP: u64 = 16;
pub const AT_CLKTCK: u64 = 17;
pub const AT_SECURE: u64 = 23;
pub const AT_RANDOM: u64 = 25;
pub const AT_HWCAP2: u64 = 26;
pub const AT_EXECFN: u64 = 31;
pub const AT_SYSINFO_EHDR: u64 = 33;
pub const AT_MINSIGSTKSZ: u64 = 51;
