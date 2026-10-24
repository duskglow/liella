//! Internal tests: the kernel structures of `liella_kernel::uapi` against the glibc types of
//! spec Appendix C (`liella_abi::layout`) wherever the two are the same ABI on x86_64 (the C
//! library passes them to the kernel unconverted). Where the kernel type is held as words, the
//! accessors are checked to read the word at the glibc member's offset.

#![allow(clippy::arithmetic_side_effects, clippy::identity_op, clippy::drop_non_drop, clippy::bool_assert_comparison, clippy::erasing_op, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::mem::{align_of, offset_of, size_of};

use liella_abi::layout as c;
use liella_kernel::uapi as k;

macro_rules! same_size {
    ($($kt:ty => $ct:ty),* $(,)?) => {
        #[test]
        fn sizes_and_alignments_match_the_c_types() {
            // Trace: no spec clause (kernel uapi types against the Appendix C layouts); D-H6, D-H17.
            $(
                assert_eq!(size_of::<$kt>(), size_of::<$ct>(), "size of {} vs {}", stringify!($kt), stringify!($ct));
                assert!(align_of::<$kt>() >= align_of::<$ct>().min(8), "align of {}", stringify!($kt));
            )*
        }
    };
}

same_size! {
    k::Timespec => c::struct_timespec,
    k::Timeval => c::struct_timeval,
    k::Timezone => c::struct_timezone,
    k::Itimerval => c::struct_itimerval,
    k::Itimerspec => c::struct_itimerspec,
    k::Tms => c::struct_tms,
    k::Timex => c::struct_timex,
    k::Stat => c::struct_stat,
    k::Statx => c::struct_statx,
    k::Statfs => c::struct_statfs,
    k::Rusage => c::struct_rusage,
    k::Rlimit => c::struct_rlimit,
    k::SchedParam => c::struct_sched_param,
    k::Sysinfo => c::struct_sysinfo,
    k::SigInfo => c::siginfo_t,
    k::StackT => c::stack_t,
    k::SigEvent => c::struct_sigevent,
    k::SignalfdSiginfo => c::struct_signalfd_siginfo,
    k::Rseq => c::struct_rseq,
    k::PollFd => c::struct_pollfd,
    k::EpollEvent => c::struct_epoll_event,
    k::FdSet => c::fd_set,
    k::Sembuf => c::struct_sembuf,
    k::MqAttr => c::struct_mq_attr,
    k::MsqidDs => c::struct_msqid_ds,
    k::SemidDs => c::struct_semid_ds,
    k::ShmidDs => c::struct_shmid_ds,
    k::Utsname => c::struct_utsname,
    k::Winsize => c::struct_winsize,
}

#[test]
fn stat_members() {
    // Trace: no spec clause (kernel uapi types against the Appendix C layouts); D-H6, D-H17.
    assert_eq!(offset_of!(k::Stat, mode), offset_of!(c::struct_stat, st_mode));
    assert_eq!(offset_of!(k::Stat, size), offset_of!(c::struct_stat, st_size));
    assert_eq!(offset_of!(k::Stat, atime), offset_of!(c::struct_stat, st_atim));
    assert_eq!(offset_of!(k::Stat, ctime), offset_of!(c::struct_stat, st_ctim));
}

#[test]
fn statx_members() {
    // Trace: no spec clause (kernel uapi types against the Appendix C layouts); D-H6, D-H17.
    assert_eq!(offset_of!(k::Statx, mode), offset_of!(c::struct_statx, stx_mode));
    assert_eq!(offset_of!(k::Statx, ino), offset_of!(c::struct_statx, stx_ino));
    assert_eq!(offset_of!(k::Statx, btime), offset_of!(c::struct_statx, stx_btime));
    assert_eq!(offset_of!(k::Statx, mnt_id), offset_of!(c::struct_statx, stx_mnt_id));
}

#[test]
fn timex_and_sysinfo_members() {
    // Trace: no spec clause (kernel uapi types against the Appendix C layouts); D-H6, D-H17.
    assert_eq!(offset_of!(k::Timex, tick), offset_of!(c::struct_timex, tick));
    assert_eq!(offset_of!(k::Timex, tai), offset_of!(c::struct_timex, tai));
    assert_eq!(offset_of!(k::Sysinfo, procs), offset_of!(c::struct_sysinfo, procs));
    assert_eq!(offset_of!(k::Sysinfo, mem_unit), offset_of!(c::struct_sysinfo, mem_unit));
}

