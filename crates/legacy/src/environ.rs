//! The exported `environ` array and its operations (spec §1.3; D-1.3; finding F7).
//!
//! The array is the caller-visible truth: C programs read and replace `environ` directly, so
//! every operation works on whatever array it currently names, through [`CWords`] (the
//! boundary's view of C memory and of the C allocator). `putenv` stores the caller's own
//! string. `setenv` stores a copy it allocates; replaced copies stay allocated, as the contract
//! requires (a previous `getenv` result stays valid). When the array must grow, a new one is
//! allocated; the previous one is freed only if this module allocated it.
//!
//! Lookups take the first matching entry; `unsetenv` removes every match, so duplicate names
//! from `execve` behave as the contract says. The functions are MT-Unsafe (`env`); the
//! boundary serialises the modifying ones.

use core::sync::atomic::{AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella::error::{Errno, Result};

use crate::mb_compat::ReadAt;
use crate::search_c::CWords;

/// The array's state: the exported `environ` word, the array this module last allocated (0 if
/// none) and its capacity in pointers.
pub struct EnvState<'a> {
    /// The exported `environ` (and `__environ`, `_environ`).
    pub environ: &'a AtomicUsize,
    /// The array this module allocated, if `environ` still names it.
    pub owned: &'a AtomicUsize,
    /// Its capacity in pointers.
    pub cap: &'a AtomicUsize,
}

/// Whether `name` may be set or unset: non-empty, without `=`.
pub fn valid_name(name: &[u8]) -> bool {
    !name.is_empty() && !name.contains(&b'=')
}

/// Whether the entry `text` (`NAME=VALUE`, or at least its first `name.len() + 1` bytes) is
/// for `name`.
pub fn matches(text: &[u8], name: &[u8]) -> bool {
    text.strip_prefix(name).is_some_and(|rest| rest.first() == Some(&b'='))
}

/// Entries in the array at `env` (before its NULL).
pub fn count(env: usize, mem: &impl CWords) -> usize {
    if env == 0 {
        return 0;
    }
    (0..).take_while(|&i| mem.word(env, i) != 0).count()
}

/// The address of entry `i` (`NAME=VALUE`) of the array at `env`; `None` at or past its NULL.
pub fn entry(env: usize, i: usize, mem: &impl CWords) -> Option<usize> {
    if env == 0 {
        return None;
    }
    (0..).map(|j| mem.word(env, j)).take_while(|&p| p != 0).nth(i)
}

/// The index and address of the first entry for `name` (through [`CWords::env_find`]).
pub fn find(env: usize, name: &[u8], mem: &impl CWords) -> Option<(usize, usize)> {
    mem.env_find(env, name)
}

/// [`find`] entry by entry: each entry is read only as far as [`entry_is`] needs. The reference
/// answer for every [`CWords::env_find`].
pub fn find_scan(env: usize, name: &[u8], mem: &impl CWords) -> Option<(usize, usize)> {
    if env == 0 {
        return None;
    }
    (0..).map(|i| (i, mem.word(env, i))).take_while(|&(_, p)| p != 0).find(|&(_, p)| entry_is(p, name, mem))
}

/// Whether the entry at `p` is for `name`, reading it only as far as it can match (ENVIRON-FIND-PREFIX):
/// its first byte (most entries differ there), then the name's length and one byte more for the `=`.
/// An entry is never measured to its end, so a lookup costs the names compared, not the
/// environment's size.
pub fn entry_is(p: usize, name: &[u8], mem: &impl CWords) -> bool {
    if let Some(f) = name.first()
        && mem.text_prefix(p, 1).first() != Some(f)
    {
        return false;
    }
    name.len().checked_add(1).is_some_and(|limit| matches(mem.text_prefix(p, limit), name))
}

/// `getenv(name)`: the address of the value of the first entry for `name`.
pub fn getenv(st: &EnvState<'_>, name: &[u8], mem: &impl CWords) -> Option<usize> {
    let (_, p) = find(st.environ.load(Acquire), name, mem)?;
    p.checked_add(name.len())?.checked_add(1)
}

/// [`getenv`], reading each entry through `read` only as far as `name` and the `=` after it
/// (not to its end, as `mem.text` does): the same entry, found sooner. An entry whose first byte
/// differs from the name's is passed over after reading that byte alone (most entries are).
pub fn getenv_bounded(st: &EnvState<'_>, name: &[u8], mem: &impl CWords, read: &mut impl ReadAt<u8>) -> Option<usize> {
    let env = st.environ.load(Acquire);
    if env == 0 {
        return None;
    }
    let limit = name.len().checked_add(1)?;
    let first = name.first();
    let mut candidate = |p: usize| first.is_none_or(|f| read.read_at(p, 1).and_then(<[u8]>::first) == Some(f)) && read.read_at(p, limit).is_some_and(|t| matches(t, name));
    let p = (0..).map(|i| mem.word(env, i)).take_while(|&p| p != 0).find(|&p| candidate(p))?;
    p.checked_add(name.len())?.checked_add(1)
}

