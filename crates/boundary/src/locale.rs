//! Locale glue (spec §7; D-7.7, D-7.8): the calling thread's current locale, `locale_t`
//! objects, the per-thread table pointers of `__ctype_b_loc` and its siblings, and the file
//! access `liella_legacy::global_locale` loads locales through.
//!
//! A `locale_t` is a heap block laid out as `struct __locale_struct` (29 words): `__locales`
//! (13 words; each holds the interned locale's handle, D-7.8), `__ctype_b`, `__ctype_tolower`,
//! `__ctype_toupper` (pointers into the `LC_CTYPE` tables, which header macros read), and
//! `__names` (13 pointers to the categories' names).

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::locale::file::Cat;
use liella::ctype::Class;
use liella::locale::view::LocaleView;
use liella::locale::{Ctype, Item, Locale};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_PRIVATE, O_CLOEXEC, O_DIRECTORY, O_RDONLY, PROT_READ};
use liella_legacy::ctype_tables::{self, C_CLASS, C_TOLOWER, C_TOUPPER, ZERO};
use liella_legacy::global_locale::{self as gl, Mapper, Objects};

use crate::sys::CSys;
use crate::{conv, heap, process, strz, tb};

/// `LC_GLOBAL_LOCALE`: `(locale_t) -1`.
pub const GLOBAL: usize = usize::MAX;

/// The POSIX layer's side of locale loading.
pub struct Glue;

impl Objects for Glue {
    fn view_at(&self, addr: usize) -> &'static LocaleView {
        tb::view_at(addr)
    }

    fn bytes_at(&self, addr: usize, len: usize) -> &'static [u8] {
        if len == 0 { &[] } else { conv::mapped_bytes(addr, len) }
    }

    fn charmap_at(&self, addr: usize) -> &'static liella::charmap::Charmap {
        tb::charmap_at(addr)
    }
}

/// `f` with `path` as a kernel path (NUL-terminated in a buffer).
fn with_path<T>(path: &[u8], f: impl FnOnce(KStr<'_>) -> Option<T>) -> Option<T> {
    let mut buf = [0u8; 1024];
    buf.get_mut(..path.len())?.copy_from_slice(path);
    *buf.get_mut(path.len())? = 0;
    f(KStr::from_nul_terminated(buf.get(..=path.len())?).ok()?)
}

/// A file's identity for `global_locale` (D-X.13): device, inode, size, and the modification
/// and change times.
fn identity_of(st: &liella_kernel::uapi::Stat) -> gl::Identity {
    let u = |v: i64| u64::from_ne_bytes(v.to_ne_bytes());
    [st.dev, st.ino, u(st.size), u(st.mtime.sec), u(st.mtime.nsec), u(st.ctime.sec), u(st.ctime.nsec)]
}

impl Mapper for Glue {
    fn map(&self, path: &[u8]) -> Option<(usize, usize, gl::Identity)> {
        let fd = with_path(path, |p| CSys.openat(Fd::CWD, p, O_RDONLY | O_CLOEXEC, 0).ok())?;
        let mapped = (|| {
            let st = CSys.fstat(fd).ok()?;
            let size = usize::try_from(st.size).ok()?;
            if size == 0 {
                return Some((0, 0, identity_of(&st)));
            }
            let region = CSys.mmap(0, size, PROT_READ, MAP_PRIVATE, Some(fd), 0).ok()?;
            Some((region.addr(), size, identity_of(&st)))
        })();
        let _ = CSys.close(fd);
        mapped
    }

    fn unmap(&self, addr: usize, len: usize) {
        // A mapping `map` made a moment ago and nothing has seen (D-7.7).
        let _ = crate::raw::munmap(addr, len);
    }

    fn identity(&self, path: &[u8]) -> Option<gl::Identity> {
        with_path(path, |p| CSys.fstatat(Fd::CWD, p, 0).ok()).map(|st| identity_of(&st))
    }

    fn read_prefix(&self, path: &[u8], buf: &mut [u8]) -> Option<usize> {
        let fd = with_path(path, |p| CSys.openat(Fd::CWD, p, O_RDONLY | O_CLOEXEC, 0).ok())?;
        let n = CSys.read(fd, buf).ok();
        let _ = CSys.close(fd);
        n
    }

    fn list_dir(&self, path: &[u8], f: &mut dyn FnMut(&[u8])) {
        let Some(fd) = with_path(path, |p| CSys.openat(Fd::CWD, p, O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).ok()) else { return };
        let mut buf = [0u8; 4096];
        while let Ok(n) = CSys.getdents64(fd, &mut buf) {
            if n == 0 {
                break;
            }
            // linux_dirent64: d_ino (8), d_off (8), d_reclen (2), d_type (1), d_name (NUL-terminated).
            let mut at = 0usize;
            while let Some(rec) = buf.get(at..n) {
                let Some(&[lo, hi]) = rec.get(16..18) else { break };
                let len = usize::from(u16::from_le_bytes([lo, hi]));
                let name = rec.get(19..len.min(rec.len())).unwrap_or(&[]);
                f(name.split(|&b| b == 0).next().unwrap_or(&[]));
                if len == 0 {
                    break;
                }
                at = at.saturating_add(len);
            }
        }
        let _ = CSys.close(fd);
    }

    fn locpath(&self) -> Option<&[u8]> {
        if process::secure() {
            return None;
        }
        process::getenv(crate::startup_names::LOCPATH).map(strz::env_value).filter(|v| !v.is_empty())
    }
}

/// The interned handle a `locale_t` names (`LC_GLOBAL_LOCALE`: the global locale's).
fn handle_of(loc: usize) -> usize {
    view_of(loc).handle()
}

/// The locale a `locale_t` names (`LC_GLOBAL_LOCALE` and an invalid value: the global one).
pub fn of(loc: usize) -> &'static Locale {
    view_of(loc).locale()
}

