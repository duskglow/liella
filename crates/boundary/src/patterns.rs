//! `fnmatch`, `glob` and `wordexp` glue (spec §14.7, §14.8, §14.10; D-14.5): the C structures
//! (`glob_t`, `wordexp_t`), the caller's callbacks and the process state over
//! `liella::pattern::{fnmatch, glob, wordexp}`.
//!
//! * Results live in caller-heap vectors (`crate::caller_alloc`): `gl_offs`/`we_offs` null
//!   slots (with `GLOB_DOOFFS`/`WRDE_DOOFFS`), the strings, a null pointer. Without `DOOFFS`
//!   or `APPEND`, `glob` sets `gl_offs` to 0 and a successful `wordexp` sets `we_offs` to 0,
//!   so the structure needs no initialisation.
//!   `globfree` frees the strings and the vector, `wordfree` the one block that holds both
//!   (WORDEXP-4); both set the vector to NULL; a NULL vector (a zeroed structure, a second
//!   call) is nothing to free.
//! * `GLOB_APPEND`/`WRDE_APPEND` copy the earlier entries into a new vector, keeping the layout
//!   the structure describes (its `gl_offs`/`we_offs`, also when this call does not pass
//!   `DOOFFS`: misuse, but never a corrupted vector), and free the old vector. A call that
//!   fails adds nothing and leaves the earlier results (POSIX `glob`: `gl_pathc` and
//!   `gl_pathv` still describe the paths matched so far), except `WRDE_NOSPACE`, which keeps the
//!   words expanded before the failure (POSIX `wordexp`). Without `APPEND`, a failed `glob`
//!   leaves `gl_pathc` 0 and `gl_pathv` NULL (or only the null slots with `GLOB_DOOFFS`); a
//!   failed `wordexp` other than `WRDE_NOSPACE` leaves the structure as it was.
//! * `gl_flags` (the flags, with `GLOB_MAGCHAR` when the pattern has metacharacters) is written
//!   only by a call that succeeds.
//! * A NULL pattern, words or structure is an error, never a crash: `glob` returns -1 with
//!   `EFAULT` (as for unknown flag bits, -1 with `EINVAL`); `wordexp` returns `WRDE_SYNTAX` with
//!   `EFAULT`; `fnmatch` returns -1.
//! * Home directories: `glob`'s `~` is `$HOME` when set and not empty, else the passwd entry of
//!   the real user ID; `wordexp`'s `~` is `$HOME` whenever it is set (XCU 2.6.1), else the
//!   passwd entry. `~user` is the passwd entry (through the name-service switch and cache).

use liella::error::{Errno, KResult};
use liella::mem::{InlineVec, TryVec};
use liella::pattern::fnmatch;
use liella::pattern::glob::{self, Access, Failure, KernelAccess};
use liella::pattern::wordexp::{self, Shell, WordError, Words};
use liella_abi::callback::{GlobErrFn, GlobT};
use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_abi::layout::wordexp_t;
use liella_kernel::sys::Sys;
use liella_legacy::nss;
use liella_legacy::versioned::GlobExistence;
use liella_legacy::nss_files::Key;

use crate::sys::CSys;
use crate::{caller_alloc, conv, errno, locale, process, spawn, strz};

/// `fnmatch(pattern, string, flags)`: 0, `FNM_NOMATCH`, or -1 (no memory, a NULL argument).
pub fn fnmatch(pattern: KResult<&[u8]>, string: KResult<&[u8]>, flags: c_int) -> c_int {
    let (Ok(p), Ok(s)) = (pattern, string) else { return -1 };
    match fnmatch::fnmatch(p, s, flags, locale::current()) {
        Ok(true) => 0,
        Ok(false) => fnmatch::NOMATCH,
        Err(_) => -1,
    }
}

