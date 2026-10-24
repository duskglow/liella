//! Message-catalog glue (spec §7.7, §7.8; D-7.5, D-7.6; Appendix D.2): the file access, the
//! environment and the locale `liella_legacy::intl` works through, the change counter
//! `_nl_msg_cat_cntr`, and the `nl_catd` descriptors of `catopen`.
//!
//! Catalog files are opened with `O_NONBLOCK` and used only when `fstat` shows a regular file, so a
//! FIFO never blocks; a regular file is mapped read-only, or copied into an anonymous mapping with
//! a NUL appended when its last byte is not one (so every string in it is terminated). An `nl_catd`
//! is a heap block of [`CAT_WORDS`] words: a tag, then the image's address, length, file length and
//! mapping length.

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::catalog::xopen;
use liella::error::{Errno, KResult};
use liella::iconv::Codec;
use liella::locale::file::Cat;
use liella::locale::Locale;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, MAP_ANONYMOUS, MAP_PRIVATE, O_CLOEXEC, O_NONBLOCK, O_RDONLY, PROT_READ, PROT_WRITE, S_IFMT, S_IFREG};
use liella_legacy::intl::{self, Identity, Image, Load, Setting};

use crate::sys::CSys;
use crate::{conv, data, heap, locale, process, raw, strz, tb};

/// The `LC_MESSAGES` category number.
const LC_MESSAGES: i32 = 5;
/// `NL_CAT_LOCALE`: `catopen` takes the locale from `LC_MESSAGES`.
const NL_CAT_LOCALE: i32 = 1;
/// Marks a live `nl_catd` ("licatd\0\0"); `catclose` clears it.
const TAG: usize = 0x6c69_6361_7464_0000;
/// The words of an `nl_catd`.
const CAT_WORDS: usize = 5;
/// The bytes of an empty file's image.
static EMPTY: [u8; 1] = [0];

/// Test hook (`selftest` builds only): the catalog file-system calls made (`load`, `identity`).
#[cfg(feature = "selftest")]
pub static FS_CALLS: AtomicUsize = AtomicUsize::new(0);

/// The POSIX layer's side of the catalog state.
pub struct Catalogs;

fn identity(st: &Stat) -> Identity {
    let ns = u64::try_from(st.mtime.sec).unwrap_or(0).wrapping_mul(1_000_000_000).wrapping_add(u64::try_from(st.mtime.nsec).unwrap_or(0));
    [st.dev, st.ino, u64::try_from(st.size).unwrap_or(0), ns]
}

