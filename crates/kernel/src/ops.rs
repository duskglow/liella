//! Typed command sets for the multiplexed system calls (`fcntl`, `ioctl`, `prctl`, `madvise`,
//! `ptrace`, `quotactl`, `semctl`, `arch_prctl`).
//!
//! A multiplexed call interprets its argument as an integer or as an address depending on
//! the command. `Sys` accepts only the commands listed here, each with an argument of the
//! right type, so no command can make the kernel write through an integer the caller chose
//! (decision D-X.9). Commands not listed go through `liella_boundary::raw`. The numbers are
//! the kernel's (⚑ KERN).

macro_rules! commands {
    ($(#[$m:meta])* $name:ident : $repr:ty { $($(#[$vm:meta])* $v:ident = $n:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum $name { $($(#[$vm])* #[doc = concat!("Command `", stringify!($n), "`.")] $v),* }
        impl $name {
            /// The kernel's command number.
            pub const fn code(self) -> $repr {
                match self { $($name::$v => $n),* }
            }
            /// The command with kernel number `n`, if it is one of this set.
            pub const fn from_code(n: $repr) -> Option<$name> {
                match n { $($n => Some($name::$v),)* _ => None }
            }
        }
    };
}

commands!(
    /// `fcntl` commands whose argument (if any) is an integer and whose result is an integer.
    FcntlInt: u32 {
        DupFd = 0, GetFd = 1, SetFd = 2, GetFl = 3, SetFl = 4, SetOwn = 8, GetOwn = 9,
        SetSig = 10, GetSig = 11, SetLease = 1024, GetLease = 1025, Notify = 1026,
        DupFdCloexec = 1030, SetPipeSz = 1031, GetPipeSz = 1032, AddSeals = 1033, GetSeals = 1034,
    }
);

commands!(
    /// `fcntl` record-lock commands (argument: `struct flock`).
    FcntlLock: u32 { GetLk = 5, SetLk = 6, SetLkw = 7, OfdGetLk = 36, OfdSetLk = 37, OfdSetLkw = 38 }
);

commands!(
    /// `fcntl` read/write-hint commands (argument: a `u64`).
    FcntlHint: u32 { GetRwHint = 1035, SetRwHint = 1036, GetFileRwHint = 1037, SetFileRwHint = 1038 }
);

commands!(
    /// `ioctl` requests without an argument.
    IoctlNone: u32 { Fioclex = 0x5451, Fionclex = 0x5450, Tiocnotty = 0x5422, Tiocexcl = 0x540C, Tiocnxcl = 0x540D }
);

commands!(
    /// `ioctl` requests taking an integer by value.
    IoctlIntArg: u32 {
        Tcsbrk = 0x5409, Tcxonc = 0x540A, Tcflsh = 0x540B, Tiocsctty = 0x540E, Tcsbrkp = 0x5425,
        /// Returns a new descriptor for the pty peer.
        Tiocgptpeer = 0x5441,
    }
);

commands!(
    /// `ioctl` requests that write one `int` through their argument.
    IoctlIntOut: u32 {
        Fionread = 0x541B, Tiocoutq = 0x5411, Tiocgpgrp = 0x540F, Tiocgsid = 0x5429,
        Tiocmget = 0x5415, Tiocgetd = 0x5424, Tiocgptn = 0x8004_5430, Tiocgpkt = 0x8004_5438,
        Tiocgptlck = 0x8004_5439, Tiocgexcl = 0x8004_5440,
    }
);

commands!(
    /// `ioctl` requests that read one `int` through their argument.
    IoctlIntIn: u32 {
        Tiocspgrp = 0x5410, Fionbio = 0x5421, Fioasync = 0x5452, Tiocpkt = 0x5420,
        Tiocmset = 0x5418, Tiocmbis = 0x5416, Tiocmbic = 0x5417, Tiocsetd = 0x5423,
        Tiocsptlck = 0x4004_5431,
    }
);

commands!(
    /// Terminal attribute setters (argument: the kernel `struct termios`).
    TcSet: u32 { Now = 0x5402, Drain = 0x5403, Flush = 0x5404 }
);

commands!(
    /// Terminal attribute setters with integer speeds (argument: the kernel `struct termios2`).
    TcSet2: u32 { Now = 0x402C_542B, Drain = 0x402C_542C, Flush = 0x402C_542D }
);

commands!(
    /// `prctl` options whose arguments are integers and whose result is the return value.
    PrctlInt: u32 {
        SetPdeathsig = 1, GetDumpable = 3, SetDumpable = 4, GetKeepcaps = 7, SetKeepcaps = 8,
        CapbsetRead = 23, CapbsetDrop = 24, GetSecurebits = 27, SetSecurebits = 28,
        GetSeccomp = 21, SetTimerslack = 29, GetTimerslack = 30, SetChildSubreaper = 36, SetNoNewPrivs = 38,
        GetNoNewPrivs = 39, SetThpDisable = 41, GetThpDisable = 42, CapAmbient = 47,
        GetSpeculationCtrl = 52, SetTsc = 26,
    }
);

commands!(
    /// `prctl` options that write one `int` through their second argument.
    PrctlIntOut: u32 { GetPdeathsig = 2, GetChildSubreaper = 37, GetTsc = 25 }
);

commands!(
    /// `madvise` advice that cannot change memory a live reference may see.
    ///
    /// Excluded (they go through `raw`): `MADV_FREE` (contents may change between two reads),
    /// `MADV_REMOVE`, `MADV_DONTFORK` (a forked child would fault on its copy of a live
    /// reference), `MADV_HWPOISON` and `MADV_SOFT_OFFLINE`.
    Advice: u32 {
        Normal = 0, Random = 1, Sequential = 2, WillNeed = 3,
        /// On a region, which `Sys` only accepts by `&mut`: no reference can observe the zeroing.
        DontNeed = 4,
        DoFork = 11, Mergeable = 12, Unmergeable = 13, HugePage = 14, NoHugePage = 15,
        DontDump = 16, DoDump = 17, WipeOnFork = 18, KeepOnFork = 19, Cold = 20, PageOut = 21,
        PopulateRead = 22, PopulateWrite = 23, DontNeedLocked = 24, Collapse = 25,
    }
);

commands!(
    /// `process_madvise` advice (non-destructive by kernel rule).
    RemoteAdvice: u32 { WillNeed = 3, Cold = 20, PageOut = 21, Collapse = 25 }
);

commands!(
    /// `ptrace` requests whose `addr` and `data` are integers (or addresses in the tracee).
    /// `POKE*` and register setters change the tracee's memory and go through `raw`: a tracee
    /// that shares this process's memory would make them arbitrary writes.
    PtraceOp: u32 {
        Traceme = 0, Cont = 7, Kill = 8, Singlestep = 9, Attach = 16, Detach = 17, Syscall = 24,
        SetOptions = 0x4200, Seize = 0x4206, Interrupt = 0x4207, Listen = 0x4208,
    }
);

commands!(
    /// `ptrace` requests that read one word from the tracee into the caller.
    PtracePeek: u32 { Text = 1, Data = 2, User = 3, GetEventMsg = 0x4201 }
);

commands!(
    /// `quotactl` sub-commands with an optional fixed-size buffer.
    QuotaCmd: u32 {
        Sync = 0x80_0001, QuotaOff = 0x80_0003, GetFmt = 0x80_0004, GetInfo = 0x80_0005,
        SetInfo = 0x80_0006, GetQuota = 0x80_0007, SetQuota = 0x80_0008, GetNextQuota = 0x80_0009,
    }
);

impl QuotaCmd {
    /// Bytes the kernel reads or writes through the buffer (⚑ KERN: `if_dqblk` 72,
    /// `if_nextdqblk` 76, `if_dqinfo` 24, the format `u32`).
    pub const fn buffer_len(self) -> usize {
        match self {
            QuotaCmd::Sync | QuotaCmd::QuotaOff => 0,
            QuotaCmd::GetFmt => 4,
            QuotaCmd::GetInfo | QuotaCmd::SetInfo => 24,
            QuotaCmd::GetQuota | QuotaCmd::SetQuota => 72,
            QuotaCmd::GetNextQuota => 80,
        }
    }
}

commands!(
    /// `semctl` commands with an integer argument (or none).
    SemCtlInt: u32 { Rmid = 0, GetPid = 11, GetVal = 12, GetNcnt = 14, GetZcnt = 15, SetVal = 16 }
);

commands!(
    /// `semctl` commands whose argument is a `semid64_ds` (or the smaller `seminfo`).
    SemCtlDs: u32 { Set = 1, Stat = 2, Info = 3, SemStat = 18, SemInfo = 19, SemStatAny = 20 }
);

commands!(
    /// `msgctl` commands (argument: `msqid64_ds`, or the smaller `msginfo`).
    MsgCtl: u32 { Rmid = 0, Set = 1, Stat = 2, Info = 3, MsgStat = 11, MsgInfo = 12, MsgStatAny = 13 }
);

commands!(
    /// `shmctl` commands (argument: `shmid64_ds`, or the smaller `shminfo64` / `shm_info`).
    ShmCtl: u32 {
        Rmid = 0, Set = 1, Stat = 2, Info = 3, Lock = 11, Unlock = 12, ShmStat = 13, ShmInfo = 14,
        ShmStatAny = 15,
    }
);

commands!(
    /// `arch_prctl` queries that write one word (`ARCH_GET_FS`, `ARCH_GET_GS`).
    // ARCH(x86_64): ARCH_GET_FS/GS codes, x86_64 only (A-3)
    ArchGet: u32 { Fs = 0x1003, Gs = 0x1004 }
);

commands!(
    /// `fsconfig` commands.
    FsconfigCmd: u32 {
        SetFlag = 0, SetString = 1, SetBinary = 2, SetPath = 3, SetPathEmpty = 4, SetFd = 5,
        CmdCreate = 6, CmdReconfigure = 7, CmdCreateExcl = 8,
    }
);

commands!(
    /// `setsockopt` levels whose options never make the kernel keep a pointer into this
    /// process for later writes. Other levels (`SOL_XDP`, `SOL_RDS`, ...) register user memory
    /// for the kernel or devices to write, and go through `raw`.
    SockLevel: u32 {
        Ip = 0, Socket = 1, Tcp = 6, Udp = 17, Ipv6 = 41, Raw = 255, Packet = 263, Netlink = 270,
        Alg = 279, Tls = 282,
    }
);