/// The view of the locale a `locale_t` names (as [`of`]; D-7.13, LOCALE-CTYPE-L). An object's
/// words 0...12 hold the address of its view (the view is leaked, immutable and never freed,
/// D-7.13), so the common case is one load of the object's first word: no handle, no slot
/// table. `newlocale` over a `base` rewrites the words one atomic store at a time and a
/// reader takes one of them, so it sees the old view or the new one, never a mix; a `locale_t`
/// that `freelocale` released is the caller's error, as before.
#[inline]
pub fn view_of(loc: usize) -> &'static LocaleView {
    if let Some(w) = tb::locale_object(loc) {
        let a = w.first().map_or(0, |a| a.load(Acquire));
        if a != 0 {
            return tb::view_at(a);
        }
    }
    view_of_special(loc)
}

/// [`view_of`] for `LC_GLOBAL_LOCALE`, a null or an invalid `locale_t`. Inline and call-free:
/// out of line, the call made every caller save its arguments (LV-4 found the same).
#[inline]
fn view_of_special(loc: usize) -> &'static LocaleView {
    gl::view(&Glue, if loc == GLOBAL || loc == 0 { gl::global_handle() } else { 0 })
}

/// The calling thread's current locale view (D-7.13): its `view` word, one `%fs` load; a thread
/// whose word is not stored (0) resolves its `uselocale` choice, else the global locale.
#[inline]
pub fn view() -> &'static LocaleView {
    match tb::read::<tb::View>() {
        0 => view_of(tb::read::<tb::Locale>()),
        w => tb::view_at(w),
    }
}

/// The calling thread's current locale: its `uselocale` choice, else the global locale.
#[inline]
pub fn current() -> &'static Locale {
    view().locale()
}

/// The interned handle of the calling thread's current locale (D-7.8): a value [`by_handle`]
/// turns back into the locale for the rest of the process (a compiled `regex_t` keeps it).
pub fn current_handle() -> usize {
    view().handle()
}

/// The locale an interned handle names (`"C"` for a value that names none).
pub fn by_handle(handle: usize) -> &'static Locale {
    gl::get(&Glue, handle)
}

/// The ABI table pointers of `l`'s `LC_CTYPE` (each at character 0): class masks, `tolower`,
/// `toupper`.
fn tables(l: &Locale) -> [usize; 3] {
    match l.ctype() {
        Ctype::C | Ctype::CUtf8 => [
            C_CLASS.as_ptr().wrapping_add(ZERO).expose_provenance(),
            C_TOLOWER.as_ptr().wrapping_add(ZERO).expose_provenance(),
            C_TOUPPER.as_ptr().wrapping_add(ZERO).expose_provenance(),
        ],
        Ctype::File(f) => [
            f.class_table().as_ptr().wrapping_add(2 * ZERO).expose_provenance(),
            f.tolower_table().as_ptr().wrapping_add(4 * ZERO).expose_provenance(),
            f.toupper_table().as_ptr().wrapping_add(4 * ZERO).expose_provenance(),
        ],
    }
}