/// `glob_pattern_p(pattern, quote)`.
pub fn glob_pattern_p(pattern: KResult<&[u8]>, quote: c_int) -> c_int {
    pattern.map_or(0, |p| c_int::from(glob::is_magic(p, quote != 0)))
}

/// A NUL-terminated copy of `bytes`.
fn z(bytes: &[u8]) -> KResult<TryVec<u8>> {
    let mut v = TryVec::with_capacity(bytes.len().saturating_add(1))?;
    v.extend_from_slice(bytes)?;
    v.push(0)?;
    Ok(v)
}

/// The passwd home directory for `key`.
fn passwd_home(key: Key<'_>) -> KResult<Option<TryVec<u8>>> {
    nss::found(nss::passwd_cached(&CSys, &crate::nscache::MEMORY, key, |p| TryVec::from_slice(p.dir)))?.transpose()
}

/// `$HOME` (`None` when unset).
fn home_var() -> KResult<Option<TryVec<u8>>> {
    process::getenv(b"HOME").map(|a| TryVec::from_slice(strz::env_value(a))).transpose()
}

/// `glob`'s home directories (see the module documentation).
fn glob_home(user: Option<&[u8]>) -> KResult<Option<TryVec<u8>>> {
    match user {
        Some(u) => passwd_home(Key::Name(u)),
        None => match home_var()? {
            Some(h) if !h.is_empty() => Ok(Some(h)),
            _ => passwd_home(Key::Id(CSys.getuid())),
        },
    }
}

/// The caller's `errno` after a callback failed (`EIO` if it left 0).
fn callback_errno() -> Errno {
    Errno::new(errno::get()).unwrap_or(Errno::EIO)
}

/// After a callback that succeeded: the caller's `errno` as it was before the 0 stored to detect
/// a failure (C0.1: a successful `glob` never leaves `errno` 0 that the library wrote), unless
/// the callback itself stored a value.
fn restore(saved: i32) {
    if errno::get() == 0 {
        errno::set_raw(saved);
    }
}

/// The caller's directory functions (`GLOB_ALTDIRFUNC`).
struct AltAccess<'a> {
    g: &'a GlobT,
    errfunc: Option<GlobErrFn>,
    exist: GlobExistence,
}

fn report(errfunc: Option<GlobErrFn>, path: &[u8], e: Errno) -> bool {
    let Some(f) = errfunc else { return false };
    let Ok(p) = z(path) else { return true };
    f(p.as_ptr().cast::<c_char>(), e.get()) != 0
}

impl AltAccess<'_> {
    fn stat_with(f: Option<liella_abi::callback::GlobStatFn>, path: &[u8]) -> KResult<u32> {
        let f = f.ok_or(Errno::ENOSYS)?;
        let p = z(path)?;
        // `struct stat` (144 bytes, spec Appendix C); `st_mode` is the low half of word 3.
        let mut st = [0u64; 18];
        let saved = errno::get();
        errno::set_raw(0);
        if f(p.as_ptr().cast::<c_char>(), st.as_mut_ptr().cast::<c_void>()) != 0 {
            return Err(callback_errno());
        }
        restore(saved);
        Ok(st.get(3).map_or(0, |w| u32::try_from(w & 0xffff_ffff).unwrap_or(0)))
    }
}