/// [`getenv_bounded`] for each of `names` in one walk over the array (SETLOCALE-3: `setlocale`
/// with `""` asks for up to fourteen names): `out[k]` becomes what `getenv_bounded` answers for
/// `names[k]` (`out` beyond `names`, names beyond `out`, and names past the 64th are left alone). The walk stops once
/// every name is found. An entry is read past its first byte only for a name that starts with it.
pub fn getenv_many(st: &EnvState<'_>, names: &[&[u8]], out: &mut [Option<usize>], mem: &impl CWords, read: &mut impl ReadAt<u8>) {
    let n = names.len().min(out.len()).min(64);
    let (names, out) = (names.get(..n).unwrap_or_default(), out.get_mut(..n).unwrap_or_default());
    out.fill(None);
    let env = st.environ.load(Acquire);
    if env == 0 {
        return;
    }
    // The first bytes of the names, as a 256-bit set (an empty name matches any entry's first byte).
    // Bit k of `done`: names[k] answered (its first entry seen); `all`: every name.
    let bit = |k: usize| u32::try_from(k).ok().and_then(|k| 1u64.checked_shl(k)).unwrap_or(0);
    let mut firsts = [0u64; 4];
    let mut any_first = false;
    let mut all = 0u64;
    for (k, name) in names.iter().enumerate() {
        all |= bit(k);
        match name.first() {
            Some(&b) => {
                if let Some(w) = firsts.get_mut(usize::from(b >> 6)) {
                    *w |= bit(usize::from(b & 63));
                }
            }
            None => any_first = true,
        }
    }
    let mut done = 0u64;
    for p in (0..).map(|i| mem.word(env, i)).take_while(|&p| p != 0) {
        if done == all {
            break;
        }
        let first = read.read_at(p, 1).and_then(<[u8]>::first).copied();
        let wanted = first.is_some_and(|b| firsts.get(usize::from(b >> 6)).is_some_and(|w| w & bit(usize::from(b & 63)) != 0));
        if !wanted && !any_first {
            continue;
        }
        for (k, (name, slot)) in names.iter().zip(out.iter_mut()).enumerate() {
            let mine = bit(k);
            if done & mine != 0 || name.first().is_some_and(|&f| Some(f) != first) {
                continue;
            }
            let Some(limit) = name.len().checked_add(1) else { continue };
            if read.read_at(p, limit).is_some_and(|t| matches(t, name)) {
                *slot = p.checked_add(limit);
                done |= mine;
            }
        }
    }
}

/// [`find`], answering on a miss the number of entries in the array (for [`append`]).
pub fn find_end(env: usize, name: &[u8], mem: &impl CWords) -> core::result::Result<(usize, usize), usize> {
    mem.env_find_end(env, name)
}

/// Append the pointer `entry` to the array `env` of `n` entries (`environ` now; [`find_end`]'s
/// count), growing into a new array if needed.
fn append(st: &EnvState<'_>, env: usize, n: usize, entry: usize, mem: &mut impl CWords) -> Result<()> {
    let need = n.checked_add(2).ok_or(Errno::ENOMEM)?;
    if env != 0 && env == st.owned.load(Relaxed) && need <= st.cap.load(Relaxed) {
        mem.set_word(env, n.saturating_add(1), 0);
        mem.set_word(env, n, entry);
        return Ok(());
    }
    let cap = need.checked_mul(2).ok_or(Errno::ENOMEM)?.max(16);
    let new = mem.alloc(cap).ok_or(Errno::ENOMEM)?;
    for i in 0..n {
        mem.set_word(new, i, mem.word(env, i));
    }
    mem.set_word(new, n, entry);
    mem.set_word(new, n.saturating_add(1), 0);
    st.environ.store(new, Release);
    let old = st.owned.swap(new, Relaxed);
    st.cap.store(cap, Relaxed);
    if old != 0 && old == env {
        mem.free(old);
    }
    Ok(())
}

/// A new `NAME=VALUE` string in memory from the C allocator.
fn make_entry(name: &[u8], value: &[u8], mem: &mut impl CWords) -> Result<usize> {
    let len = name.len().checked_add(value.len()).and_then(|n| n.checked_add(2)).ok_or(Errno::ENOMEM)?;
    let words = len.div_ceil(8);
    let p = mem.alloc(words).ok_or(Errno::ENOMEM)?;
    let bytes = name.iter().chain(b"=").chain(value.iter()).copied().chain(core::iter::repeat(0));
    let mut it = bytes;
    for i in 0..words {
        let mut word = [0u8; 8];
        for b in &mut word {
            *b = it.next().unwrap_or(0);
        }
        mem.set_word(p, i, usize::from_le_bytes(word));
    }
    Ok(p)
}

