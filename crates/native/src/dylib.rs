//! The Rust dylib (feature `dylib`, D-X.10): what this crate adds when it is built as
//! `libliella_rust.so.<abi>`, the Rust ABI version `<abi>` defined in `spec/abi/rust-abi.toml`
//! (docs/toolchain.md gives every build parameter and the exact commands).
//!
//! * The **Rust ABI version** this build was made as, [`RUST_ABI`], and the hash of its
//!   definition, [`RUST_ABI_HASH`]. Both are given at compile time by the environment variables
//!   `LIELLA_RUST_ABI` and `LIELLA_RUST_ABI_HASH` (`cargo xtask rust-dylib` sets them from the
//!   definition); a build without them is `unversioned`, with a zero hash, and is not a Rust ABI
//!   version. `src/ffi.rs` embeds both in the ELF note `.note.liella.rustabi` and exports the
//!   identifier as the C string `LIELLA_RUST_ABI` (the attributes that do so are unsafe, so they
//!   are written there, category *native*); this module builds their contents.
//! * The **global allocator** and the **panic handler**. A Rust dylib must define both (rustc
//!   requires them of every crate type that links), so a program linked against the dylib
//!   defines neither: its allocations are the dylib's, which are the C library's heap
//!   ([`heap::Allocator`](crate::heap::Allocator)), and a panic anywhere in it is reported on
//!   standard error and ends the process with status 101 through C `exit`.
//!
//! The dylib holds no state: the allocator and every stateful service call `libc.so.6`'s
//! native C ABI, so programs and dylibs of several Rust ABI versions in one process share one
//! heap, one environment and one set of exit handlers.

#![forbid(unsafe_code)]

/// The Rust ABI version this build was made as (`lr1`, ...; `unversioned` without
/// `LIELLA_RUST_ABI` at compile time). A program reads here the version it was compiled
/// against; the version of the library it runs with is the exported `LIELLA_RUST_ABI`.
pub const RUST_ABI: &str = match option_env!("LIELLA_RUST_ABI") {
    Some(id) => id,
    None => "unversioned",
};

/// The hash of the version's definition in `spec/abi/rust-abi.toml` (its field `hash`: 16
/// lowercase hexadecimal digits), or 16 zeros without `LIELLA_RUST_ABI_HASH` at compile time.
pub const RUST_ABI_HASH: &str = match option_env!("LIELLA_RUST_ABI_HASH") {
    Some(hash) => hash,
    None => "0000000000000000",
};

/// The most bytes an identifier may have (`lr` and a number, optionally `-` and a suffix).
pub const RUST_ABI_MAX: usize = 31;

/// The note's type in the owner namespace `Liella`: `LR` (0x4c52) in the upper 16 bits, the
/// descriptor's format (1) in the lower. Not a small number, so that `readelf` does not show it
/// as one of the generic note types (type 1 would print as `NT_VERSION`).
pub const NOTE_TYPE: u32 = 0x4c52_0001;

/// Is `id` a valid identifier: 1 to [`RUST_ABI_MAX`] bytes of `a`-`z`, `0`-`9` and `-`?
const fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    if b.is_empty() || b.len() > RUST_ABI_MAX {
        return false;
    }
    let mut rest = b;
    while let [c, tail @ ..] = rest {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-') {
            return false;
        }
        rest = tail;
    }
    true
}

/// Is `hash` 16 lowercase hexadecimal digits?
const fn valid_hash(hash: &str) -> bool {
    let b = hash.as_bytes();
    if b.len() != 16 {
        return false;
    }
    let mut rest = b;
    while let [c, tail @ ..] = rest {
        if !(c.is_ascii_digit() || (*c >= b'a' && *c <= b'f')) {
            return false;
        }
        rest = tail;
    }
    true
}

// A malformed value given at compile time stops the build.
const _: () = assert!(valid_id(RUST_ABI), "LIELLA_RUST_ABI: 1 to 31 bytes of a-z, 0-9 and -");
const _: () = assert!(valid_hash(RUST_ABI_HASH), "LIELLA_RUST_ABI_HASH: 16 lowercase hexadecimal digits");

/// The length of the exported string `LIELLA_RUST_ABI`: the identifier and its NUL.
pub(crate) const SYMBOL_LEN: usize = RUST_ABI.len() + 1;

/// The note descriptor's length: the identifier, NUL, the hash, NUL.
const DESC_LEN: usize = RUST_ABI.len() + 1 + RUST_ABI_HASH.len() + 1;