// ------------------------------------------------------------------------------ ctype words
//
// D-5.5: a thread block's `ctype_b`, `ctype_tolower` and `ctype_toupper` hold the tables of the
// thread's current locale at all times (the ABI's per-thread cache that `__ctype_b_loc` and its
// siblings expose, and that the byte classification functions read). Writers: `uselocale` (the
// calling thread's), `setlocale` (every thread using the global locale, D-7.8), thread creation
// (the new thread's, when it is listed). Every store of the global locale's tables, and every
// switch between the global locale and an object, runs with the thread-list lock held
// (`thread::serialized`, `thread::each_block`), reading the global handle inside it: `setlocale`
// changes the global locale before it takes the lock, so the last update a thread receives is
// the newest global locale's. A switch between two objects needs no lock: the update skips the
// thread before and after. The tables are statics or read-only file mappings, never written
// and never unmapped (D-7.7), so readers take no lock and need no ordering for their contents.
// A thread the runtime does not list (a test process's or another `ld.so`'s threads, D-22.10)
// fills its words on first use and follows only its own `uselocale` and `setlocale` calls (as
// every thread does in glibc).

/// Store the ctype words of `b` for its current choice (its object, else the global locale).
/// Callers serialise against [`publish_global`] (see above).
pub(crate) fn refresh_words(b: &liella_rt::tls::ThreadBlock) {
    store_words(b, view_of(b.locale.load(Acquire)));
}

/// `b`'s three ctype words for `v` (class, `tolower`, `toupper`: the [`tables`] order) and, in
/// a process the runtime started, its `view` word (D-7.13). Elsewhere (a test process's or
/// another `ld.so`'s threads, D-22.10) a thread may not be listed, so `setlocale` in another
/// thread could not reach its view: the word stays 0 and [`view`] resolves on every call, as
/// before. In a process the runtime started every thread that runs library code is listed.
fn store_words(b: &liella_rt::tls::ThreadBlock, v: &'static LocaleView) {
    let [class, lower, upper] = tables(v.locale());
    b.ctype_b.store(class, Release);
    b.ctype_tolower.store(lower, Release);
    b.ctype_toupper.store(upper, Release);
    if crate::thread::owned() {
        b.view.store(core::ptr::from_ref(v).expose_provenance(), Release);
    }
}

/// After `newlocale` changed the object `loc` in place: the `view` word of every thread that
/// uses it, so [`current`] follows the object as it did when it read the object (D-7.13). The
/// ctype words are left as they were (D-5.5: only `uselocale` and `setlocale` store them).
fn refresh_views_of(loc: usize) {
    if !crate::thread::owned() {
        return;
    }
    crate::thread::each_block(|b| {
        if b.locale.load(Acquire) == loc {
            b.view.store(core::ptr::from_ref(view_of(loc)).expose_provenance(), Release);
        }
    });
}

/// After the global locale changed: the words of every thread that uses it.
fn publish_global() {
    crate::thread::each_block(|b| {
        if b.locale.load(Acquire) == 0 {
            store_words(b, view_of(GLOBAL));
        }
    });
}

/// The address of the calling thread's pointer `slot` (0 `__ctype_b`, 1 `__ctype_tolower`,
/// 2 `__ctype_toupper`), always current (D-5.5); filled here on first use in a thread the
/// runtime does not list.
#[inline]
pub fn ctype_slot(slot: usize) -> usize {
    // TINY-HOT: the filled case is one `%fs` load and a test, then the thread pointer plus the
    // word's offset (`__ctype_b_loc` = two loads and an add, no call).
    if tb::read::<tb::CtypeB>() == 0 {
        ctype_fill();
    }
    let off = match slot {
        0 => core::mem::offset_of!(liella_rt::tls::ThreadBlock, ctype_b),
        1 => core::mem::offset_of!(liella_rt::tls::ThreadBlock, ctype_tolower),
        _ => core::mem::offset_of!(liella_rt::tls::ThreadBlock, ctype_toupper),
    };
    tb::tp().wrapping_add(off)
}

/// [`ctype_slot`]'s first call in a thread whose words are not stored yet.
#[cold]
#[inline(never)]
fn ctype_fill() {
    let b = tb::current();
    crate::thread::serialized(|| refresh_words(b));
}

/// The table at `word` (the entry for character 0, `ZERO` entries of `UNIT` bytes before it),
/// as the bytes of its 384 entries.
#[inline]
fn table_bytes<const UNIT: usize>(word: usize) -> &'static [u8] {
    // The word holds an address `tables` gave (a static, or a mapped locale file whose byte
    // tables `CtypeFile::parse` checked to be complete), or one a program stored through
    // `__ctype_*_loc`, which the header macros index alike.
    conv::mapped_bytes(word.wrapping_sub(UNIT.wrapping_mul(ZERO)), UNIT.wrapping_mul(ctype_tables::LEN))
}

