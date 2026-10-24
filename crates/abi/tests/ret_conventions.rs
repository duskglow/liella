//! Internal tests of the C output conventions (`liella_abi::ret`, spec C0.1): sentinels, positive
//! error numbers, and no error number on success.

#![allow(clippy::unwrap_used, missing_docs)]

use liella_abi::ret::{self, Ret};
use liella_kernel::Errno;

fn failed<T>(r: &Ret<T>) -> bool {
    r.errno.is_some_and(|e| e.get() > 0)
}

#[test]
fn neg1_conventions() {
    // Clauses: s0_6_c0_1_1, s0_6_c0_1_2 (X2, X3).
    assert_eq!(ret::zero(Ok(())), Ret { value: 0, errno: None });
    assert_eq!(ret::zero(Err(Errno::EBADF)), Ret { value: -1, errno: Some(Errno::EBADF) });
    assert_eq!(ret::int(Ok(7)), Ret { value: 7, errno: None });
    assert_eq!(ret::int(Err(Errno::EMFILE)).value, -1);
    // A value outside int: EOVERFLOW, never a truncated or negative success.
    let big = ret::int(Ok(i64::from(i32::MAX) + 1));
    assert_eq!((big.value, big.errno), (-1, Some(Errno::EOVERFLOW)));
    assert_eq!(ret::int(Ok(-5)).errno, Some(Errno::EOVERFLOW));
    assert_eq!(ret::ssize(Ok(42)), Ret { value: 42, errno: None });
    assert_eq!(ret::ssize(Ok(usize::MAX)).errno, Some(Errno::EOVERFLOW));
    assert_eq!(ret::off(Ok(u64::MAX)).value, -1);
    assert!(failed(&ret::ssize(Err(Errno::EINTR))));
}

#[test]
fn pointer_conventions() {
    // Clauses: s0_6_c0_1_1 (X2).
    assert_eq!(ret::null(Ok(0x1000)), Ret { value: 0x1000, errno: None });
    assert_eq!(ret::null(Err(Errno::ENOMEM)), Ret { value: 0, errno: Some(Errno::ENOMEM) });
}

#[test]
fn errnum_never_sets_errno() {
    // Clauses: s22_n_2, s3_1_c_6_2 (X2, X3).
    assert_eq!(ret::errnum(Ok(())), Ret { value: 0, errno: None });
    assert_eq!(ret::errnum(Err(Errno::EBUSY)), Ret { value: 16, errno: None });
}

#[test]
fn native_convention_negates_the_error() {
    // Trace: no spec clause (native C ABI result convention, negative error number); D-X.10.
    assert_eq!(ret::native(Ok(0)), 0);
    assert_eq!(ret::native(Ok(42)), 42);
    assert_eq!(ret::native(Err(Errno::ENOENT)), -2);
    assert_eq!(ret::native(Ok(u64::MAX)), -i64::from(Errno::EOVERFLOW.get()), "a success cannot look negative");
}

#[test]
fn size_and_eof_conventions() {
    // Clauses: s0_6_c0_1_1, s0_6_c0_1_2 (X2, X3).
    assert_eq!(ret::size_or_neg1(Ok(3)).errno, None);
    assert_eq!(ret::size_or_neg1(Err(Errno::EILSEQ)), Ret { value: usize::MAX, errno: Some(Errno::EILSEQ) });
    assert_eq!(ret::size_or_neg1(Ok(usize::MAX)).errno, Some(Errno::EOVERFLOW), "a success cannot look like the sentinel");
    assert_eq!(ret::eof(Ok(Some(0xff))), Ret { value: 255, errno: None }, "bytes are returned as unsigned char");
    assert_eq!(ret::eof(Ok(None)), Ret { value: -1, errno: None }, "end of file leaves errno alone");
    assert_eq!(ret::eof(Err(Errno::EIO)).errno, Some(Errno::EIO));
}