/// The descriptor's length padded to the note alignment (4).
pub(crate) const DESC_PADDED: usize = DESC_LEN.next_multiple_of(4);

/// `out` with `bytes` copied in at offset `at` (evaluated at compile time only: an index out of
/// bounds stops the build).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // i < bytes.len(), at + i < N
const fn padded<const N: usize>(bytes: &[u8], at: usize, mut out: [u8; N]) -> [u8; N] {
    let mut i = 0;
    while i < bytes.len() {
        out[at + i] = bytes[i];
        i += 1;
    }
    out
}

/// The contents of `LIELLA_RUST_ABI`: the identifier, NUL-terminated.
pub(crate) const fn symbol() -> [u8; SYMBOL_LEN] {
    padded(RUST_ABI.as_bytes(), 0, [0; SYMBOL_LEN])
}

/// An ELF note (System V gABI, "Note Section") with the owner name `Liella` (7 bytes with its
/// NUL, padded to 8) and a descriptor of `D` bytes (a multiple of 4).
#[repr(C, align(4))]
pub(crate) struct Note<const D: usize> {
    namesz: u32,
    descsz: u32,
    kind: u32,
    name: [u8; 8],
    desc: [u8; D],
}

/// `n` as the 32-bit size of a note field (at most 64 here, so the conversion is exact).
#[allow(clippy::as_conversions)] // n <= u32::MAX, asserted
const fn size32(n: usize) -> u32 {
    assert!(n <= 0xffff);
    n as u32
}

/// The note `.note.liella.rustabi`: owner `Liella`, type [`NOTE_TYPE`], descriptor the
/// identifier and the hash, each NUL-terminated (`lr1\0` then 16 hexadecimal digits and `\0`),
/// zero-padded to a multiple of 4 (the padding is not counted in `descsz`).
pub(crate) const fn note() -> Note<DESC_PADDED> {
    let desc = padded(RUST_ABI.as_bytes(), 0, [0; DESC_PADDED]);
    Note {
        namesz: size32(b"Liella\0".len()),
        descsz: size32(DESC_LEN),
        kind: NOTE_TYPE,
        name: *b"Liella\0\0",
        desc: padded(RUST_ABI_HASH.as_bytes(), SYMBOL_LEN, desc),
    }
}

/// The global allocator of every program linked against the dylib: the C library's heap.
#[cfg(not(test))]
#[global_allocator]
static HEAP: crate::heap::Allocator = crate::heap::Allocator;

/// The panic handler of every program linked against the dylib: the message on standard error,
/// then C `exit(101)` (exit handlers run, streams are flushed; nothing unwinds).
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    use core::fmt::Write;
    let mut err = crate::fd::Stdio::ERR;
    let _ = writeln!(err, "panicked (liella_native {RUST_ABI}): {info}");
    crate::process::exit(101)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn symbol_is_the_identifier_and_a_nul() {
        let s = symbol();
        assert_eq!(&s[..RUST_ABI.len()], RUST_ABI.as_bytes());
        assert_eq!(s[RUST_ABI.len()], 0);
    }

    #[test]
    fn note_follows_the_gabi_format() {
        let n = note();
        assert_eq!(n.namesz, 7);
        assert_eq!(&n.name, b"Liella\0\0");
        assert_eq!(n.kind, 0x4c52_0001);
        let len = usize::try_from(n.descsz).unwrap();
        assert_eq!(len, RUST_ABI.len() + 1 + 16 + 1);
        let mut want = Vec::new();
        want.extend_from_slice(RUST_ABI.as_bytes());
        want.push(0);
        want.extend_from_slice(RUST_ABI_HASH.as_bytes());
        want.push(0);
        assert_eq!(&n.desc[..len], want.as_slice());
        assert!(n.desc[len..].iter().all(|&b| b == 0));
        assert_eq!(DESC_PADDED % 4, 0);
        assert_eq!(core::mem::align_of::<Note<DESC_PADDED>>(), 4);
        assert_eq!(core::mem::size_of::<Note<DESC_PADDED>>(), 12 + 8 + DESC_PADDED);
    }

    #[test]
    fn identifiers_and_hashes_are_validated() {
        assert!(valid_id("lr1") && valid_id("lr999-test") && valid_id("unversioned"));
        assert!(!valid_id("") && !valid_id("LR1") && !valid_id("lr 1") && !valid_id(&"l".repeat(32)));
        assert!(valid_hash("b84ac90a5c7fa1e1") && !valid_hash("2BFF9533D0E6C65A") && !valid_hash("2bff"));
    }
}
