//! The C string contracts of spec §4 composed from `liella::bytes` (D-4.1, D-4.3): copying with
//! terminators, appending at the end of a terminated destination, `strl*` truncation rules,
//! `strxfrm`, and the argz/envz packed lists. Generic over the code unit (`u8`, `i32` for
//! `wchar_t`).
//!
//! Destinations whose size the operation decides are reached through [`Dest`]: the boundary
//! implements it over a C pointer (and, for fortified calls, enforces `destlen`, D-C0.5).

use alloc::vec::Vec;

use liella::bytes::{copy, copy_padded, copy_truncating, Unit};
use liella::error::{Errno, Result};

/// A destination buffer whose required length is decided by the operation.
pub trait Dest<T> {
    /// The first `n` units of the destination.
    fn get(&mut self, n: usize) -> Result<&mut [T]>;
}

/// A slice is a destination of its own length; a longer request is `ERANGE`.
impl<T> Dest<T> for [T] {
    fn get(&mut self, n: usize) -> Result<&mut [T]> {
        self.get_mut(..n).ok_or(Errno::ERANGE)
    }
}

impl<T, D: Dest<T> + ?Sized> Dest<T> for &mut D {
    fn get(&mut self, n: usize) -> Result<&mut [T]> {
        (**self).get(n)
    }
}

/// Store `src` and a terminator (`strcpy`, `wcscpy`); returns the terminator's index (the
/// `stpcpy` offset).
pub fn copy_z<T: Unit>(src: &[T], mut d: impl Dest<T>) -> Result<usize> {
    let n = src.len();
    let out = d.get(n.checked_add(1).ok_or(Errno::EOVERFLOW)?)?;
    copy(out, src);
    if let Some(t) = out.get_mut(n) {
        *t = T::ZERO;
    }
    Ok(n)
}

/// `strncpy`/`stpncpy`: exactly `n` units, zero-padded, unterminated if `src` fills them;
/// returns `min(len, n)`.
pub fn copy_n_padded<T: Unit>(src: &[T], n: usize, mut d: impl Dest<T>) -> Result<usize> {
    Ok(copy_padded(d.get(n)?, src))
}

/// `strcat`/`strncat`/`wcscat`/`wcsncat`: append `src` after the `dlen` units of text already in
/// the destination, then a terminator.
pub fn cat_z<T: Unit>(dlen: usize, src: &[T], mut d: impl Dest<T>) -> Result<()> {
    let total = dlen.checked_add(src.len()).and_then(|x| x.checked_add(1)).ok_or(Errno::EOVERFLOW)?;
    let out = d.get(total)?;
    if let Some(tail) = out.get_mut(dlen..) {
        copy_z_into(tail, src);
    }
    Ok(())
}

fn copy_z_into<T: Unit>(out: &mut [T], src: &[T]) {
    let n = copy(out, src);
    if let Some(t) = out.get_mut(n) {
        *t = T::ZERO;
    }
}

/// `strlcpy`/`wcslcpy`: copy at most `size - 1` units, terminate if `size > 0`; returns the
/// source length (the whole source is read, ⚑ HIST).
pub fn lcpy<T: Unit>(src: &[T], size: usize, mut d: impl Dest<T>) -> Result<usize> {
    if size > 0 {
        copy_truncating(d.get(size)?, src);
    }
    Ok(src.len())
}

/// `strlcat`/`wcslcat`: `dlen` is the destination text length found within `size` (`size`
/// itself if unterminated, in which case nothing is written); returns `dlen + src.len()`.
pub fn lcat<T: Unit>(dlen: usize, src: &[T], size: usize, mut d: impl Dest<T>) -> Result<usize> {
    let ret = dlen.saturating_add(src.len());
    if dlen < size {
        let out = d.get(size)?;
        if let Some(tail) = out.get_mut(dlen..) {
            copy_truncating(tail, src);
        }
    }
    Ok(ret)
}

/// `strxfrm`/`wcsxfrm` in the `"C"` locale: the key is the text itself. Writes it (with a
/// terminator) only if it fits in `n` units; returns its length either way.
pub fn xfrm<T: Unit>(src: &[T], n: usize, mut d: impl Dest<T>) -> Result<usize> {
    if src.len() < n {
        copy_z(src, &mut d)?;
    }
    Ok(src.len())
}