/// Words `0, 1, 2, ...` so an accessor's result names the word it read.
fn numbered<const N: usize>() -> [u64; N] {
    core::array::from_fn(|i| i as u64)
}

#[test]
fn ipc_accessors_read_the_c_members() {
    // Trace: no spec clause (kernel uapi word accessors against the Appendix C layouts); D-H6, D-H17.
    let sem = k::SemidDs { words: numbered() };
    assert_eq!(sem.nsems() as usize * 8, offset_of!(c::struct_semid_ds, sem_nsems));
    let msq = k::MsqidDs { words: numbered() };
    assert_eq!(msq.qnum() as usize * 8, offset_of!(c::struct_msqid_ds, msg_qnum));
    let shm = k::ShmidDs { words: numbered() };
    assert_eq!(shm.segsz() as usize * 8, offset_of!(c::struct_shmid_ds, shm_segsz));
    assert_eq!(shm.nattch() as usize * 8, offset_of!(c::struct_shmid_ds, shm_nattch));
}

#[test]
fn siginfo_accessors_read_the_c_members() {
    // Trace: no spec clause (kernel uapi word accessors against the Appendix C layouts); D-H6, D-H17.
    let mut s = k::SigInfo::queued(10, 1234, 5678, 0xfeed);
    assert_eq!(offset_of!(k::SigInfo, code), offset_of!(c::siginfo_t, si_code));
    // si_pid and si_uid of the kill/rt layout; si_value at the rt sigval.
    assert_eq!(16 + 0, offset_of!(c::siginfo_t, _sifields._rt.si_pid));
    assert_eq!(16 + 4, offset_of!(c::siginfo_t, _sifields._rt.si_uid));
    assert_eq!(16 + 8, offset_of!(c::siginfo_t, _sifields._rt.si_sigval));
    assert_eq!((s.pid(), s.uid(), s.value()), (1234, 5678, 0xfeed));
    s.fields[1] = 0x0000_0007_0000_0003;
    assert_eq!(16 + 8, offset_of!(c::siginfo_t, _sifields._sigchld.si_status));
    assert_eq!(s.status(), 3);
}

#[test]
fn signalfd_siginfo_accessors() {
    // Trace: no spec clause (kernel uapi word accessors against the Appendix C layouts); D-H6, D-H17.
    let mut s = k::SignalfdSiginfo::default();
    s.words[0] = 17;
    s.words[3] = 4321;
    assert_eq!(offset_of!(c::struct_signalfd_siginfo, ssi_pid), 12);
    assert_eq!((s.signo(), s.pid()), (17, 4321));
}

/// The C shapes `liella_legacy` defines for itself (phase 5), because the ABI crate's versions
/// model a member as a union only `unsafe` code may read, or because the type is compat-only:
/// they must lay out exactly as the C definitions.
#[test]
fn legacy_c_shapes_match_the_c_layouts() {
    // Trace: no spec clause (liella-legacy C shapes against the Appendix C layouts); D-H6, C5.
    use liella_legacy::signal_compat::{CSigaction, CSigstack};
    assert_eq!((size_of::<CSigaction>(), align_of::<CSigaction>()), (size_of::<c::struct_sigaction>(), align_of::<c::struct_sigaction>()));
    assert_eq!(offset_of!(CSigaction, mask), offset_of!(c::struct_sigaction, sa_mask));
    assert_eq!(offset_of!(CSigaction, flags), offset_of!(c::struct_sigaction, sa_flags));
    assert_eq!(offset_of!(CSigaction, restorer), offset_of!(c::struct_sigaction, sa_restorer));
    assert_eq!((size_of::<CSigstack>(), align_of::<CSigstack>()), (size_of::<c::struct_sigstack>(), align_of::<c::struct_sigstack>()));
    assert_eq!(offset_of!(CSigstack, onstack), offset_of!(c::struct_sigstack, ss_onstack));
    // struct sigvec (4.2BSD, compat; no installed header): handler, int mask, int flags.
    assert_eq!(size_of::<liella_legacy::signal_compat::CSigvec>(), 16);
    // The old struct ntptimeval that ntp_gettime fills: the 2.12 one's first three members.
    use liella_legacy::clock_compat::NtpTimevalOld;
    assert_eq!(size_of::<NtpTimevalOld>(), offset_of!(c::struct_ntptimeval, tai));
    assert_eq!(offset_of!(NtpTimevalOld, esterror), offset_of!(c::struct_ntptimeval, esterror));
    // struct vtimes (compat): ten 4-byte members.
    assert_eq!(size_of::<liella_legacy::sys_compat::Vtimes>(), 40);
}