impl Access for AltAccess<'_> {
    type Dir = usize;

    fn open(&mut self, path: &[u8]) -> KResult<usize> {
        let f = self.g.gl_opendir.ok_or(Errno::ENOSYS)?;
        let p = z(path)?;
        let saved = errno::get();
        errno::set_raw(0);
        let h = f(p.as_ptr().cast::<c_char>()).expose_provenance();
        if h == 0 {
            return Err(callback_errno());
        }
        restore(saved);
        Ok(h)
    }

    fn read(&mut self, dir: &mut usize, name: &mut TryVec<u8>) -> KResult<Option<u8>> {
        let Some(f) = self.g.gl_readdir else { return Ok(None) };
        let e = f(core::ptr::with_exposed_provenance_mut::<c_void>(*dir)).expose_provenance();
        let Some((kind, n)) = conv::alt_dirent(e) else { return Ok(None) };
        name.clear();
        name.extend_from_slice(n)?;
        Ok(Some(kind))
    }

    fn close(&mut self, dir: usize) {
        if let Some(f) = self.g.gl_closedir {
            f(core::ptr::with_exposed_provenance_mut::<c_void>(dir));
        }
    }

    fn lstat(&mut self, path: &[u8]) -> KResult<u32> {
        // The existence check: `gl_lstat`, or `gl_stat` for `glob@GLIBC_2.2.5` (LG-2).
        Self::stat_with(self.exist.pick(self.g.gl_lstat, self.g.gl_stat), path)
    }

    fn stat(&mut self, path: &[u8]) -> KResult<u32> {
        Self::stat_with(self.g.gl_stat, path)
    }

    fn nocheck_skips_existence(&self) -> bool {
        self.exist.nocheck_skips_existence()
    }

    fn error(&mut self, path: &[u8], e: Errno) -> bool {
        report(self.errfunc, path, e)
    }

    fn home(&mut self, user: Option<&[u8]>) -> KResult<Option<TryVec<u8>>> {
        glob_home(user)
    }
}

/// Free the strings `[from, to)` of a result vector and the vector.
fn free_vector(addr: usize, from: usize, to: usize) {
    if let Some(words) = conv::result_vector(addr, to) {
        for &w in words.get(from..).unwrap_or(&[]) {
            caller_alloc::release(usize::try_from(w).unwrap_or(0));
        }
    }
    caller_alloc::release(addr);
}

/// A new result vector: `offs` null slots, the earlier entries `old` (the strings are moved),
/// copies of `new`, a null pointer. Its address; on failure nothing is left allocated.
fn build_vector<'x>(offs: usize, old: &[u64], new: impl ExactSizeIterator<Item = &'x [u8]>) -> KResult<usize> {
    let n = offs.checked_add(old.len()).and_then(|n| n.checked_add(new.len())).and_then(|n| n.checked_add(1)).ok_or(Errno::ENOMEM)?;
    let p = caller_alloc::calloc(n, 8);
    let Some(v) = conv::fresh::<usize>(p, n) else { return Err(Errno::ENOMEM) };
    let addr = p.expose_provenance();
    for (slot, &w) in v.iter_mut().skip(offs).zip(old) {
        *slot = usize::try_from(w).unwrap_or(0);
    }
    let base = offs.saturating_add(old.len());
    for (i, s) in new.enumerate() {
        match caller_alloc::dup_z(s) {
            Ok(a) => {
                if let Some(slot) = v.get_mut(base.saturating_add(i)) {
                    *slot = a;
                }
            }
            Err(e) => {
                for slot in v.iter().skip(base).take(i) {
                    caller_alloc::release(*slot);
                }
                caller_alloc::release(addr);
                return Err(e);
            }
        }
    }
    Ok(addr)
}

