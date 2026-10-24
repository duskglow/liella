//! Fortify logic (C0.5, D-C0.5): a `__name_chk(..., destlen)` call builds its destination slice
//! from the compiler-known size `destlen`, so the internal function's bound check is the
//! fortify check. These helpers decide whether a request fits; the boundary reports a failure
//! through `__chk_fail` (spec §2.5).

/// The destination size to use: `n` units if they fit in `destlen` bytes of `unit`-byte units,
/// otherwise `None` (a fortify failure). `destlen == usize::MAX` means "size unknown" (the
/// compiler passes `(size_t)-1`), which always fits.
pub fn fits(n: usize, destlen: usize, unit: usize) -> Option<usize> {
    if destlen == usize::MAX {
        return Some(n);
    }
    let cap = destlen.checked_div(unit).unwrap_or(destlen);
    (n <= cap).then_some(n)
}

/// The capacity, in units, of a destination of `destlen` bytes (for the copy functions whose
/// write length the internal function decides).
pub fn capacity(destlen: usize, unit: usize) -> usize {
    destlen.checked_div(unit).unwrap_or(destlen)
}
