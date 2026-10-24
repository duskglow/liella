//! The spec Appendix C tables (spec/abi/*.tsv), read at test time, and the comparisons the
//! generated layout tests make against them.

#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

const LAYOUTS: &str = include_str!("../../../../spec/abi/layouts.tsv");
const INITIALIZERS: &str = include_str!("../../../../spec/abi/initializers.tsv");

fn rows() -> impl Iterator<Item = Vec<&'static str>> {
    LAYOUTS.lines().skip(1).map(|l| l.split('\t').collect())
}

/// Size and alignment of `ty` per the table.
pub(crate) fn expect_type(ty: &str, size: usize, align: usize) {
    let row = rows().find(|f| f[0] == ty && f[1].is_empty()).unwrap_or_else(|| panic!("{ty}: not in layouts.tsv"));
    let want: (usize, usize) = (row[2].parse().unwrap(), row[3].parse().unwrap());
    assert_eq!((size, align), want, "{ty}: (size, align)");
}

/// A type whose C alignment exceeds what its size allows Rust to express: the size must match
/// and Liella's alignment must be the largest that divides it, below the C alignment.
pub(crate) fn expect_underaligned(ty: &str, size: usize, align: usize) {
    let row = rows().find(|f| f[0] == ty && f[1].is_empty()).unwrap_or_else(|| panic!("{ty}: not in layouts.tsv"));
    let (want_size, c_align): (usize, usize) = (row[2].parse().unwrap(), row[3].parse().unwrap());
    assert_eq!(size, want_size, "{ty}: size");
    assert!(want_size % c_align != 0, "{ty}: expressible in Rust; no exception needed");
    assert!(align < c_align && want_size % align == 0 && want_size % (align * 2) != 0, "{ty}: alignment {align}");
}

/// Offset of member `path` of `ty` per the table.
pub(crate) fn expect_member(ty: &str, path: &str, offset: usize) {
    let row = rows().find(|f| f[0] == ty && f[1] == path).unwrap_or_else(|| panic!("{ty}.{path}: not in layouts.tsv"));
    let want: usize = row[2].parse().unwrap();
    assert_eq!(offset, want, "{ty}.{path}: offset");
}

/// The bytes of initializer `name` of `ty` per the table.
pub(crate) fn expect_initializer(ty: &str, name: &str, bytes: &[u8]) {
    let row: Vec<&str> = INITIALIZERS
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f[0] == ty && f[1] == name)
        .unwrap_or_else(|| panic!("{ty} = {name}: not in initializers.tsv"));
    let hex = row[4];
    let want: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    assert_eq!(bytes.len(), row[2].parse::<usize>().unwrap(), "{ty} = {name}: size");
    assert_eq!(bytes, &want[..], "{ty} = {name}: bytes");
}