/// `setenv(name, value, overwrite)`.
pub fn setenv(st: &EnvState<'_>, name: &[u8], value: &[u8], overwrite: bool, mem: &mut impl CWords) -> Result<()> {
    setenv_at(st, name, value, overwrite, mem).map(|_| ())
}

/// [`setenv`], answering the index of the entry it replaced (the first for `name`), if it
/// replaced one rather than adding one or leaving the array alone.
pub fn setenv_at(st: &EnvState<'_>, name: &[u8], value: &[u8], overwrite: bool, mem: &mut impl CWords) -> Result<Option<usize>> {
    if !valid_name(name) {
        return Err(Errno::EINVAL);
    }
    let env = st.environ.load(Acquire);
    match find_end(env, name, mem) {
        Ok(_) if !overwrite => Ok(None),
        Ok((i, _)) => {
            let p = make_entry(name, value, mem)?;
            mem.set_word(env, i, p);
            Ok(Some(i))
        }
        Err(n) => {
            let p = make_entry(name, value, mem)?;
            append(st, env, n, p, mem).inspect_err(|_| mem.free(p)).map(|()| None)
        }
    }
}

/// The array that starts at slot `at` of the array at `env` (the same NULL ends both).
fn suffix(env: usize, at: usize) -> Option<usize> {
    at.checked_mul(size_of::<usize>()).and_then(|o| env.checked_add(o))
}

/// Remove every entry of the array at `env` for `name`, keeping the order of the others
/// (ENVIRON-UNSET-SCAN). Each match comes from [`find`] over the rest of the array (in the C
/// layer the scanner `getenv` uses), so entries are compared only there: an array without the
/// name is read once and not written, slots before the first match are not touched, and the
/// slots after it are only moved. With `string` (address, length) the name is re-read from
/// there for each search instead (`putenv`: the caller's string).
fn remove<M: CWords>(env: usize, mem: &mut M, name: &[u8], string: Option<(usize, usize)>) {
    let next = |m: &M, at: usize| {
        let name = string.map_or(name, |(a, len)| m.text_prefix(a, len));
        suffix(env, at).and_then(|s| find(s, name, m)).and_then(|(i, _)| i.checked_add(at))
    };
    if env == 0 {
        return;
    }
    let Some(first) = next(mem, 0) else {
        return;
    };
    // `w` is the slot the next kept entry moves to, `r` the next slot to read; `w < r` once an
    // entry is removed, so the slots `next` reads (from `r` on) are not yet written.
    let (mut w, mut r) = (first, first.saturating_add(1));
    loop {
        let stop = next(mem, r);
        while Some(r) != stop {
            let p = mem.word(env, r);
            if p == 0 {
                break;
            }
            mem.set_word(env, w, p);
            (w, r) = (w.saturating_add(1), r.saturating_add(1));
        }
        match stop {
            Some(s) => r = s.saturating_add(1),
            None => break,
        }
    }
    mem.set_word(env, w, 0);
}

/// `unsetenv(name)`: remove every entry for `name`.
pub fn unsetenv(st: &EnvState<'_>, name: &[u8], mem: &mut impl CWords) -> Result<()> {
    if !valid_name(name) {
        return Err(Errno::EINVAL);
    }
    remove(st.environ.load(Acquire), mem, name, None);
    Ok(())
}

/// `putenv(string)`: store the caller's `NAME=VALUE` string itself; a string without `=` removes
/// the name (glibc extension).
pub fn putenv(st: &EnvState<'_>, string: usize, mem: &mut impl CWords) -> Result<()> {
    putenv_at(st, string, mem).map(|_| ())
}

/// [`putenv`], answering the index of the entry it replaced (the first for the name), if it
/// replaced one rather than adding or removing entries.
pub fn putenv_at(st: &EnvState<'_>, string: usize, mem: &mut impl CWords) -> Result<Option<usize>> {
    let text = mem.text(string);
    let env = st.environ.load(Acquire);
    let Some(eq) = text.iter().position(|&b| b == b'=') else {
        if text.is_empty() {
            return Err(Errno::EINVAL);
        }
        // The string is re-read for each search (`remove` writes the array), as far as its
        // measured length only.
        let len = text.len();
        remove(env, mem, &[], Some((string, len)));
        return Ok(None);
    };
    let name = text.get(..eq).unwrap_or(&[]);
    if name.is_empty() {
        return Err(Errno::EINVAL);
    }
    match find_end(env, name, mem) {
        Ok((i, _)) => {
            mem.set_word(env, i, string);
            Ok(Some(i))
        }
        Err(n) => append(st, env, n, string, mem).map(|()| None),
    }
}

/// `clearenv()`: an empty environment (`environ` becomes NULL).
pub fn clearenv(st: &EnvState<'_>, mem: &mut impl CWords) {
    let env = st.environ.swap(0, AcqRel);
    let owned = st.owned.swap(0, Relaxed);
    st.cap.store(0, Relaxed);
    if owned != 0 && owned == env {
        mem.free(owned);
    }
}