/// The byte offset of a C `int` byte argument's entry in a 384-entry table of `UNIT`-byte
/// entries (-128...255), or `None`.
#[inline]
fn entry_offset<const UNIT: usize>(c: i32) -> Option<usize> {
    usize::try_from(c.wrapping_add(128)).ok().filter(|&i| i < ctype_tables::LEN).map(|i| i.wrapping_mul(UNIT))
}

/// [`byte_class`] for a thread whose ctype words are not stored: the locale's answer.
#[cold]
#[inline(never)]
fn class_unstored(c: i32) -> Class {
    ctype_tables::class(c, current())
}

/// [`case_map`] for a thread whose ctype words are not stored: the locale's answer.
#[cold]
#[inline(never)]
fn case_unstored(c: i32, upper: bool) -> i32 {
    let l = current();
    if upper { ctype_tables::toupper(c, l) } else { ctype_tables::tolower(c, l) }
}

/// The calling thread's byte classes of `c` (`isalpha`, ...): its `__ctype_b` word's table
/// (D-5.5); none outside -128...255.
#[inline]
pub fn byte_class(c: i32) -> Class {
    let w = tb::read::<tb::CtypeB>();
    if w == 0 {
        return class_unstored(c);
    }
    let Some(o) = entry_offset::<2>(c) else { return Class(0) };
    table_bytes::<2>(w).get(o..o.wrapping_add(2)).and_then(|b| <[u8; 2]>::try_from(b).ok()).map_or(Class(0), |b| Class(u16::from_ne_bytes(b)))
}

/// `toupper` (`upper`) or `tolower` of `c` through the calling thread's word `w` (D-5.5);
/// values outside -128...255 map to themselves.
#[inline]
fn case_map(c: i32, w: usize, upper: bool) -> i32 {
    if w == 0 {
        return case_unstored(c, upper);
    }
    let Some(o) = entry_offset::<4>(c) else { return c };
    table_bytes::<4>(w).get(o..o.wrapping_add(4)).and_then(|b| <[u8; 4]>::try_from(b).ok()).map_or(c, i32::from_ne_bytes)
}

/// The table word (`slot` 13 class, 14 `tolower`, 15 `toupper`) of the object `loc`, or 0 for
/// `LC_GLOBAL_LOCALE`, a null or an invalid `locale_t` (the caller then asks the locale).
#[inline]
fn object_table(loc: usize, slot: usize) -> usize {
    tb::locale_object(loc).and_then(|w| w.get(slot)).map_or(0, |a| a.load(Acquire))
}

/// The byte classes of `c` in `loc` (`isalpha_l`, ...): the object's `__ctype_b` word's table,
/// as the thread's own word does for [`byte_class`].
#[inline]
pub fn byte_class_l(c: i32, loc: usize) -> Class {
    let w = object_table(loc, 13);
    if w == 0 {
        return class_of_loc(c, loc);
    }
    let Some(o) = entry_offset::<2>(c) else { return Class(0) };
    table_bytes::<2>(w).get(o..o.wrapping_add(2)).and_then(|b| <[u8; 2]>::try_from(b).ok()).map_or(Class(0), |b| Class(u16::from_ne_bytes(b)))
}

/// [`byte_class_l`] for `LC_GLOBAL_LOCALE`, a null or an invalid `locale_t`.
#[cold]
#[inline(never)]
fn class_of_loc(c: i32, loc: usize) -> Class {
    ctype_tables::class(c, of(loc))
}

/// [`case_map_l`] for `LC_GLOBAL_LOCALE`, a null or an invalid `locale_t`.
#[cold]
#[inline(never)]
fn case_of_loc(c: i32, loc: usize, upper: bool) -> i32 {
    let l = of(loc);
    if upper { ctype_tables::toupper(c, l) } else { ctype_tables::tolower(c, l) }
}

/// `toupper_l` (`upper`) or `tolower_l` of `c` through the object's table.
#[inline]
pub fn case_map_l(c: i32, loc: usize, upper: bool) -> i32 {
    let w = object_table(loc, if upper { 15 } else { 14 });
    if w == 0 {
        return case_of_loc(c, loc, upper);
    }
    case_map(c, w, upper)
}

/// `toupper(c)` in the calling thread's locale (D-5.5).
#[inline]
pub fn toupper(c: i32) -> i32 {
    case_map(c, tb::read::<tb::CtypeToupper>(), true)
}