/// `strxfrm`/`wcsxfrm`: write `key` (with a terminator) only if it fits in `n` units; return
/// its length either way. The key is measured on a clone, then written.
pub fn xfrm_key<T: Unit, K: Iterator<Item = T> + Clone>(key: K, n: usize, mut d: impl Dest<T>) -> Result<usize> {
    let len = key.clone().count();
    if len < n {
        let out = d.get(len.checked_add(1).ok_or(Errno::EOVERFLOW)?)?;
        for (o, u) in out.iter_mut().zip(key) {
            *o = u;
        }
        if let Some(t) = out.get_mut(len) {
            *t = T::ZERO;
        }
    }
    Ok(len)
}

/// Units of a key built on the stack by [`xfrm_fill`] before the destination is touched.
const XFRM_STACK: usize = 256;

/// `strxfrm`/`wcsxfrm` in one pass (STRXFRM-EN): `fill(out)` writes the key's leading units into
/// `out` (as many as fit) and returns the key's length, or `None` having written nothing (then
/// `key()` is measured and written as [`xfrm_key`] does). As there, the destination is written
/// only if the key and its terminator fit in `n` units: the key is built in a stack buffer and
/// copied; a key longer than the buffer that still fits is built again in the destination.
pub fn xfrm_fill<T: Unit, K: Iterator<Item = T> + Clone>(n: usize, mut d: impl Dest<T>, fill: impl Fn(&mut [T]) -> Option<usize>, key: impl FnOnce() -> K) -> Result<usize> {
    if n == 0 {
        // The measuring call (`strxfrm(NULL, s, 0)`): nothing is written, so no stack buffer
        // to clear (STRXFRM-3).
        return match fill(&mut []) {
            Some(len) => Ok(len),
            None => xfrm_key(key(), n, d),
        };
    }
    let mut buf = [T::ZERO; XFRM_STACK];
    let Some(len) = fill(buf.get_mut(..n.min(XFRM_STACK)).unwrap_or_default()) else {
        return xfrm_key(key(), n, d);
    };
    if len < n {
        let out = d.get(len.checked_add(1).ok_or(Errno::EOVERFLOW)?)?;
        match (buf.get(..len), out.get_mut(..len)) {
            (Some(k), Some(o)) => o.copy_from_slice(k),
            _ => {
                fill(out);
            }
        }
        if let Some(t) = out.get_mut(len) {
            *t = T::ZERO;
        }
    }
    Ok(len)
}

// ------------------------------------------------------------------------------ argz / envz

/// The entries of an argz vector: `(start, len)` of each NUL-terminated string in `argz`.
pub fn argz_entries(argz: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut pos = 0usize;
    core::iter::from_fn(move || {
        let rest = argz.get(pos..).filter(|r| !r.is_empty())?;
        let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let start = pos;
        pos = pos.saturating_add(len).saturating_add(1);
        Some((start, len))
    })
}

fn try_vec(cap: usize) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    v.try_reserve_exact(cap).map_err(|_| Errno::ENOMEM)?;
    Ok(v)
}

fn push(v: &mut Vec<u8>, b: &[u8]) -> Result<()> {
    v.try_reserve(b.len()).map_err(|_| Errno::ENOMEM)?;
    // Capacity was reserved above, so this cannot allocate (S8).
    #[allow(clippy::disallowed_methods)]
    v.extend_from_slice(b);
    Ok(())
}

/// `argz_create`: an argz vector from a list of texts.
pub fn argz_from_list<'a>(items: impl Iterator<Item = &'a [u8]>) -> Result<Vec<u8>> {
    let mut v = try_vec(0)?;
    for it in items {
        push(&mut v, it)?;
        push(&mut v, &[0])?;
    }
    Ok(v)
}

/// `argz_create_sep`: split `text` at `sep`, dropping empty entries.
pub fn argz_from_sep(text: &[u8], sep: u8) -> Result<Vec<u8>> {
    argz_from_list(text.split(|&b| b == sep).filter(|s| !s.is_empty()))
}

/// `argz_append`/`argz_add`: `argz` followed by `more` (itself an argz vector or one entry with
/// its terminator).
pub fn argz_concat(argz: &[u8], more: &[u8]) -> Result<Vec<u8>> {
    let mut v = try_vec(argz.len().saturating_add(more.len()))?;
    push(&mut v, argz)?;
    push(&mut v, more)?;
    Ok(v)
}

/// `argz_delete`: remove the entry starting at `at`.
pub fn argz_delete(argz: &[u8], at: usize) -> Result<Vec<u8>> {
    let mut v = try_vec(argz.len())?;
    for (s, l) in argz_entries(argz) {
        if s != at {
            push(&mut v, argz.get(s..s.saturating_add(l).saturating_add(1)).unwrap_or(&[]))?;
        }
    }
    Ok(v)
}

