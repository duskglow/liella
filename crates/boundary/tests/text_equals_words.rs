//! The fused aligned compare of a NUL-terminated text with a stored word list
//! (`strz::text_equals_words`, GETTEXT-LANGUAGE-ALIGNED-COMPARE): its answer equals
//! `strlen(text) == len && bytes equal` for every length 0..=64 at every alignment, for texts that end
//! on the last byte of a mapped page whose next page is unmapped (an extra byte or an unaligned wide
//! read would fault), for mismatches at every byte, for texts shorter or longer than the stored one,
//! and for bytes that look like a terminator to a careless word test (0x01, 0x80, 0xff, 0x0100).
#![no_builtins] // calls to libc names reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::ctypes::c_void;
use liella_boundary::posix::s04_strings as s;
use liella_boundary::strz::text_equals_words;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};

const PAGE: usize = 4096;

fn words(bytes: &[u8]) -> Vec<u64> {
    bytes
        .chunks(8)
        .map(|c| {
            let mut w = [0u8; 8];
            w[..c.len()].copy_from_slice(c);
            u64::from_le_bytes(w)
        })
        .collect()
}

/// The text `t` and a NUL at `at`.
fn put(at: usize, t: &[u8]) {
    let mut v = t.to_vec();
    v.push(0);
    s::memcpy(at as *mut c_void, v.as_ptr().cast::<c_void>(), v.len());
}

/// What the unfused code decided: the text's length is the stored one and the bytes are equal.
fn oracle(text: &[u8], stored: &[u8]) -> bool {
    !stored.is_empty() && text == stored
}

fn check(at: usize, text: &[u8], stored: &[u8], ctx: &str) {
    let w = words(stored);
    let got = text_equals_words(at, stored.len(), &|k| w.get(k).copied().unwrap_or(0));
    let want = oracle(text, stored);
    assert_eq!(got.is_some(), want, "{ctx}: text {text:?} stored {stored:?}");
    if let Some(view) = got {
        assert_eq!(view.as_ptr() as usize, at, "{ctx}");
        assert_eq!(view, stored, "{ctx}");
    }
}

fn text_of(n: usize, seed: usize) -> Vec<u8> {
    const ODD: [u8; 8] = [0x01, 0x80, 0xff, 0x7f, 0x81, 0xfe, 0x02, 0x10];
    (0..n).map(|i| if (i + seed) % 3 == 0 { ODD[(i + seed) % 8] } else { b'a' + ((i * 7 + seed) % 26) as u8 }).collect()
}

fn variants(t: &[u8]) -> Vec<Vec<u8>> {
    let mut v = vec![t.to_vec()];
    for i in 0..t.len() {
        let mut m = t.to_vec();
        m[i] = if m[i] == b'z' { b'y' } else { b'z' };
        v.push(m);
    }
    // Shorter and longer stored lists than the text.
    v.push(t[..t.len().saturating_sub(1)].to_vec());
    v.push(t[..t.len() / 2].to_vec());
    let mut longer = t.to_vec();
    longer.push(b'q');
    v.push(longer);
    // Bytes that look like a terminator to a word test.
    let mut tricky = t.to_vec();
    tricky.extend_from_slice(&[0x01, 0x80, 0xff, 0x01]);
    v.push(tricky);
    v
}

#[test]
fn matches_the_unfused_compare_at_every_length_and_alignment() {
    // Clauses: s7_7_c_4 (LANGUAGE value compare; no behaviour change, GETTEXT-LANGUAGE-ALIGNED-COMPARE).
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    let mut equal = 0usize;
    for n in 0..=64usize {
        let text = text_of(n, n);
        // Every alignment inside the page, and the text ending on the page's last byte (terminator included).
        let ats: Vec<usize> = (0..8).map(|a| base + 64 + a).chain((0..8).map(|a| base + PAGE - n - 1 - a)).collect();
        for at in ats {
            put(at, &text);
            for stored in variants(&text) {
                // A text that ends on the last byte of the page: only the exact length may be asked of
                // it, as the unfused code asks (a longer stored list mismatches at the terminator).
                check(at, &text, &stored, &format!("n={n} at%8={}", at % 8));
                equal += usize::from(oracle(&text, &stored));
            }
        }
    }
    assert!(equal > 400, "equal cases ran: {equal}");
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn a_text_with_a_word_of_odd_bytes_after_a_shorter_stored_one_is_not_equal() {
    // Clauses: s7_7_c_4 (no spec clause for the word technique itself).
    let r = CSys.mmap(0, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let base = r.addr();
    for a in 0..8usize {
        let text = [b"abcdefgh".as_slice(), &[0x01, 0x80, 0xff, 0x01, 0x80, 0xff, 0x7f, 0x81]].concat();
        put(base + a, &text);
        for len in 1..=text.len() {
            check(base + a, &text, &text[..len], &format!("prefix len={len} a={a}"));
        }
    }
    CSys.munmap(r).unwrap();
}