/// `tolower(c)` in the calling thread's locale (D-5.5).
#[inline]
pub fn tolower(c: i32) -> i32 {
    case_map(c, tb::read::<tb::CtypeTolower>(), false)
}

/// Fill a `locale_t` object for the interned locale `handle`.
fn fill(words: &[AtomicUsize], handle: usize) {
    let v = gl::view(&Glue, handle);
    let l = *v.locale();
    let at = core::ptr::from_ref(v).expose_provenance();
    for w in words.iter().take(13) {
        w.store(at, Release);
    }
    for (w, v) in words.iter().skip(13).zip(tables(&l)) {
        w.store(v, Release);
    }
    // __names: LC_ALL (slot 6) has no category; it repeats LC_CTYPE's.
    for (i, w) in words.iter().skip(16).take(13).enumerate() {
        let name = i32::try_from(i).ok().and_then(Cat::from_number).map_or(l.name(Cat::Ctype), |c| l.name(c));
        w.store(name.as_ptr().expose_provenance(), Release);
    }
}

/// A new `locale_t` for `handle`, or 0 with `ENOMEM`.
fn new_object(handle: usize) -> usize {
    let Some(addr) = heap::alloc_words(tb::LOCALE_WORDS) else { return 0 };
    fill(tb::words64(addr, tb::LOCALE_WORDS), handle);
    addr
}

/// The categories of an `LC_*_MASK` combination, or `EINVAL` for unknown bits.
fn categories(mask: i32) -> Result<([Cat; 12], usize), liella::error::Errno> {
    let all = Cat::ALL.iter().fold(0i32, |m, c| m | (1 << c.number()));
    if mask & !all != 0 {
        return Err(liella::error::Errno::EINVAL);
    }
    let mut out = [Cat::Ctype; 12];
    let mut n = 0usize;
    for c in Cat::ALL {
        if mask & (1 << c.number()) != 0 {
            if let Some(o) = out.get_mut(n) {
                *o = c;
            }
            n = n.saturating_add(1);
        }
    }
    Ok((out, n))
}

/// `newlocale(mask, name, base)`: the new object (or `base`, reused), or 0 and the error.
pub fn newlocale(mask: i32, name: Option<&[u8]>, base: usize) -> Result<usize, liella::error::Errno> {
    let (cats, n) = categories(mask)?;
    let name = name.ok_or(liella::error::Errno::EINVAL)?;
    let base_locale = if base == 0 { Locale::C } else { *of(base) };
    let handle = gl::newlocale(&CSys, &Glue, cats.get(..n).unwrap_or(&[]), name, base_locale)?;
    // `base` is consumed: reuse its block (D-7.3).
    if base != 0 && base != GLOBAL {
        if let Some(w) = tb::locale_object(base) {
            fill(w, handle);
            refresh_views_of(base);
            return Ok(base);
        }
    }
    match new_object(handle) {
        0 => Err(liella::error::Errno::ENOMEM),
        a => Ok(a),
    }
}

/// `duplocale(loc)`: a new object for the same locale.
pub fn duplocale(loc: usize) -> Result<usize, liella::error::Errno> {
    match new_object(handle_of(loc)) {
        0 => Err(liella::error::Errno::ENOMEM),
        a => Ok(a),
    }
}

/// `freelocale(loc)`.
pub fn freelocale(loc: usize) {
    if loc != 0 && loc != GLOBAL {
        heap::free_words(loc);
    }
}

/// `uselocale(loc)`: the previous choice (`LC_GLOBAL_LOCALE` for the global locale); `loc` 0
/// only queries.
pub fn uselocale(loc: usize) -> usize {
    if loc == 0 {
        // NO-ROW: the query (`ls` asks once per file) is one `%fs` load of the thread's own word.
        let prev = tb::read::<tb::Locale>();
        return if prev == 0 { GLOBAL } else { prev };
    }
    let b = tb::current();
    let prev = b.locale.load(Relaxed);
    let new = if loc == GLOBAL { 0 } else { loc };
    if loc != 0 {
        let switch = || {
            b.locale.store(new, Release);
            refresh_words(b);
        };
        if prev != 0 && new != 0 {
            // Object to object: `publish_global` skips this thread before and after (D-5.5).
            switch();
        } else if prev != new || b.ctype_b.load(Relaxed) == 0 {
            crate::thread::serialized(switch);
        }
    }
    if prev == 0 { GLOBAL } else { prev }
}