/// `argz_insert`: insert `entry` before the entry at `before` (at the end if `before` is not an
/// entry start).
pub fn argz_insert(argz: &[u8], before: usize, entry: &[u8]) -> Result<Vec<u8>> {
    let mut v = try_vec(argz.len().saturating_add(entry.len()).saturating_add(1))?;
    let at = if argz_entries(argz).any(|(s, _)| s == before) { before } else { argz.len() };
    push(&mut v, argz.get(..at).unwrap_or(argz))?;
    push(&mut v, entry)?;
    push(&mut v, &[0])?;
    push(&mut v, argz.get(at..).unwrap_or(&[]))?;
    Ok(v)
}

/// `argz_replace`: replace every occurrence of `from` by `to` in every entry; returns the new
/// vector and the number of replacements.
pub fn argz_replace(argz: &[u8], from: &[u8], to: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut v = try_vec(argz.len())?;
    let mut count = 0usize;
    if from.is_empty() {
        push(&mut v, argz)?;
        return Ok((v, 0));
    }
    for (s, l) in argz_entries(argz) {
        let mut e = argz.get(s..s.saturating_add(l)).unwrap_or(&[]);
        while let Some(i) = liella::bytes::find_seq(e, from, |x| x) {
            push(&mut v, e.get(..i).unwrap_or(&[]))?;
            push(&mut v, to)?;
            count = count.saturating_add(1);
            e = e.get(i.saturating_add(from.len())..).unwrap_or(&[]);
        }
        push(&mut v, e)?;
        push(&mut v, &[0])?;
    }
    Ok((v, count))
}

/// `argz_stringify`: replace each terminator except the last with `sep` (in place).
pub fn argz_stringify(argz: &mut [u8], sep: u8) {
    let n = argz.len();
    for (i, b) in argz.iter_mut().enumerate() {
        if *b == 0 && i.saturating_add(1) < n {
            *b = sep;
        }
    }
}

/// `argz_next`: the entry after the one starting at `at` (`None` for the first), or `None` at
/// the end.
pub fn argz_next(argz: &[u8], at: Option<usize>) -> Option<usize> {
    match at {
        None => (!argz.is_empty()).then_some(0),
        Some(a) => {
            let rest = argz.get(a..)?;
            let next = a.saturating_add(rest.iter().position(|&b| b == 0)?).saturating_add(1);
            (next < argz.len()).then_some(next)
        }
    }
}

/// The name part of an envz entry (up to `=`, or all of it).
fn envz_name(entry: &[u8]) -> &[u8] {
    entry.split(|&b| b == b'=').next().unwrap_or(entry)
}

/// `envz_entry`: the start of the entry whose name is `name`.
pub fn envz_entry(envz: &[u8], name: &[u8]) -> Option<usize> {
    argz_entries(envz).find(|&(s, l)| envz_name(envz.get(s..s.saturating_add(l)).unwrap_or(&[])) == name).map(|(s, _)| s)
}

/// `envz_get`: the start of the value of `name`, `None` if absent or null (no `=`).
pub fn envz_get(envz: &[u8], name: &[u8]) -> Option<usize> {
    let s = envz_entry(envz, name)?;
    let e = envz.get(s..)?;
    let len = e.iter().position(|&b| b == 0).unwrap_or(e.len());
    let eq = e.get(..len)?.iter().position(|&b| b == b'=')?;
    Some(s.saturating_add(eq).saturating_add(1))
}

/// `envz_add`: set `name` to `value` (`None`: a null entry without `=`), replacing an existing
/// entry.
pub fn envz_add(envz: &[u8], name: &[u8], value: Option<&[u8]>) -> Result<Vec<u8>> {
    let base = match envz_entry(envz, name) {
        Some(s) => argz_delete(envz, s)?,
        None => argz_concat(envz, &[])?,
    };
    let mut entry = try_vec(name.len().saturating_add(value.map_or(0, <[u8]>::len)).saturating_add(2))?;
    push(&mut entry, name)?;
    if let Some(v) = value {
        push(&mut entry, b"=")?;
        push(&mut entry, v)?;
    }
    push(&mut entry, &[0])?;
    argz_concat(&base, &entry)
}