/// The result block of `wordexp` (WORDEXP-4, one allocation): `offs` null slots, the earlier
/// words `old` (copied: the old block is the caller's to release), the words of `got`, a null
/// pointer, then all the strings. The slots point into the same block, so `wordfree` releases
/// that one address. Its address; on failure nothing is allocated.
fn place_block(offs: usize, old: &[u64], got: &Collected) -> KResult<usize> {
    let ends = got.ends.as_slice();
    let n = offs.checked_add(old.len()).and_then(|n| n.checked_add(ends.len())).and_then(|n| n.checked_add(1)).ok_or(Errno::ENOMEM)?;
    let mut text = got.bytes.len();
    for &w in old {
        let len = conv::caller_text(usize::try_from(w).unwrap_or(0)).len();
        if w != 0 {
            text = text.checked_add(len).and_then(|t| t.checked_add(1)).ok_or(Errno::ENOMEM)?;
        }
    }
    let total = n.checked_mul(8).and_then(|t| t.checked_add(text)).ok_or(Errno::ENOMEM)?;
    let p = caller_alloc::calloc(total, 1);
    let buf = conv::fresh::<u8>(p, total).ok_or(Errno::ENOMEM)?;
    let addr = p.expose_provenance();
    let (head, tail) = buf.split_at_mut(n.saturating_mul(8));
    let base = addr.saturating_add(head.len());
    let mut slots = head.chunks_exact_mut(8).skip(offs);
    let mut pos = 0usize;
    for &w in old {
        let slot = slots.next();
        let (Some(slot), true) = (slot, w != 0) else { continue };
        let src = conv::caller_text(usize::try_from(w).unwrap_or(0));
        let end = pos.saturating_add(src.len());
        if let Some(dst) = tail.get_mut(pos..end) {
            dst.copy_from_slice(src);
        }
        slot.copy_from_slice(&base.saturating_add(pos).to_ne_bytes());
        pos = end.saturating_add(1);
    }
    let mut from = 0usize;
    for &end in ends {
        let Some(slot) = slots.next() else { break };
        let len = end.saturating_sub(from);
        if let (Some(dst), Some(src)) = (tail.get_mut(pos..pos.saturating_add(len)), got.bytes.as_slice().get(from..end)) {
            dst.copy_from_slice(src);
        }
        slot.copy_from_slice(&base.saturating_add(pos).to_ne_bytes());
        pos = pos.saturating_add(len);
        from = end;
    }
    Ok(addr)
}

fn width(v: u64) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

// ------------------------------------------------------------------------------ glob

/// `glob(pattern, flags, errfunc, pglob)` (and `glob64`): 0, a `GLOB_*` failure, or -1 with
/// `errno` (unknown flag bits: `EINVAL`; a NULL pattern or structure: `EFAULT`).
pub fn glob(pattern: KResult<&[u8]>, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>) -> c_int {
    glob_versioned(pattern, flags, errfunc, pglob, GlobExistence::Lstat)
}

/// `glob@GLIBC_2.2.5` (and `glob64@GLIBC_2.2.5`): [`glob`], except that with `GLOB_ALTDIRFUNC`
/// a name's existence is checked with `gl_stat` (`liella_legacy::versioned`, LG-2).
pub fn glob_2_2_5(pattern: KResult<&[u8]>, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>) -> c_int {
    glob_versioned(pattern, flags, errfunc, pglob, GlobExistence::for_version(true))
}

fn glob_versioned(pattern: KResult<&[u8]>, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>, exist: GlobExistence) -> c_int {
    let (Ok(pat), Some(g)) = (pattern, pglob) else {
        errno::set(Errno::EFAULT);
        return -1;
    };
    if flags & !glob::KNOWN_FLAGS != 0 {
        errno::set(Errno::EINVAL);
        return -1;
    }
    let append = flags & glob::APPEND != 0;
    if !append && flags & glob::DOOFFS == 0 {
        g.gl_offs = 0;
    }
    let loc = locale::current();
    let r = if flags & glob::ALTDIRFUNC != 0 {
        let mut acc = AltAccess { g, errfunc, exist };
        glob::glob(pat, flags, loc, &mut acc)
    } else {
        let mut on_error = |p: &[u8], e: Errno| report(errfunc, p, e);
        let mut on_home = |u: Option<&[u8]>| glob_home(u);
        let mut acc = KernelAccess { sys: &CSys, on_error: &mut on_error, on_home: &mut on_home };
        glob::glob(pat, flags, loc, &mut acc)
    };
    let offs = width(g.gl_offs);
    let found = match r {
        Ok(f) => f,
        Err(fail) => {
            if !append {
                g.gl_pathc = 0;
                g.gl_pathv = if flags & glob::DOOFFS != 0 { build_vector(offs, &[], core::iter::empty()).unwrap_or(0) } else { 0 };
            }
            return fail.code();
        }
    };
    let old_n = if append && g.gl_pathv != 0 { width(g.gl_pathc) } else { 0 };
    let old = if old_n > 0 { conv::result_vector(g.gl_pathv, offs.saturating_add(old_n)).and_then(|w| w.get(offs..)).unwrap_or(&[]) } else { &[] };
    match build_vector(offs, old, found.paths.iter().map(TryVec::as_slice)) {
        Ok(v) => {
            if append {
                caller_alloc::release(g.gl_pathv);
            }
            g.gl_pathv = v;
            g.gl_pathc = u64::try_from(old.len().saturating_add(found.paths.len())).unwrap_or(u64::MAX);
            g.gl_flags = flags | if found.magic { glob::MAGCHAR } else { 0 };
            0
        }
        Err(_) => {
            if !append {
                g.gl_pathc = 0;
                g.gl_pathv = 0;
            }
            Failure::NoSpace.code()
        }
    }
}