/// The variables `setlocale(cat, "")` reads (`global_locale::setlocale`: `LC_ALL`, the
/// category's own, `LANG`), in [`crate::startup_names`] (STARTUP-READ-CONSTANTS).
const ENV_KEYS: [&[u8]; 14] = {
    use crate::startup_names as n;
    [
        n::LC_ALL, n::LC_CTYPE, n::LC_NUMERIC, n::LC_TIME, n::LC_COLLATE, n::LC_MONETARY, n::LC_MESSAGES, n::LC_PAPER, n::LC_NAME,
        n::LC_ADDRESS, n::LC_TELEPHONE, n::LC_MEASUREMENT, n::LC_IDENTIFICATION, n::LANG,
    ]
};

/// The index of `k` in [`ENV_KEYS`].
fn env_key(k: &[u8]) -> Option<usize> {
    ENV_KEYS.iter().position(|&n| n == k)
}

/// `setlocale(category, name)`: the name (`'static`, NUL-terminated), or `None`.
pub fn setlocale(category: i32, name: Option<&[u8]>) -> Option<&'static [u8]> {
    let cat = if category == 6 { None } else { Some(Cat::from_number(category)?) };
    // NO-ROW: a query changes nothing, so it skips the environment table and the change check.
    let Some(name) = name else {
        return gl::setlocale(&CSys, &Glue, cat, gl::Request::Query);
    };
    let name = Some(name);
    // `""`: every variable the selection may read, from one walk over `environ` (SETLOCALE-3:
    // fourteen `getenv` scans before); a name it was not given to look up is looked up alone.
    let mut found: [Option<&'static [u8]>; ENV_KEYS.len()] = [None; ENV_KEYS.len()];
    if name == Some(b"") {
        let mut at = [None; ENV_KEYS.len()];
        process::getenv_many(&ENV_KEYS, &mut at);
        for (v, a) in found.iter_mut().zip(at) {
            *v = a.map(strz::env_value);
        }
    }
    let getenv = |k: &[u8]| -> Option<&'static [u8]> {
        match env_key(k) {
            Some(i) => found.get(i).copied().flatten(),
            None => process::getenv(k).map(strz::env_value),
        }
    };
    let req = match name {
        None => gl::Request::Query,
        Some(b"") => gl::Request::Environment(&getenv),
        Some(n) => gl::Request::Name(n),
    };
    let before = gl::global_handle();
    let r = gl::setlocale(&CSys, &Glue, cat, req);
    // A changed category may select other message catalogs (§7.7, `_nl_msg_cat_cntr`).
    if gl::global_handle() != before {
        publish_global();
        crate::catalog::locale_changed();
    }
    r
}

/// `liella_locale_invalidate` (D-X.13): the mapped locale files and parsed charmaps are checked
/// against their files before their next use (changed ones are read again), and the global
/// locale is loaded again by its names. `ENOENT` when a name no longer resolves (the global
/// locale is then unchanged).
pub fn invalidate() -> Result<(), liella::error::Errno> {
    let before = gl::global_handle();
    let r = gl::invalidate_and_reload(&CSys, &Glue);
    if gl::global_handle() != before {
        publish_global();
        crate::catalog::locale_changed();
    }
    r
}

/// An empty string for absent items.
static EMPTY: [u8; 1] = [0];

/// `nl_langinfo(item)` in `l`: the item's address, or for a word item its value (D-7.10). The
/// category's item table answers (LV-3); a category without one, the name item and an unknown
/// category take [`langinfo_slow`].
#[inline]
pub fn langinfo(item: i32, v: &LocaleView) -> usize {
    // The `nl_item`'s bits as unsigned: category number above 16 bits, item index below. A
    // negative item, a number past the table, `LC_ALL` and the name item (index 0xffff: no table
    // has that many entries) find no entry and take the slow path.
    let bits = u32::from_ne_bytes(item.to_ne_bytes());
    if let Some(entry) = v.entry(bits >> 16, usize::try_from(bits & 0xffff).unwrap_or(usize::MAX)) {
        return match entry {
            Item::Word(v) => usize::try_from(v).unwrap_or(0),
            Item::Bytes([]) => EMPTY.as_ptr().expose_provenance(),
            Item::Bytes(b) => b.as_ptr().expose_provenance(),
        };
    }
    langinfo_slow(item, v.locale())
}