/// `envz_merge`: add every entry of `other`; existing names are replaced only with `override_`.
pub fn envz_merge(envz: &[u8], other: &[u8], override_: bool) -> Result<Vec<u8>> {
    let mut cur = argz_concat(envz, &[])?;
    for (s, l) in argz_entries(other) {
        let e = other.get(s..s.saturating_add(l)).unwrap_or(&[]);
        let name = envz_name(e);
        if envz_entry(&cur, name).is_some() && !override_ {
            continue;
        }
        let value = e.get(name.len()..).and_then(|r| r.strip_prefix(b"="));
        cur = envz_add(&cur, name, value)?;
    }
    Ok(cur)
}

/// `envz_remove`.
pub fn envz_remove(envz: &[u8], name: &[u8]) -> Result<Vec<u8>> {
    match envz_entry(envz, name) {
        Some(s) => argz_delete(envz, s),
        None => argz_concat(envz, &[]),
    }
}

/// `envz_strip`: drop null entries (those without `=`).
pub fn envz_strip(envz: &[u8]) -> Result<Vec<u8>> {
    let mut v = try_vec(envz.len())?;
    for (s, l) in argz_entries(envz) {
        let e = envz.get(s..s.saturating_add(l)).unwrap_or(&[]);
        if e.contains(&b'=') {
            push(&mut v, e)?;
            push(&mut v, &[0])?;
        }
    }
    Ok(v)
}

/// `argz_count`: the number of entries.
pub fn argz_count(argz: &[u8]) -> usize {
    argz_entries(argz).count()
}

/// `argz_add`: `argz` followed by the entry `text` (without its terminator).
pub fn argz_add(argz: &[u8], text: &[u8]) -> Result<Vec<u8>> {
    let mut v = argz_concat(argz, text)?;
    push(&mut v, &[0])?;
    Ok(v)
}

/// `argz_add_sep`: `argz` followed by the entries of `text` split at `sep`.
pub fn argz_add_sep(argz: &[u8], text: &[u8], sep: u8) -> Result<Vec<u8>> {
    argz_concat(argz, &argz_from_sep(text, sep)?)
}

/// The offset of address `entry` in a vector at address `base` of `len` bytes; `None` for a
/// NULL entry or one outside the vector.
fn offset_in(base: usize, len: usize, entry: usize) -> Option<usize> {
    if entry == 0 {
        return None;
    }
    entry.checked_sub(base).filter(|&o| o < len)
}

/// `argz_delete`: remove the entry at address `entry` (NULL or outside: unchanged).
pub fn argz_delete_addr(argz: &[u8], base: usize, entry: usize) -> Result<Vec<u8>> {
    match offset_in(base, argz.len(), entry) {
        Some(o) => argz_delete(argz, o),
        None => argz_concat(argz, &[]),
    }
}

/// `argz_insert`: insert `text` before the entry containing address `before` (moved back to
/// that entry's start); NULL appends; an address outside the vector is `EINVAL` (D-4.12).
pub fn argz_insert_addr(argz: &[u8], base: usize, before: usize, text: &[u8]) -> Result<Vec<u8>> {
    if before == 0 {
        return argz_add(argz, text);
    }
    let o = offset_in(base, argz.len(), before).ok_or(Errno::EINVAL)?;
    let start = argz.get(..o).and_then(|h| h.iter().rposition(|&b| b == 0)).map_or(0, |i| i.saturating_add(1));
    argz_insert(argz, start, text)
}

/// `argz_next`: the offset of the entry after the one at address `entry` (NULL: the first).
pub fn argz_next_addr(argz: &[u8], base: usize, entry: usize) -> Option<usize> {
    if entry == 0 {
        return argz_next(argz, None);
    }
    argz_next(argz, Some(entry.checked_sub(base)?))
}

/// `argz_extract`: the address of each entry, then a 0 (NULL), into `d`.
pub fn argz_extract(argz: &[u8], base: usize, mut d: impl Dest<usize>) -> Result<()> {
    let n = argz_count(argz);
    let out = d.get(n.checked_add(1).ok_or(Errno::EOVERFLOW)?)?;
    for (slot, (s, _)) in out.iter_mut().zip(argz_entries(argz)) {
        *slot = base.saturating_add(s);
    }
    if let Some(last) = out.get_mut(n) {
        *last = 0;
    }
    Ok(())
}

/// `argz_replace`, adding the number of replacements to `*count` if given.
pub fn argz_replace_counted(argz: &[u8], from: &[u8], to: &[u8], count: Option<&mut u32>) -> Result<Vec<u8>> {
    let (v, n) = argz_replace(argz, from, to)?;
    if let Some(c) = count {
        *c = c.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
    }
    Ok(v)
}