/// `globfree(pglob)` (and `globfree64`).
pub fn globfree(pglob: Option<&mut GlobT>) {
    let Some(g) = pglob else { return };
    if g.gl_pathv != 0 {
        let offs = width(g.gl_offs);
        free_vector(g.gl_pathv, offs, offs.saturating_add(width(g.gl_pathc)));
        g.gl_pathv = 0;
    }
}

// ------------------------------------------------------------------------------ wordexp

/// The process as word expansion sees it. `${X:=w}` assigns in `local`, an overlay that lives for
/// one `wordexp` call: later references in the same call see the value, and the environment is
/// never written (owner ruling 2026-10-03, D-14.7: expanding a string must not change the
/// caller's state; glibc sets the variable in the environment).
#[derive(Default)]
struct ProcessShell {
    local: TryVec<(TryVec<u8>, TryVec<u8>)>,
}

impl Shell for ProcessShell {
    fn var(&mut self, name: &[u8], out: &mut liella::pattern::wordexp::ValBuf) -> KResult<bool> {
        out.clear();
        if let Some((_, v)) = self.local.iter().find(|(n, _)| n.as_slice() == name) {
            out.extend_from_slice(v)?;
            return Ok(true);
        }
        match process::getenv(name) {
            Some(a) => {
                out.extend_from_slice(strz::env_value(a))?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn set_var(&mut self, name: &[u8], value: &[u8]) -> KResult<()> {
        let mut v = TryVec::with_capacity(value.len())?;
        v.extend_from_slice(value)?;
        if let Some(slot) = self.local.iter_mut().find(|(n, _)| n.as_slice() == name) {
            slot.1 = v;
            return Ok(());
        }
        let mut n = TryVec::with_capacity(name.len())?;
        n.extend_from_slice(name)?;
        self.local.push((n, v))
    }

    fn arg_count(&mut self) -> usize {
        process::arg_count().saturating_sub(1)
    }

    fn arg(&mut self, i: usize, out: &mut liella::pattern::wordexp::ValBuf) -> KResult<bool> {
        out.clear();
        process::arg_bytes(i, out)
    }

    fn pid(&mut self) -> i32 {
        CSys.getpid()
    }

    fn home(&mut self, user: Option<&[u8]>, out: &mut liella::pattern::wordexp::ValBuf) -> KResult<bool> {
        out.clear();
        let h = match user {
            Some(u) => passwd_home(Key::Name(u))?,
            None => {
                // `$HOME` straight into `out` (no copy in between).
                if let Some(a) = process::getenv(b"HOME") {
                    out.extend_from_slice(strz::env_value(a))?;
                    return Ok(true);
                }
                passwd_home(Key::Id(CSys.getuid()))?
            }
        };
        match h {
            Some(h) => {
                out.extend_from_slice(&h)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn run(&mut self, command: &[u8], show_err: bool, out: &mut TryVec<u8>) -> KResult<()> {
        spawn::capture_shell(&z(command)?, show_err, out)
    }

    fn stderr(&mut self, text: &[u8]) {
        process::stderr(text);
    }

    fn glob(&mut self, pattern: &[u8], out: &mut TryVec<TryVec<u8>>) -> KResult<()> {
        let mut on_error = |_: &[u8], _: Errno| false;
        let mut on_home = |u: Option<&[u8]>| glob_home(u);
        let mut acc = KernelAccess { sys: &CSys, on_error: &mut on_error, on_home: &mut on_home };
        match glob::glob(pattern, 0, locale::current(), &mut acc) {
            Ok(f) => {
                *out = f.paths;
                Ok(())
            }
            Err(Failure::NoSpace) => Err(Errno::ENOMEM),
            Err(_) => Ok(()),
        }
    }
}

/// The fields of one call: their bytes, each with its terminating zero, back to back (inline
/// for a usual call), and where each ends. The result block is made from them in one go
/// ([`place_block`]); dropped before that (a failure), there is nothing to free.
struct Collected {
    bytes: InlineVec<u8, 256>,
    ends: InlineVec<usize, 16>,
}

impl Collected {
    #[inline(always)]
    fn new() -> Collected {
        Collected { bytes: InlineVec::new(), ends: InlineVec::new() }
    }
}

impl Words for Collected {
    fn push(&mut self, word: &[u8]) -> KResult<()> {
        self.bytes.extend_from_slice(word)?;
        self.bytes.push(0)?;
        // A failure here leaves stray bytes past the last end: they belong to no word.
        self.ends.push(self.bytes.len())
    }
}

/// `wordexp(words, we, flags)`: 0 or a `WRDE_*` failure.
pub fn wordexp(words: KResult<&[u8]>, we: Option<&mut wordexp_t>, flags: c_int) -> c_int {
    let (Ok(text), Some(w)) = (words, we) else {
        errno::set(Errno::EFAULT);
        return WordError::Syntax.code();
    };
    if flags & wordexp::REUSE != 0 {
        wordfree(Some(w));
        w.we_wordc = 0;
    }
    let mut got = Collected::new();
    let r = wordexp::wordexp(text, flags, locale::current(), &mut ProcessShell::default(), &mut got);
    if let Err(e) = r {
        if e != WordError::NoSpace {
            return e.code();
        }
    }
    let append = flags & wordexp::APPEND != 0 && w.we_wordv != 0;
    let offs = if flags & (wordexp::DOOFFS | wordexp::APPEND) != 0 { width(w.we_offs) } else { 0 };
    let old_n = if append { width(w.we_wordc) } else { 0 };
    let old = if old_n > 0 { conv::result_vector(w.we_wordv, offs.saturating_add(old_n)).and_then(|v| v.get(offs..)).unwrap_or(&[]) } else { &[] };
    match place_block(offs, old, &got) {
        Ok(v) => {
            let added = got.ends.len();
            if append {
                caller_alloc::release(w.we_wordv);
            }
            w.we_wordv = v;
            // The layout `wordfree` reads: without DOOFFS or APPEND, no null slots (an
            // uninitialised `we_offs` would otherwise free from the wrong slots).
            if flags & (wordexp::DOOFFS | wordexp::APPEND) == 0 {
                w.we_offs = 0;
            }
            w.we_wordc = u64::try_from(old.len().saturating_add(added)).unwrap_or(u64::MAX);
            r.map_or_else(WordError::code, |()| 0)
        }
        Err(_) => WordError::NoSpace.code(),
    }
}

/// `wordfree(we)`.
pub fn wordfree(we: Option<&mut wordexp_t>) {
    let Some(w) = we else { return };
    if w.we_wordv != 0 {
        // One block holds the vector and the strings (`place_block`).
        caller_alloc::release(w.we_wordv);
        w.we_wordv = 0;
    }
}