/// `f` with `path` as a kernel path (NUL-terminated in a buffer); `ENAMETOOLONG` beyond
/// `PATH_MAX`.
fn with_path<T>(path: &[u8], f: impl FnOnce(KStr<'_>) -> KResult<T>) -> KResult<T> {
    // A short path (the usual case: one per lookup) costs no clearing of PATH_MAX bytes.
    if path.len() < 256 {
        return terminated(&mut [0u8; 256], path, f);
    }
    terminated(&mut [0u8; intl::PATH_MAX], path, f)
}

/// `f` with `path` copied into `buf` and NUL-terminated there.
fn terminated<T>(buf: &mut [u8], path: &[u8], f: impl FnOnce(KStr<'_>) -> KResult<T>) -> KResult<T> {
    buf.get_mut(..path.len()).ok_or(Errno::ENAMETOOLONG)?.copy_from_slice(path);
    let z = buf.get(..=path.len()).ok_or(Errno::ENAMETOOLONG)?;
    f(KStr::from_nul_terminated(z).map_err(|_| Errno::ENOENT)?)
}

/// The image of the open regular file `fd` of `size` bytes (see the module documentation).
fn image_of(fd: Fd, size: usize, id: Identity) -> Load {
    if size == 0 {
        return Load::Image(Image { addr: EMPTY.as_ptr().expose_provenance(), len: 1, file_len: 0, map_len: 0, id });
    }
    let Ok(region) = CSys.mmap(0, size, PROT_READ, MAP_PRIVATE, Some(fd), 0) else { return Load::Missing(Errno::ENOMEM) };
    let addr = region.addr();
    let bytes = conv::mapped_bytes(addr, size);
    if bytes.last() == Some(&0) {
        return Load::Image(Image { addr, len: size, file_len: size, map_len: size, id });
    }
    let Some(len) = size.checked_add(1) else { return Load::Missing(Errno::ENOMEM) };
    let copy = CSys.mmap(0, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).map(|r| r.addr());
    let r = match copy.ok().and_then(|c| conv::own_bytes(c, len).map(|dst| (c, dst))) {
        Some((c, dst)) => {
            if let Some(d) = dst.get_mut(..size) {
                d.copy_from_slice(bytes);
            }
            Load::Image(Image { addr: c, len, file_len: size, map_len: len, id })
        }
        None => Load::Missing(Errno::ENOMEM),
    };
    let _ = raw::munmap(addr, size);
    r
}

impl intl::Host for Catalogs {
    fn bytes_at(&self, addr: usize, len: usize) -> &'static [u8] {
        if len == 0 || addr == 0 { &[] } else { conv::mapped_bytes(addr, len) }
    }

    fn words_at(&self, addr: usize, n: usize) -> &'static [AtomicUsize] {
        if n == 0 || addr == 0 { &[] } else { tb::words64(addr, n) }
    }

    fn load(&self, path: &[u8]) -> Load {
        #[cfg(feature = "selftest")]
        FS_CALLS.fetch_add(1, Relaxed);
        let fd = match with_path(path, |p| CSys.openat(Fd::CWD, p, O_RDONLY | O_CLOEXEC | O_NONBLOCK, 0)) {
            Ok(fd) => fd,
            Err(e) => return Load::Missing(e),
        };
        let r = match CSys.fstat(fd) {
            Ok(st) if st.mode & S_IFMT == S_IFREG => match usize::try_from(st.size) {
                Ok(size) => image_of(fd, size, identity(&st)),
                Err(_) => Load::NotRegular,
            },
            _ => Load::NotRegular,
        };
        let _ = CSys.close(fd);
        r
    }

    fn unload(&self, image: &Image) {
        if image.map_len != 0 {
            let _ = raw::munmap(image.addr, image.map_len);
        }
    }

    fn identity(&self, path: &[u8]) -> Option<Identity> {
        #[cfg(feature = "selftest")]
        FS_CALLS.fetch_add(1, Relaxed);
        with_path(path, |p| CSys.fstatat(Fd::CWD, p, 0)).ok().map(|st| identity(&st))
    }

    fn language(&self) -> Option<&'static [u8]> {
        // Remembered across lookups (GETTEXT-ENV-SCAN): `translate` holds its lock here.
        static LANGUAGE: process::EnvMemo = process::EnvMemo::new();
        if process::secure() { None } else { LANGUAGE.getenv(b"LANGUAGE") }.map(strz::env_value)
    }

    fn locale(&self) -> &'static Locale {
        locale::current()
    }

    fn codec(&self, name: &[u8]) -> Option<Codec> {
        if name.is_empty() { None } else { crate::iconv::codec_named(name) }
    }

    fn charmap_at(&self, addr: usize) -> &'static liella::charmap::Charmap {
        // The address of a charmap a codec held (this host's `codec`, or a loaded locale's): one
        // `liella_legacy::global_locale` leaked, as `tb::charmap_at` requires.
        crate::tb::charmap_at(addr)
    }
}

fn count_change(changed: bool) {
    if changed {
        data::_nl_msg_cat_cntr.fetch_add(1, Release);
    }
}

/// `setlocale` changed the global locale: catalogs may change (`_nl_msg_cat_cntr` moves).
pub fn locale_changed() {
    intl::locale_changed();
    count_change(true);
}

fn text(t: KResult<Option<&[u8]>>) -> Option<&[u8]> {
    t.ok().flatten()
}

/// `dcgettext(domain, msgid, category)`: the translation's address, or `msgid` (its address).
pub fn dcgettext(domain: KResult<Option<&[u8]>>, msgid: usize, id: KResult<Option<&[u8]>>, category: i32) -> usize {
    let Some(id) = text(id) else { return msgid };
    intl::translate(&CSys, &Catalogs, text(domain), id, None, category).unwrap_or(msgid)
}

/// `gettext(msgid)`.
pub fn gettext(msgid: usize, id: KResult<Option<&[u8]>>) -> usize {
    dcgettext(Ok(None), msgid, id, LC_MESSAGES)
}

/// `dcngettext(domain, msgid, msgid_plural, n, category)`: the translation's address, or `msgid`
/// for `n == 1` and `msgid_plural` otherwise.
pub fn dcngettext(domain: KResult<Option<&[u8]>>, msgid: usize, id: KResult<Option<&[u8]>>, plural: usize, n: u64, category: i32) -> usize {
    let untranslated = if n == 1 { msgid } else { plural };
    let Some(id) = text(id) else { return untranslated };
    intl::translate(&CSys, &Catalogs, text(domain), id, Some(n), category).unwrap_or(untranslated)
}