/// [`langinfo`] for what the item table does not answer: the name item, items past the table,
/// a category read from its file, an unknown category.
#[cold]
#[inline(never)]
fn langinfo_slow(item: i32, l: &Locale) -> usize {
    let empty = EMPTY.as_ptr().expose_provenance();
    let (cat, Ok(index)) = (item >> 16, usize::try_from(item & 0xffff)) else { return empty };
    let Some(cat) = Cat::from_number(cat) else { return empty };
    // `_NL_LOCALE_NAME(category)`: index 0xffff names the category's locale (glibc 2.26).
    if index == 0xffff {
        return gl::name_z(&CSys, &Glue, l.name(cat)).map_or(empty, |s| s.as_ptr().expose_provenance());
    }
    match l.item(cat, index) {
        Item::Word(v) => usize::try_from(v).unwrap_or(0),
        Item::Bytes([]) => empty,
        Item::Bytes(b) => b.as_ptr().expose_provenance(),
    }
}

/// A string item's address for `struct lconv`; a grouping that begins with 0x7f or 0xff (no
/// grouping) is `""`.
fn lconv_string(l: &Locale, cat: Cat, index: usize, grouping: bool) -> usize {
    let s = l.string(cat, index);
    if s.is_empty() || (grouping && matches!(s.first(), Some(0x7f | 0xff))) {
        return EMPTY.as_ptr().expose_provenance();
    }
    s.as_ptr().expose_provenance()
}

/// A byte item for `struct lconv`: 0xff (none) is `CHAR_MAX`.
fn lconv_char(l: &Locale, index: usize) -> u8 {
    match l.item(Cat::Monetary, index) {
        Item::Bytes(b) => match b.first() {
            None | Some(0xff) => 127,
            Some(&v) => v,
        },
        Item::Word(_) => 127,
    }
}

/// `localeconv()`: the calling thread's `struct lconv`, filled from its current locale.
pub fn localeconv() -> usize {
    let w = &tb::current().lconv;
    // The words are worked out once per locale view (D-7.13) and copied into this thread's
    // `struct lconv`, so that a pointer returned earlier keeps its address (D-7.8).
    for (slot, v) in w.iter().zip(view().lconv(lconv_words)) {
        slot.store(v, Relaxed);
    }
    core::ptr::from_ref(w).expose_provenance()
}

/// The twelve words of `struct lconv` for `l`: ten strings, then the 14 chars (`LC_MONETARY`
/// items 7..=14 and 16..=21, packed little-endian).
fn lconv_words(l: &Locale) -> [usize; 12] {
    let strings = [
        (Cat::Numeric, 0, false), (Cat::Numeric, 1, false), (Cat::Numeric, 2, true),
        (Cat::Monetary, 0, false), (Cat::Monetary, 1, false), (Cat::Monetary, 2, false),
        (Cat::Monetary, 3, false), (Cat::Monetary, 4, true), (Cat::Monetary, 5, false), (Cat::Monetary, 6, false),
    ];
    let mut out = [0usize; 12];
    for (slot, (cat, index, grouping)) in out.iter_mut().zip(strings) {
        *slot = lconv_string(l, cat, index, grouping);
    }
    let chars: [u8; 16] = [
        lconv_char(l, 7), lconv_char(l, 8), lconv_char(l, 9), lconv_char(l, 10), lconv_char(l, 11), lconv_char(l, 12),
        lconv_char(l, 13), lconv_char(l, 14), lconv_char(l, 16), lconv_char(l, 17), lconv_char(l, 18), lconv_char(l, 19),
        lconv_char(l, 20), lconv_char(l, 21), 0, 0,
    ];
    for (slot, bytes) in out.iter_mut().skip(10).zip(chars.chunks_exact(8)) {
        let word: [u8; 8] = bytes.try_into().unwrap_or([0; 8]);
        *slot = usize::from_le_bytes(word);
    }
    out
}

/// `setlocale` for C: the name's address, or 0.
pub fn setlocale_c(category: i32, name: liella::error::KResult<Option<&[u8]>>) -> usize {
    match name {
        Ok(n) => setlocale(category, n).map_or(0, |s| s.as_ptr().expose_provenance()),
        Err(_) => 0,
    }
}

/// `newlocale` for C (`name` NULL: `EINVAL`).
pub fn newlocale_c(mask: i32, name: liella::error::KResult<&[u8]>, base: usize) -> liella::error::KResult<usize> {
    newlocale(mask, Some(name?), base)
}

