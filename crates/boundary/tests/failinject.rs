//! ALLOC-FAIL-INJECT (#2020): the test builds' allocation-failure hook
//! (`liella_boundary::selftest::failinject`) fails the allocations a test selects, in both
//! allocation domains (the internal heap behind the fallible collections, and the caller-owned
//! `malloc` family), and nothing once it is off.

#![allow(clippy::unwrap_used, missing_docs)]

use liella::error::Errno;
use liella::mem::{TryBox, TryVec};
use liella_boundary::selftest::failinject::{self, FailAlloc};
use std::sync::{Arc, Barrier};

#[global_allocator]
static ALLOC: FailAlloc = FailAlloc;

fn z(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

#[test]
fn failinject_nth_allocation_fails_once() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT).
    let before = failinject::injected();
    failinject::fail_nth(3);
    let a = TryBox::new(1u64);
    let b: Result<TryVec<u32>, _> = TryVec::with_capacity(4);
    let c = TryBox::new(3u64);
    let d = TryBox::new(4u64);
    failinject::off();
    assert!(a.is_ok() && b.is_ok(), "the allocations before the third succeed");
    assert_eq!(c.err(), Some(Errno::ENOMEM), "the third fails with ENOMEM");
    assert!(d.is_ok(), "only the third: the plan is spent");
    assert_eq!(failinject::injected() - before, 1);
}

#[test]
fn failinject_counts_only_real_allocations() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT).
    let mut v: TryVec<u8> = TryVec::with_capacity(16).unwrap();
    failinject::fail_nth(1);
    // Spare capacity covers these: no allocation, so the plan stays armed.
    v.reserve(8).unwrap();
    v.extend_from_slice(b"0123456789").unwrap();
    let grow = v.reserve(64);
    failinject::off();
    assert_eq!(grow, Err(Errno::ENOMEM), "the first growth that reaches the allocator fails");
    assert_eq!(v.as_slice(), b"0123456789", "a failed growth leaves the contents");
}

#[test]
fn failinject_size_threshold() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT).
    failinject::fail_at_least(4096);
    let small = TryBox::new([0u8; 64]);
    let big: Result<TryVec<u8>, _> = TryVec::with_capacity(4096);
    let big_exact = TryVec::<u64>::new().reserve_exact(1024);
    let small_again: Result<TryVec<u8>, _> = TryVec::with_capacity(4095);
    failinject::off();
    assert!(small.is_ok() && small_again.is_ok(), "below the threshold: allocated");
    assert_eq!(big.err(), Some(Errno::ENOMEM), "4096 bytes: fails");
    assert_eq!(big_exact, Err(Errno::ENOMEM), "1024 words: fails");
}

#[test]
fn failinject_caller_domain_is_null_with_enomem() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT; strdup is §4.10).
    let src = z("dup");
    let before = liella_boundary::selftest::caller_blocks();
    failinject::fail_nth(1);
    liella_boundary::errno::set_raw(0);
    let p = liella_boundary::selftest::opaque::strdup(src.as_ptr().cast());
    let e = liella_boundary::errno::get();
    failinject::off();
    assert!(p.is_null(), "strdup: NULL");
    assert_eq!(e, Errno::ENOMEM.get(), "errno ENOMEM, as for exhausted memory");
    assert_eq!(liella_boundary::selftest::caller_blocks(), before, "no block counted");
}

#[test]
fn failinject_off_again() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT).
    failinject::fail_at_least(1);
    assert_eq!(TryBox::new(7u32).err(), Some(Errno::ENOMEM), "armed: everything fails");
    failinject::off();
    let before = failinject::injected();
    for n in [1usize, 64, 4096, 1 << 20] {
        let v: Result<TryVec<u8>, _> = TryVec::with_capacity(n);
        assert!(v.is_ok(), "off: {n} bytes allocated");
    }
    let p = liella_boundary::selftest::opaque::strdup(z("after").as_ptr().cast());
    // Kept (six bytes): in a hosted test build the block is the host C library's.
    assert!(!p.is_null(), "off: the caller domain allocates");
    failinject::fail_nth(2);
    failinject::off();
    assert!(TryBox::new(1u8).is_ok() && TryBox::new(2u8).is_ok(), "off disarms an unspent plan");
    assert_eq!(failinject::injected(), before, "nothing injected while off");
}

#[test]
fn failinject_plan_is_per_thread() {
    // Trace: no spec clause (test facility, ALLOC-FAIL-INJECT).
    // The thread and the barriers exist before the plan is armed: spawning allocates.
    let armed = Arc::new(Barrier::new(2));
    let tried = Arc::new(Barrier::new(2));
    let (a, t) = (Arc::clone(&armed), Arc::clone(&tried));
    let other = std::thread::spawn(move || {
        a.wait();
        let ok = TryBox::new(5u32).is_ok();
        t.wait();
        ok
    });
    failinject::fail_at_least(1);
    armed.wait();
    let here = TryBox::new(5u32).is_ok();
    tried.wait();
    failinject::off();
    assert!(other.join().unwrap(), "another thread's allocations are not affected");
    assert!(!here, "the arming thread's are");
}
