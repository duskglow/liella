//! D-X.58: in a process the runtime did not start (this test process: another C library's
//! loader made every thread block), the block words Liella keeps at 0x9c-0x17f of a block it
//! made (flags, descriptor, locale, ctype words, lconv, h_errno, resolver state, view, cache line)
//! are that library's data. Liella reads none of them as a pointer or a decision and writes none:
//! with decoy values in all of them (bit 0 of 0x9c set, pointers aimed at a decoy area) the
//! answers stay correct and the words stay as they were. A binary with one test, so the process
//! has the harness thread and the test's thread only; nothing but Liella runs while the decoys
//! are in place, and the block is restored before anything is asserted.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella_boundary::posix::s05_ctype as ct;
use liella_boundary::posix::s07_locale as lc;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s16_nameservices as ns;
use liella_boundary::posix::s22_threads as th;
use liella_boundary::posix::s25_ipc as ipc;
use liella_boundary::{netdb, tb};
use liella_rt::tls::{ThreadBlock, BLOCK_SIZE};

/// The area the decoy pointers aim at. Zero words (a table read through a decoy gives 0, not
/// a fault), except the word at byte 32: a descriptor read through the decoy descriptor pointer
/// sees a pending, enabled cancellation there, so a cancellation-point entry that trusted the
/// block's descriptor word answers `ECANCELED` instead of reading.
static DECOY: [AtomicUsize; 512] = {
    let mut d = [const { AtomicUsize::new(0) }; 512];
    d[4] = AtomicUsize::new(4);
    d
};

/// The number of words at 0x9c-0x17f: flags, descriptor, locale, three ctype words, twelve lconv
/// words, h_errno, resolver state, view and the eight cache words.
const WORDS: usize = 29;

/// The block's words at 0x9c-0x17f, in field order (no allocation: the decoys may be in place).
fn words(b: &ThreadBlock) -> [usize; WORDS] {
    let mut v = [0; WORDS];
    let mut out = v.iter_mut();
    let mut push = |w: usize| *out.next().unwrap() = w;
    push(b.flags.load(Relaxed) as usize);
    push(b.thread.load(Relaxed));
    push(b.locale.load(Relaxed));
    [&b.ctype_b, &b.ctype_toupper, &b.ctype_tolower].into_iter().chain(&b.lconv).for_each(|w| push(w.load(Relaxed)));
    push(b.h_errno.load(Relaxed) as usize);
    push(b.res.load(Relaxed));
    push(b.view.load(Relaxed));
    b.cache.iter().for_each(|w| push(w.load(Relaxed)));
    v
}

/// Store `v` (as [`words`] returns them) into the block.
fn put(b: &ThreadBlock, v: &[usize; WORDS]) {
    let mut it = v.iter().copied();
    let mut next = || it.next().unwrap();
    b.flags.store(next() as u32, Relaxed);
    b.thread.store(next(), Relaxed);
    b.locale.store(next(), Relaxed);
    for w in [&b.ctype_b, &b.ctype_toupper, &b.ctype_tolower] {
        w.store(next(), Relaxed);
    }
    for w in &b.lconv {
        w.store(next(), Relaxed);
    }
    b.h_errno.store(next() as i32, Relaxed);
    b.res.store(next(), Relaxed);
    b.view.store(next(), Relaxed);
    for w in &b.cache {
        w.store(next(), Relaxed);
    }
}

/// What the test asks Liella, each as one word, with no allocation.
const ASKED: usize = 13;
fn ask(fds: [i32; 2]) -> [usize; ASKED] {
    let tp = tb::tp();
    let outside = |a: usize| usize::from(!(tp..tp + BLOCK_SIZE).contains(&a));
    netdb::set_h_errno(7);
    let mut byte = [0u8; 1];
    let wrote = io::write(fds[1], b"x".as_ptr().cast(), 1);
    let read = io::read(fds[0], byte.as_mut_ptr().cast(), 1);
    [
        ct::toupper(i32::from(b'a')) as usize,
        ct::tolower(i32::from(b'Q')) as usize,
        usize::from(ct::isalpha(i32::from(b'z')) != 0) + 2 * usize::from(ct::isdigit(i32::from(b'z')) != 0),
        ct::towupper(u32::from('a')) as usize,
        outside(ct::__ctype_b_loc() as usize),
        outside(ct::__ctype_toupper_loc() as usize),
        lc::uselocale(core::ptr::null_mut()) as usize,
        outside(lc::localeconv() as usize),
        netdb::h_errno() as usize,
        outside(ns::__h_errno_location() as usize),
        th::pthread_self().0 as usize,
        (wrote as usize) << 8 | read as usize,
        usize::from(byte[0]),
    ]
}

#[test]
fn foreign_block_words_neither_trusted_nor_written() {
    // Trace: no spec clause (D-X.58: a thread block another library made).
    let mut fds = [0i32; 2];
    assert_eq!(ipc::pipe(Some(&mut fds)), 0);
    std::thread::spawn(move || {
        let b = tb::current();
        let saved = words(b);
        let first = ask(fds);
        let decoy = DECOY.as_ptr() as usize;
        let table = decoy + 2048;
        // Bit 0 of the flags word set, the descriptor word aimed at the decoy descriptor, every
        // other pointer at the zero half of the decoy area, h_errno a value never stored.
        let mut planted = [table; WORDS];
        planted[0] = u32::MAX as usize;
        planted[1] = decoy;
        planted[18] = 0x5a5a;
        put(b, &planted);
        let during = ask(fds);
        let after = words(b);
        put(b, &saved);
        let expect = [
            usize::from(b'A'),
            usize::from(b'q'),
            1,
            u32::from('A') as usize,
            1,
            1,
            usize::MAX,
            1,
            7,
            1,
            first[10],
            1 << 8 | 1,
            usize::from(b'x'),
        ];
        assert_eq!(first, expect, "the answers with the block as the other library left it");
        assert_ne!(first[10], decoy, "pthread_self is Liella's descriptor");
        assert_eq!(during, expect, "the answers with decoys in the block's words");
        assert_eq!(after, planted, "no word of the other library's block was written");
        assert_eq!(DECOY.iter().map(|w| w.load(Relaxed)).filter(|&w| w != 0).collect::<Vec<_>>(), [4], "the decoy area is untouched");
    })
    .join()
    .unwrap();
    io::close(fds[0]);
    io::close(fds[1]);
}