/// The name of category `category` (a C `LC_*` number) of the calling thread's current locale,
/// copied into `out` if it fits whole: its length (the native interface, D-X.10). `LC_ALL` (6)
/// is the composite name `setlocale(LC_ALL, NULL)` would give; an unknown category is `EINVAL`.
pub fn current_name(category: i32, out: &mut [u8]) -> liella::error::KResult<usize> {
    let l = current();
    if category == 6 {
        // Twelve names below `MAX_NAME` bytes, with their category labels, fit in 4 KiB.
        let mut buf = [0u8; 4096];
        let n = liella::locale::name::composite(l.names(), &mut buf).ok_or(liella::error::Errno::ERANGE)?;
        return Ok(conv::copy_out(buf.get(..n).unwrap_or(&[]), out));
    }
    let cat = Cat::from_number(category).ok_or(liella::error::Errno::EINVAL)?;
    Ok(conv::copy_out(l.name(cat), out))
}

/// The radix character of `l`'s `LC_NUMERIC` as bytes, for the narrow `strtod` family (D-8.3).
pub fn radix(l: &Locale) -> &'static [u8] {
    l.decimal_point()
}

/// The wide radix character of `l` (`_NL_NUMERIC_DECIMAL_POINT_WC`), for the `wcstod` family.
pub fn wradix(l: &Locale) -> [i32; 1] {
    [match l.item(Cat::Numeric, 3) {
        Item::Word(w) => i32::try_from(w).unwrap_or(0x2E),
        Item::Bytes(_) => 0x2E,
    }]
}

/// The byte fold of `l` for the case-insensitive functions (`strcasecmp`, `strcasestr`; D-4.4:
/// the locale's byte `tolower` table).
pub fn lower(l: &'static Locale) -> impl Fn(u8) -> u8 + Copy {
    move |b| liella::ctype::to_lower(b, l)
}

/// The wide fold of `l` (`wcscasecmp`): `towlower` on a `wchar_t` unit.
pub fn wlower(l: &'static Locale) -> impl Fn(i32) -> i32 + Copy {
    move |c| u32::try_from(c).map_or(c, |u| i32::try_from(liella::ctype::wtrans(u, liella::ctype::Trans::Lower, l)).unwrap_or(c))
}

/// `rpmatch(response)` (§7.6; D-7.9): 1 when `response` (a C string's address) matches the
/// current locale's `YESEXPR`, 0 when it matches `NOEXPR`, otherwise -1 (also for an expression
/// that does not compile, and for NULL, where the reference crashes). The expressions are
/// POSIX extended regular expressions, matched with the library's `regcomp`/`regexec`.
pub fn rpmatch(response: usize) -> i32 {
    if response == 0 {
        return -1;
    }
    let l = current();
    for (index, answer) in [(0usize, 1), (1, 0)] {
        let Item::Bytes(expr) = l.item(Cat::Messages, index) else { continue };
        let expr = expr.split(|&b| b == 0).next().unwrap_or(&[]);
        let mut re = liella_abi::layout::regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] };
        let flags = liella::pattern::regex::cflags::EXTENDED | liella::pattern::regex::cflags::NOSUB;
        if crate::regex::regcomp(Some(&mut re), Ok(expr), flags) != 0 {
            continue;
        }
        let matched = crate::regex::regexec(Some(&re), response, 0, 0, 0) == 0;
        crate::regex::regfree(Some(&mut re));
        if matched {
            return answer;
        }
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The item-table fast path of `nl_langinfo` answers what the per-call path answers (LV-4),
    /// for the built-in locales over every category number around the valid ones, item indexes
    /// past the tables and negative items.
    #[test]
    #[allow(clippy::unwrap_used)] // a test: a failed build of the view is the failure
    fn langinfo_fast_path_is_the_per_call_path() {
        let c = LocaleView::c();
        let utf8 = LocaleView::new(Locale::c_utf8(b"C.UTF-8"), 1, &|_, _| None).unwrap();
        let mut asked = 0usize;
        let mut hits = 0usize;
        for v in [&c, &utf8] {
            for cat in (-2..16).chain([0x7fff, 0x8000]) {
                for index in (0..300).chain([0xfffe]) {
                    let item = (cat << 16) | index;
                    let want = langinfo_slow(item, v.locale());
                    assert_eq!(langinfo(item, v), want, "{:?} item {cat:#x}:{index}", v.locale().name(Cat::Ctype));
                    asked += 1;
                    hits += usize::from(want != EMPTY.as_ptr().expose_provenance());
                }
            }
        }
        for item in [-1, i32::MIN, i32::MAX, 0xffff, (6 << 16) | 3, 13 << 16] {
            assert_eq!(langinfo(item, &c), langinfo_slow(item, c.locale()), "item {item:#x}");
        }
        // Not vacuous: plenty of real entries were compared.
        assert!(asked > 10_000 && hits > 300, "{asked} asked, {hits} non-empty");
    }
}
