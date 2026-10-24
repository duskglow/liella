//! Internal tests: modern kernel calls that `Sys` covers although the POSIX layer exports no
//! wrapper for them (implementation-plan.md phase 1, D-24.6, H17). `openat2`, `clone3` and
//! `futex` are covered by the Appendix A suite; these are the rest.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::*;
use liella_kernel::Errno;

const K: CSys = CSys;

#[test]
fn sched_getattr_reports_the_policy() {
    // Trace: no spec clause (a kernel call glibc does not wrap, reached through Sys); D-24.6, D-H17.
    let a = K.sched_getattr(0, 0).unwrap();
    assert_eq!((a.size(), a.policy), (56, SCHED_OTHER));
    assert_eq!(K.sched_getattr(0, 1), Err(Errno::EINVAL), "flags must be 0");
    assert_eq!(K.sched_getattr(i32::MAX, 0), Err(Errno::ESRCH));
}

#[test]
fn sched_setattr_changes_policy_and_nice() {
    // Trace: no spec clause (a kernel call glibc does not wrap, reached through Sys); D-24.6, D-H17.
    assert_eq!(selftest::in_child(|k| {
        let mut a = SchedAttr::default();
        a.policy = SCHED_BATCH;
        a.nice = 3;
        k.sched_setattr(0, &a, 0).unwrap();
        let back = k.sched_getattr(0, 0).unwrap();
        if (back.policy, back.nice) != (SCHED_BATCH, 3) {
            return 1;
        }
        // Real-time without privilege: EPERM.
        a.policy = SCHED_FIFO;
        a.priority = 1;
        a.nice = 0;
        i32::from(k.sched_setattr(0, &a, 0) != Err(Errno::EPERM)) * 2
    }).unwrap(), ChildEnd::Exited(0));
}

#[test]
fn statmount_and_listmount() {
    // Trace: no spec clause (kernel calls glibc does not wrap, reached through Sys); D-24.6, D-H17.
    // Linux 6.8. On older kernels (this suite's reference, 5.15) both are ENOSYS.
    let mut ids = [0u64; 64];
    match K.listmount(&MntIdReq::new(LSMT_ROOT, 0), &mut ids, 0) {
        Ok(n) => {
            assert!(n >= 1);
            let mut buf = [0u8; 4096];
            K.statmount(&MntIdReq::new(ids[0], STATMOUNT_MNT_BASIC | STATMOUNT_SB_BASIC), &mut buf, 0).unwrap();
            let size = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
            assert!(usize::try_from(size).unwrap() <= buf.len() && size >= 32);
        }
        Err(e) => {
            assert_eq!(e, Errno::ENOSYS);
            let mut buf = [0u8; 512];
            assert_eq!(K.statmount(&MntIdReq::new(1, STATMOUNT_SB_BASIC), &mut buf, 0), Err(Errno::ENOSYS));
        }
    }
}