/// `textdomain(name)`: the default domain's text, or NULL (memory exhausted).
pub fn textdomain(name: KResult<Option<&[u8]>>) -> usize {
    intl::textdomain(&CSys, &Catalogs, text(name)).map_or(0, |(a, changed)| {
        count_change(changed);
        a
    })
}

fn bind(which: Setting, domain: KResult<Option<&[u8]>>, value: KResult<Option<&[u8]>>) -> usize {
    let r = intl::bind(&CSys, &Catalogs, which, text(domain), text(value));
    data::_nl_domain_bindings.store(intl::bindings_head(), Release);
    r.map_or(0, |(a, changed)| {
        count_change(changed);
        a
    })
}

/// `bindtextdomain(domain, dir)`.
pub fn bindtextdomain(domain: KResult<Option<&[u8]>>, dir: KResult<Option<&[u8]>>) -> usize {
    bind(Setting::Directory, domain, dir)
}

/// `bind_textdomain_codeset(domain, codeset)`.
pub fn bind_textdomain_codeset(domain: KResult<Option<&[u8]>>, codeset: KResult<Option<&[u8]>>) -> usize {
    bind(Setting::Codeset, domain, codeset)
}

// ------------------------------------------------------------------------------ §7.8

/// `catopen(name, flag)`: the descriptor, or the error (a NULL name is `EINVAL`, decision).
pub fn catopen(name: KResult<&[u8]>, flag: i32) -> KResult<usize> {
    let name = name.map_err(|_| Errno::EINVAL)?;
    let lang;
    let locale: &[u8] = if flag == NL_CAT_LOCALE {
        locale::current().name(Cat::Messages)
    } else {
        lang = process::getenv(b"LANG").map(strz::env_value).filter(|v| !v.is_empty());
        lang.unwrap_or(b"C")
    };
    let nlspath = process::secure_getenv(b"NLSPATH").map(strz::env_value);
    let img = intl::catopen(&Catalogs, nlspath, name, locale)?;
    let Some(cd) = heap::alloc_words(CAT_WORDS) else {
        intl::Host::unload(&Catalogs, &img);
        return Err(Errno::ENOMEM);
    };
    let w = tb::words64(cd, CAT_WORDS);
    for (slot, v) in w.iter().skip(1).zip([img.addr, img.len, img.file_len, img.map_len]) {
        slot.store(v, Relaxed);
    }
    if let Some(t) = w.first() {
        t.store(TAG, Release);
    }
    Ok(cd)
}

/// The words of a live descriptor, or `EBADF`. `(nl_catd)-1`, NULL and misaligned values are
/// rejected; any other value must be a descriptor `catopen` returned (spec §7.8).
fn descriptor(cd: usize) -> KResult<&'static [AtomicUsize]> {
    if cd == 0 || cd == usize::MAX || cd % 8 != 0 {
        return Err(Errno::EBADF);
    }
    let w = tb::words64(cd, CAT_WORDS);
    if w.first().map(|t| t.load(Acquire)) != Some(TAG) {
        return Err(Errno::EBADF);
    }
    Ok(w)
}

fn word(w: &[AtomicUsize], i: usize) -> usize {
    w.get(i).map_or(0, |x| x.load(Relaxed))
}

/// `catgets(cd, set, msg, default)`: the message's address, or `default` with the error:
/// `ENOMSG` for a message the catalog lacks (negative numbers included), `EBADF` for an invalid
/// descriptor (spec §7.8 and C0.1, POSIX; the reference leaves `errno` alone in both cases).
pub fn catgets(cd: usize, set: i32, msg: i32, default: usize) -> (usize, Option<Errno>) {
    let w = match descriptor(cd) {
        Ok(w) => w,
        Err(e) => return (default, Some(e)),
    };
    let (addr, len, file_len) = (word(w, 1), word(w, 2), word(w, 3));
    let found = xopen::Cat::parse_header(conv::mapped_bytes(addr, len), file_len).and_then(|c| c.get(set, msg));
    match found.and_then(|off| addr.checked_add(off)) {
        Some(a) => (a, None),
        None => (default, Some(Errno::ENOMSG)),
    }
}

/// `catclose(cd)`.
pub fn catclose(cd: usize) -> KResult<()> {
    let w = descriptor(cd)?;
    let img = Image { addr: word(w, 1), len: word(w, 2), file_len: word(w, 3), map_len: word(w, 4), id: [0; 4] };
    if let Some(t) = w.first() {
        t.store(0, Release);
    }
    intl::Host::unload(&Catalogs, &img);
    heap::free_words(cd);
    Ok(())
}
