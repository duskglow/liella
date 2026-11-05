//! The process-wide state of the gettext family (spec §7.7 ⚑ HIST; D-7.5) and the search of
//! `catopen` (§7.8; D-7.6): the default domain, the bindings, the loaded catalogs and their
//! converted strings. The file formats are `liella::catalog`.
//!
//! **State.** Everything lives in records the module leaks (arrays of atomic words) and in leaked
//! strings, reached from the statics below by address through [`Host`]; one lock serializes every
//! function. Nothing is freed: every pointer returned to C (domain and directory copies,
//! translations inside mapped catalogs, converted strings) stays valid for the rest of the process,
//! and strings are interned, so memory grows only with distinct names, catalogs and conversions.
//!
//! **Lookup** (`spec/behaviour/catalogs.md`): the locale is the name of the requested category of
//! the calling thread's locale; `C`, `POSIX` and `C.UTF-8` translate nothing. Otherwise each
//! element of `LANGUAGE` (read through `secure_getenv`: ignored in a secure process), or the locale
//! name when `LANGUAGE` is unset or empty, is tried in order (an element `C` or `POSIX` ends the
//! search untranslated, as observed; so do `C.UTF-8` and `C.utf8`, by decision, as the locale of
//! those names does), after replacement through the alias file, in each of its directory
//! variants (`liella::catalog::variants`): `<dir>/<variant>/<CATEGORY>/<domain>.mo`, where `<dir>`
//! is the domain's binding (`/usr/share/locale` by default), then, for `LC_MESSAGES` only, the
//! same variants under `/usr/share/locale-langpack` whatever the binding (as the reference, a
//! distribution addition; owner ruling 2026-10-22, #411), before the next element. The first
//! catalog that has the msgid gives the translation; a catalog that is not usable is skipped.
//!
//! **Caching** (decision; the reference library returns a msgid's earlier translation after a
//! change of `LANGUAGE` until `_nl_msg_cat_cntr` moves, and never re-reads a replaced catalog
//! file): no translation is cached across lookups. Each lookup reads `LANGUAGE`'s text and the
//! locale anew (the host may remember which `environ` slot holds `LANGUAGE` until this library's
//! environment writers or `environ` change: GETTEXT-ENV-SCAN, ruling #504), and checks each loaded catalog it uses against its file with a `stat` (device, inode,
//! size, modification time) at most once per second, and at once after any change of a setting
//! (`crate::recheck`, owner ruling 2026-10-05; the same value set again is no change, ruling
//! 2026-10-23): a replaced file is loaded again, seen up to one
//! second late. A path found missing is tried again on the same clock (at most once per second,
//! at once after a change of a setting) and, for a relative path, at every lookup. A changed
//! `LANGUAGE` is not a change of a setting (owner ruling 2026-10-07, P4).
//! What *is* kept is which catalog records a search tries, in order, per domain, category,
//! locale name, `LANGUAGE` (its text, compared at every lookup) and settings generation
//! ([`RESOLVED`]): the same records the search would find, each still checked against its file as
//! above. A changed `LANGUAGE` starts no generation: the list of records for each `LANGUAGE`
//! text stays valid, and its catalogs keep their one-second clock.
//!
//! **Conversion**: a catalog whose header names a charset is converted to the domain's bound
//! codeset, else to the codeset of the calling thread's `LC_CTYPE`, with transliteration through
//! that `LC_CTYPE` (unconvertible characters become its default replacement); `""` bound means
//! no conversion. An unknown charset or codeset makes the catalog unusable for the lookup, and a
//! translation that does not convert counts as missing. Converted strings are kept per catalog,
//! target and `LC_CTYPE`, so a msgid gives the same pointer every time.

use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::catalog::{self, mo, plural, variants, xopen};
use liella::charmap::Charmap;
use liella::error::Errno;
use liella::iconv::{self, Codec, Conv, State};
use liella::locale::file::Cat;
use liella::locale::{name, Data, Locale, ALIAS_FILE};
use liella::mem::TryVec;
use liella_kernel::sys::Sys;
use liella_rt::sync::RawMutex;

use crate::recheck;

/// `_nl_default_dirname`'s text: the directory of unbound domains.
pub const DEFAULT_DIR_TEXT: [u8; 18] = *b"/usr/share/locale\0";
/// `_libc_intl_domainname`'s text.
pub const LIBC_DOMAIN_TEXT: [u8; 5] = *b"libc\0";
/// The second directory searched for `LC_MESSAGES` catalogs, after the domain's own (#411).
const LANGPACK_DIR: &[u8] = b"/usr/share/locale-langpack";
/// The default directory, as returned for an unbound domain.
static DEFAULT_DIR: [u8; 18] = DEFAULT_DIR_TEXT;
/// The default domain.
static DEFAULT_DOMAIN: [u8; 9] = *b"messages\0";
/// The longest catalog path tried, with its terminator.
pub const PATH_MAX: usize = 4096;

/// A file's identity: device, inode, size and modification time (nanoseconds).
pub type Identity = [u64; 4];

/// A file loaded read-only: `len` bytes at `addr` ending with a NUL (the file itself, or a copy of
/// it with a NUL appended when its last byte is not one), the file's own length, the length to
/// unmap (0: nothing), and its identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// Address of the bytes.
    pub addr: usize,
    /// Length of the bytes (with the final NUL).
    pub len: usize,
    /// The file's length.
    pub file_len: usize,
    /// The mapping's length (0 when nothing is mapped).
    pub map_len: usize,
    /// The file's identity.
    pub id: Identity,
}

/// What loading a path gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    /// The file's bytes.
    Image(Image),
    /// The open failed (`ENOENT`, `EACCES`, `ENOTDIR`, ...).
    Missing(Errno),
    /// Not a regular file (a directory, a FIFO, a device): never opened for reading, so a FIFO
    /// cannot block (decision; the reference library blocks).
    NotRegular,
}

/// What the module needs from the POSIX layer.
pub trait Host {
    /// The `len` bytes at `addr`: a string this module leaked, or an [`Image`].
    fn bytes_at(&self, addr: usize, len: usize) -> &'static [u8];
    /// The `n` words at `addr`, a record this module leaked.
    fn words_at(&self, addr: usize, n: usize) -> &'static [AtomicUsize];
    /// Load the file at `path` (without a terminator).
    fn load(&self, path: &[u8]) -> Load;
    /// Unmap an image [`Host::load`] returned.
    fn unload(&self, image: &Image);
    /// The identity of the file at `path` now, if it exists.
    fn identity(&self, path: &[u8]) -> Option<Identity>;
    /// `LANGUAGE`, unless unset or the process is in secure mode (its text read at each call).
    fn language(&self) -> Option<&'static [u8]>;
    /// The calling thread's current locale.
    fn locale(&self) -> &'static Locale;
    /// The codec a charset name denotes (without suffixes).
    fn codec(&self, name: &[u8]) -> Option<Codec>;
    /// The charmap at `addr`, an address a [`Codec::Charmap`] this host returned holds.
    fn charmap_at(&self, addr: usize) -> &'static Charmap;
}

static LOCK: RawMutex = RawMutex::new();
/// The check generation: moves with every change of a setting (not with `LANGUAGE`, owner
/// ruling 2026-10-07); a catalog not checked in the current one is checked at once, a missing one
/// tried again.
static GENERATION: AtomicUsize = AtomicUsize::new(1);
/// The settings generation: moves with every change of a setting that the resolution cache does
/// not hold in its key (the default domain, a binding, `setlocale`), not with `LANGUAGE`.
static SETTINGS: AtomicUsize = AtomicUsize::new(1);
/// The default domain's copy (0: `messages`).
static DOMAIN: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
/// The heads of the record lists (0: empty).
static STRINGS: AtomicUsize = AtomicUsize::new(0);
static BINDINGS: AtomicUsize = AtomicUsize::new(0);
static CATALOGS: AtomicUsize = AtomicUsize::new(0);
/// The alias file: address, length, state (0 not read, 1 read, 2 absent).
static ALIASES: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

// Record layouts (word indices). Word 0 of every record links its list (the next record's address).
const S_ADDR: usize = 1;
const S_LEN: usize = 2;
const S_WORDS: usize = 3;

const B_DOMAIN: usize = 1; // addr, len
const B_DIR: usize = 3; // addr (0: unset), len
const B_CODESET: usize = 5; // addr (0: unset), len
const B_WORDS: usize = 7;

const C_PATH: usize = 1; // addr, len
const C_STATE: usize = 3;
const C_GEN: usize = 4;
const C_ID: usize = 5; // four words
const C_IMAGE: usize = 9; // addr, len
const C_PLURAL: usize = 11; // addr (0: the default rule), len
const C_CHARSET: usize = 13; // state (0 none, 1 named), addr, len
const C_CONV: usize = 16;
const C_COUNT: usize = 17;
/// When the file was last checked (monotonic ns, `recheck`), and in which generation.
const C_CHECKED: usize = 18;
const C_CHECKED_GEN: usize = 19;
/// The lookup index (`mo::index`): address, length (0: none, search without it).
const C_INDEX: usize = 20;
/// The catalog's charset as a codec, resolved at its first use (tag, charmap; tag 0: not yet).
const C_FROM: usize = 22;
/// The target codec of the last lookup and what selected it: the bound codeset's text address
/// (0: none), the `LC_CTYPE` identity (0: empty slot), the locale's charmap address (0: none),
/// the codec (tag, charmap).
const C_TO: usize = 24;
/// 1 when the codecs [`C_FROM`] and [`C_TO`] keep are the same (the translation is returned as
/// stored, GETTEXT-3); cleared with `C_TO`.
const C_SAME: usize = 29;
const C_WORDS: usize = 30;

const ABSENT: usize = 0;
const BAD: usize = 1;
const GOOD: usize = 2;

const V_KEY: usize = 1; // three words: target codec (two), LC_CTYPE identity
const V_SLOTS: usize = 4;
const V_WORDS: usize = 5;

/// A slot whose conversion failed.
const FAILED: usize = 1;

fn get(rec: &[AtomicUsize], i: usize) -> usize {
    rec.get(i).map_or(0, |w| w.load(Acquire))
}

fn put(rec: &[AtomicUsize], i: usize, v: usize) {
    if let Some(w) = rec.get(i) {
        w.store(v, Release);
    }
}

fn addr_of(s: &[u8]) -> usize {
    s.as_ptr().expose_provenance()
}

/// A new zeroed record of `n` words.
fn record(n: usize) -> Result<&'static [AtomicUsize], Errno> {
    let mut v = TryVec::with_capacity(n)?;
    for _ in 0..n {
        v.push(AtomicUsize::new(0))?;
    }
    Ok(v.leak())
}

/// The records of a list, newest first.
fn each<H: Host, T>(h: &H, head: &AtomicUsize, words: usize, mut f: impl FnMut(&'static [AtomicUsize]) -> Option<T>) -> Option<T> {
    let mut at = head.load(Acquire);
    while at != 0 {
        let rec = h.words_at(at, words);
        if let Some(r) = f(rec) {
            return Some(r);
        }
        at = get(rec, 0);
    }
    None
}

fn push_front(head: &AtomicUsize, rec: &'static [AtomicUsize]) {
    put(rec, 0, head.load(Acquire));
    head.store(rec.as_ptr().expose_provenance(), Release);
}

/// `s` interned: a leaked copy followed by a NUL (deduplicated), as (address, length).
fn intern<H: Host>(h: &H, s: &[u8]) -> Result<(usize, usize), Errno> {
    if let Some(found) = each(h, &STRINGS, S_WORDS, |r| (h.bytes_at(get(r, S_ADDR), get(r, S_LEN)) == s).then(|| (get(r, S_ADDR), get(r, S_LEN)))) {
        return Ok(found);
    }
    let mut v = TryVec::with_capacity(s.len().saturating_add(1))?;
    v.extend_from_slice(s)?;
    v.push(0)?;
    let leaked: &'static [u8] = v.leak();
    let rec = record(S_WORDS)?;
    put(rec, S_ADDR, addr_of(leaked));
    put(rec, S_LEN, s.len());
    push_front(&STRINGS, rec);
    Ok((addr_of(leaked), s.len()))
}

fn text<H: Host>(h: &H, rec: &[AtomicUsize], i: usize) -> &'static [u8] {
    match get(rec, i) {
        0 => &[],
        a => h.bytes_at(a, get(rec, i.saturating_add(1))),
    }
}

fn bump() {
    SETTINGS.fetch_add(1, Release);
    GENERATION.fetch_add(1, Release);
}

/// A setting that selects catalogs changed outside this module (`setlocale`).
pub fn locale_changed() {
    bump();
}

/// The address `_nl_domain_bindings` holds: the newest binding record, 0 before the first.
pub fn bindings_head() -> usize {
    BINDINGS.load(Acquire)
}

fn current_domain<H: Host>(h: &H) -> &'static [u8] {
    match DOMAIN[0].load(Acquire) {
        0 => DEFAULT_DOMAIN.split_at(8).0,
        a => h.bytes_at(a, DOMAIN[1].load(Acquire)),
    }
}

/// `textdomain(name)`: the address of the (new) default domain's text, and whether the change
/// counter moves (every set does, as observed). `""` restores `messages`. `None` on allocation
/// failure.
pub fn textdomain<S: Sys, H: Host>(sys: &S, h: &H, name: Option<&[u8]>) -> Option<(usize, bool)> {
    let _g = LOCK.lock(sys);
    let Some(name) = name else {
        return Some((match DOMAIN[0].load(Acquire) {
            0 => addr_of(&DEFAULT_DOMAIN),
            a => a,
        }, false));
    };
    // The same name again selects the same catalogs: no new check generation (owner ruling
    // 2026-10-23, #1056/#1057; D-7.12). The change counter still moves, as observed.
    let same = current_domain(h) == if name.is_empty() { DEFAULT_DOMAIN.split_at(8).0 } else { name };
    let (addr, len) = if name.is_empty() { (0, 0) } else { intern(h, name).ok()? };
    DOMAIN[1].store(len, Release);
    DOMAIN[0].store(addr, Release);
    if !same {
        bump();
    }
    Some((if addr == 0 { addr_of(&DEFAULT_DOMAIN) } else { addr }, true))
}

fn find_binding<H: Host>(h: &H, domain: &[u8]) -> Option<&'static [AtomicUsize]> {
    each(h, &BINDINGS, B_WORDS, |r| (text(h, r, B_DOMAIN) == domain).then_some(r))
}

/// Which setting of a binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    /// The directory (`bindtextdomain`).
    Directory,
    /// The output codeset (`bind_textdomain_codeset`).
    Codeset,
}

/// `bindtextdomain` and `bind_textdomain_codeset`: the address of the setting's text after the
/// call (the default directory for an unset directory; `None` for an unset codeset), and whether
/// the change counter moves (a new value). A NULL or empty domain gives `None` and changes
/// nothing; so does an allocation failure.
pub fn bind<S: Sys, H: Host>(sys: &S, h: &H, which: Setting, domain: Option<&[u8]>, value: Option<&[u8]>) -> Option<(usize, bool)> {
    let domain = domain.filter(|d| !d.is_empty())?;
    let slot = if which == Setting::Directory { B_DIR } else { B_CODESET };
    let _g = LOCK.lock(sys);
    let existing = find_binding(h, domain);
    let current = |r: Option<&'static [AtomicUsize]>| match (r.map_or(0, |r| get(r, slot)), which) {
        (0, Setting::Directory) => Some(addr_of(&DEFAULT_DIR)),
        (0, Setting::Codeset) => None,
        (a, _) => Some(a),
    };
    let Some(value) = value else { return current(existing).map(|a| (a, false)) };
    let rec = match existing {
        Some(r) => r,
        None => {
            let r = record(B_WORDS).ok()?;
            let (a, l) = intern(h, domain).ok()?;
            put(r, B_DOMAIN, a);
            put(r, B_DOMAIN.saturating_add(1), l);
            push_front(&BINDINGS, r);
            r
        }
    };
    // The unchanged value changes nothing: no new check generation (owner ruling 2026-10-23,
    // #1056, revising 2026-10-05; D-7.12), so the resolved search stays valid and the files keep
    // their one-second clock. A program that calls `bindtextdomain` before each lookup does no
    // file-system work for it.
    if get(rec, slot) != 0 && text(h, rec, slot) == value {
        return Some((get(rec, slot), false));
    }
    bump();
    let (a, l) = intern(h, value).ok()?;
    put(rec, slot.saturating_add(1), l);
    put(rec, slot, a);
    Some((a, true))
}

/// The alias file's text (read once), if installed.
fn aliases<H: Host>(h: &H) -> Option<&'static [u8]> {
    match ALIASES[2].load(Acquire) {
        1 => return Some(h.bytes_at(ALIASES[0].load(Acquire), ALIASES[1].load(Acquire))),
        2 => return None,
        _ => {}
    }
    match h.load(ALIAS_FILE) {
        Load::Image(img) => {
            ALIASES[0].store(img.addr, Release);
            ALIASES[1].store(img.len, Release);
            ALIASES[2].store(1, Release);
            Some(h.bytes_at(img.addr, img.len))
        }
        _ => {
            ALIASES[2].store(2, Release);
            None
        }
    }
}

/// The catalog paths [`translate`] builds in a short buffer (most are far shorter).
const SHORT_PATH: usize = 256;

/// `parts` one after another in `buf`: the bytes written, or `None` if they do not fit.
fn join<'b>(buf: &'b mut [u8], parts: &[&[u8]]) -> Option<&'b [u8]> {
    let mut len = 0usize;
    for p in parts {
        let end = len.checked_add(p.len())?;
        buf.get_mut(len..end)?.copy_from_slice(p);
        len = end;
    }
    buf.get(..len)
}

/// The longest name [`alias_of`] remembers.
const MEMO_MAX: usize = 64;
/// The last name [`alias_of`] looked up (its bytes, then its length in `MEMO[0]`; `usize::MAX`:
/// none yet), and the answer as an offset and length into the alias table (`MEMO[2]` is
/// `usize::MAX` when the name has no alias). The table is loaded once and kept ([`aliases`]),
/// so an answer never goes stale. Read and written under `LOCK`.
static MEMO_NAME: [AtomicU8; MEMO_MAX] = [const { AtomicU8::new(0) }; MEMO_MAX];
static MEMO: [AtomicUsize; 3] = [const { AtomicUsize::new(usize::MAX) }; 3];

/// The alias file's value for `element`, if any (under `LOCK`). Every lookup asks for the same
/// name until the locale or `LANGUAGE` changes, so the last answer is kept instead of scanning
/// the table again.
fn alias_of<H: Host>(h: &H, element: &[u8]) -> Option<&'static [u8]> {
    let table = aliases(h)?;
    let n = element.len();
    if MEMO[0].load(Relaxed) == n && MEMO_NAME.iter().zip(element).all(|(m, &b)| m.load(Relaxed) == b) {
        let (off, len) = (MEMO[1].load(Relaxed), MEMO[2].load(Relaxed));
        return if len == usize::MAX { None } else { table.get(off..off.checked_add(len)?) };
    }
    let found = name::lookup_alias(table, element);
    if n <= MEMO_MAX {
        for (m, &b) in MEMO_NAME.iter().zip(element) {
            m.store(b, Relaxed);
        }
        let (off, len) = found.map_or((0, usize::MAX), |v| (v.as_ptr().addr().wrapping_sub(table.as_ptr().addr()), v.len()));
        MEMO[1].store(off, Relaxed);
        MEMO[2].store(len, Relaxed);
        MEMO[0].store(n, Relaxed);
    }
    found
}

/// The identity of the `LC_CTYPE` data of `l` (its transliteration table).
fn ctype_identity(l: &Locale) -> usize {
    match l.data(Cat::Ctype) {
        Data::C => 1,
        Data::CUtf8 => 2,
        Data::File(f) => addr_of(f.bytes()),
    }
}

/// Fill a catalog record from a loaded image.
fn prepare<H: Host>(h: &H, rec: &[AtomicUsize], img: &Image) -> Result<(), Errno> {
    for i in C_IMAGE..C_WORDS {
        put(rec, i, 0);
    }
    for (k, v) in img.id.iter().enumerate() {
        put(rec, C_ID.saturating_add(k), usize::try_from(*v).unwrap_or(usize::MAX));
    }
    put(rec, C_STATE, BAD);
    let mut bytes = h.bytes_at(img.addr, img.len);
    let Some(mut m) = mo::Mo::parse(bytes) else { return Ok(()) };
    if m.has_sysdep() {
        let expanded: &'static [u8] = mo::expand(&m)?.leak();
        bytes = expanded;
        let Some(e) = mo::Mo::parse(bytes) else { return Ok(()) };
        m = e;
    }
    if let Some(header) = m.header() {
        if let Some(cs) = mo::charset(header) {
            put(rec, C_CHARSET.saturating_add(1), addr_of(cs));
            put(rec, C_CHARSET.saturating_add(2), cs.len());
            put(rec, C_CHARSET, 1);
        }
        if let Some(p) = plural::compile(header)? {
            let p: &'static [u8] = p.leak();
            put(rec, C_PLURAL.saturating_add(1), p.len());
            put(rec, C_PLURAL, addr_of(p));
        }
    }
    // The lookup index (PJ-9): without memory for it the catalog is searched as before.
    if let Some(ix) = mo::index(&m).ok().flatten() {
        let ix: &'static [u8] = ix.leak();
        put(rec, C_INDEX.saturating_add(1), ix.len());
        put(rec, C_INDEX, addr_of(ix));
    }
    put(rec, C_COUNT, m.len());
    put(rec, C_IMAGE.saturating_add(1), bytes.len());
    put(rec, C_IMAGE, addr_of(bytes));
    put(rec, C_STATE, GOOD);
    Ok(())
}

fn stored_id(rec: &[AtomicUsize]) -> Identity {
    let w = |k: usize| u64::try_from(get(rec, C_ID.saturating_add(k))).unwrap_or(u64::MAX);
    [w(0), w(1), w(2), w(3)]
}

/// The catalog record of `path`, made (not yet loaded: absent, of no generation) if there is none.
fn record_for<H: Host>(h: &H, path: &[u8]) -> Option<&'static [AtomicUsize]> {
    if let Some(r) = each(h, &CATALOGS, C_WORDS, |r| (text(h, r, C_PATH) == path).then_some(r)) {
        return Some(r);
    }
    let r = record(C_WORDS).ok()?;
    let (a, l) = intern(h, path).ok()?;
    put(r, C_PATH, a);
    put(r, C_PATH.saturating_add(1), l);
    push_front(&CATALOGS, r);
    Some(r)
}

/// Whether the loaded file of `rec` (at `path`) is still the file there. The one place that
/// decides when a file is checked: at most once per second (`recheck`, owner ruling 2026-10-05;
/// D-7.12), and at once in a new generation (a changed setting; not `LANGUAGE`, P4).
fn unchanged<S: Sys, H: Host>(sys: &S, h: &H, rec: &[AtomicUsize], path: &[u8], clock: &Clock) -> bool {
    let generation = GENERATION.load(Acquire);
    let now = clock.now(sys);
    let last = u64::try_from(get(rec, C_CHECKED)).unwrap_or(0);
    if get(rec, C_CHECKED_GEN) == generation && !recheck::due(last, now) {
        return true;
    }
    let same = h.identity(path) == Some(stored_id(rec));
    if same {
        checked(rec, now, generation);
    }
    same
}

/// The monotonic time of one lookup, read at its first use: one clock read per lookup, however
/// many candidates it checks.
struct Clock {
    at: core::cell::Cell<u64>,
    /// The counter read before the lookup (GETTEXT-6), before any clock read: later than the clock's time.
    tsc: u64,
}

impl Clock {
    const fn new(tsc: u64) -> Self {
        Clock { at: core::cell::Cell::new(0), tsc }
    }

    fn now<S: Sys>(&self, sys: &S) -> u64 {
        match self.at.get() {
            0 => {
                let t = recheck::now(sys);
                self.at.set(t);
                t
            }
            t => t,
        }
    }

    /// The counter and the time of this lookup's clock read, if it made one.
    fn read(&self) -> Option<(u64, u64)> {
        Some(self.at.get()).filter(|&t| t != 0).map(|t| (self.tsc, t))
    }
}

/// Record a check of `rec`'s file at `now` in `generation`.
fn checked(rec: &[AtomicUsize], now: u64, generation: usize) {
    put(rec, C_CHECKED, usize::try_from(now).unwrap_or(0));
    put(rec, C_CHECKED_GEN, generation);
}

/// The usable catalog at `path` (loading it, or loading it again when its file changed), or
/// `None`.
fn catalog_at<S: Sys, H: Host>(sys: &S, h: &H, path: &[u8], clock: &Clock) -> Option<&'static [AtomicUsize]> {
    catalog_of(sys, h, record_for(h, path)?, clock)
}

/// The catalog of the record `rec` if usable (loading its file, or loading it again when the file
/// changed), or `None`.
fn catalog_of<S: Sys, H: Host>(sys: &S, h: &H, rec: &'static [AtomicUsize], clock: &Clock) -> Option<&'static [AtomicUsize]> {
    let generation = GENERATION.load(Acquire);
    let path = text(h, rec, C_PATH);
    let state = get(rec, C_STATE);
    if state == ABSENT {
        // A missing file is tried again at most once per second, as a loaded one is checked
        // (owner ruling 2026-10-07), and at once in a new generation.
        let last = u64::try_from(get(rec, C_CHECKED)).unwrap_or(0);
        if get(rec, C_GEN) == generation && path.first() == Some(&b'/') && !recheck::due(last, clock.now(sys)) {
            return None;
        }
    } else if unchanged(sys, h, rec, path, clock) {
        return (state == GOOD).then_some(rec);
    }
    match h.load(path) {
        Load::Image(img) => {
            if prepare(h, rec, &img).is_err() {
                put(rec, C_STATE, BAD);
            }
            // A file that is not a catalog, or whose strings were expanded into a copy, is no
            // longer referenced (a usable catalog's own image never goes away: C may hold its
            // strings).
            if get(rec, C_STATE) != GOOD || get(rec, C_IMAGE) != img.addr {
                h.unload(&img);
            }
            checked(rec, recheck::now(sys), generation);
        }
        Load::Missing(_) | Load::NotRegular => {
            put(rec, C_STATE, ABSENT);
            put(rec, C_CHECKED, usize::try_from(recheck::now(sys)).unwrap_or(0));
            put(rec, C_GEN, generation);
        }
    }
    (get(rec, C_STATE) == GOOD).then_some(rec)
}

/// The two words that name a codec (its tag and charmap), for the conversion cache's key.
fn codec_words(c: Codec) -> [usize; 2] {
    let w = Conv { from: c, to: c, translit: false, ignore: false, state: State::default() }.to_words();
    [w[0], w[1]]
}

/// Convert `input` from `from` to `to` (transliterating through `loc`), appending to `out`.
fn convert_into(from: Codec, to: Codec, input: &[u8], loc: &Locale, out: &mut TryVec<u8>) -> Result<bool, Errno> {
    let mut room = input.len().saturating_mul(4).saturating_add(16);
    loop {
        let mut buf = TryVec::new();
        buf.resize(room, 0u8)?;
        let mut conv = Conv { from, to, translit: true, ignore: false, state: State::default() };
        let o = conv.convert(input, buf.as_mut_slice(), loc);
        match o.result {
            Ok(_) => {
                out.extend_from_slice(buf.as_slice().get(..o.written).unwrap_or(&[]))?;
                return Ok(true);
            }
            Err(Errno::E2BIG) => room = room.checked_mul(2).ok_or(Errno::ENOMEM)?,
            Err(_) => return Ok(false),
        }
    }
}

/// The converted forms of translation `i` (a block: `u32` count, `u32` offset per form, then
/// each form converted and followed by a NUL), or `None` if a form does not convert.
fn convert_entry(m: &mo::Mo<'_>, i: usize, (from, to): (Codec, Codec), loc: &Locale) -> Result<Option<TryVec<u8>>, Errno> {
    let mut forms: TryVec<usize> = TryVec::new();
    let mut k = 0u64;
    while let Some(off) = m.form(i, k) {
        if k > 0 && off == m.form(i, 0).unwrap_or(off) {
            break;
        }
        forms.push(off)?;
        k = k.saturating_add(1);
    }
    let count = forms.len();
    let head = count.saturating_add(1).saturating_mul(4);
    let mut block: TryVec<u8> = TryVec::new();
    block.resize(head, 0)?;
    for (j, &off) in forms.as_slice().iter().enumerate() {
        let at = u32::try_from(block.len()).map_err(|_| Errno::ENOMEM)?;
        if let Some(dst) = j.checked_add(1).and_then(|x| x.checked_mul(4)).and_then(|s| block.as_mut_slice().get_mut(s..s.saturating_add(4))) {
            dst.copy_from_slice(&at.to_le_bytes());
        }
        if !convert_into(from, to, m.text(off), loc, &mut block)? {
            return Ok(None);
        }
        block.push(0)?;
    }
    if let Some(dst) = block.as_mut_slice().get_mut(..4) {
        dst.copy_from_slice(&u32::try_from(count).unwrap_or(0).to_le_bytes());
    }
    Ok(Some(block))
}

/// The address of converted form `k` of translation `i` of the catalog `rec`, converting and
/// keeping it on first use; `None` if it does not convert.
fn converted<H: Host>(h: &H, rec: &'static [AtomicUsize], m: &mo::Mo<'_>, (i, k): (usize, u64), (from, to): (Codec, Codec), loc: &Locale) -> Option<usize> {
    let [t0, t1] = codec_words(to);
    let key = [t0, t1, ctype_identity(loc)];
    let n = get(rec, C_COUNT);
    let slots_n = n.checked_mul(2)?;
    let head = rec.get(C_CONV)?;
    let same = |v: &[AtomicUsize]| key.iter().enumerate().all(|(j, x)| get(v, V_KEY.saturating_add(j)) == *x);
    let conv = match each(h, head, V_WORDS, |v| same(v).then_some(v)) {
        Some(v) => v,
        None => {
            let v = record(V_WORDS).ok()?;
            for (j, x) in key.iter().enumerate() {
                put(v, V_KEY.saturating_add(j), *x);
            }
            let slots = record(slots_n).ok()?;
            put(v, V_SLOTS, slots.as_ptr().expose_provenance());
            push_front(head, v);
            v
        }
    };
    let slots = h.words_at(get(conv, V_SLOTS), slots_n);
    let at = i.checked_mul(2)?;
    let mut block = (get(slots, at), get(slots, at.saturating_add(1)));
    if block.0 == 0 {
        match convert_entry(m, i, (from, to), loc).ok()? {
            Some(b) => {
                let b: &'static [u8] = b.leak();
                block = (addr_of(b), b.len());
            }
            None => block = (FAILED, 0),
        }
        put(slots, at.saturating_add(1), block.1);
        put(slots, at, block.0);
    }
    if block.0 == FAILED {
        return None;
    }
    let bytes = h.bytes_at(block.0, block.1);
    let word = |j: usize| -> Option<usize> { usize::try_from(u32::from_le_bytes(bytes.get(j..j.checked_add(4)?)?.try_into().ok()?)).ok() };
    let count = word(0)?;
    let k = usize::try_from(k).ok().filter(|&k| k < count).unwrap_or(0);
    block.0.checked_add(word(k.checked_add(1)?.checked_mul(4)?)?)
}

/// Translation `msgid` (with `n` for a plural lookup) in the catalog `rec`: its address, or
/// `None` if the catalog has none (or it does not convert).
fn lookup<H: Host>(h: &H, rec: &'static [AtomicUsize], msgid: &[u8], n: Option<u64>, codeset: Option<&[u8]>, loc: &Locale) -> Option<usize> {
    let image_addr = get(rec, C_IMAGE);
    let m = mo::Mo::open(h.bytes_at(image_addr, get(rec, C_IMAGE.saturating_add(1))))?;
    let i = match get(rec, C_INDEX) {
        0 => m.find(msgid)?,
        a => m.find_in(h.bytes_at(a, get(rec, C_INDEX.saturating_add(1))), msgid)?,
    };
    let k = n.map_or(0, |n| plural::evaluate(match get(rec, C_PLURAL) {
        0 => None,
        a => Some(h.bytes_at(a, get(rec, C_PLURAL.saturating_add(1)))),
    }, n));
    let plain = || image_addr.checked_add(m.form(i, k)?);
    if get(rec, C_CHARSET) == 0 {
        return plain();
    }
    // The target codec depends only on the bound codeset's text (interned: one address per
    // text) and the locale's `LC_CTYPE` data and charmap, so the last one is kept with the
    // catalog (P11).
    let selector = selector(codeset, loc);
    let kept = selector.iter().enumerate().all(|(j, w)| get(rec, C_TO.saturating_add(j)) == *w);
    // The same selector found the same codecs before (both kept codecs stay right, see
    // `cached_codec`): no conversion, nothing to resolve or compare (GETTEXT-3).
    if kept && get(rec, C_SAME) == 1 && codeset != Some(b"") {
        return plain();
    }
    let from = cached_codec(h, rec, C_FROM, || h.codec(h.bytes_at(get(rec, C_CHARSET.saturating_add(1)), get(rec, C_CHARSET.saturating_add(2)))))?;
    if codeset == Some(b"") {
        return plain();
    }
    if !kept {
        for j in C_TO..=C_SAME {
            put(rec, j, 0);
        }
    }
    let to = cached_codec(h, rec, C_TO.saturating_add(3), || match codeset {
        Some(cs) => h.codec(iconv::parse_name(cs).name),
        None => Some(iconv::of_locale(loc)),
    })?;
    for (j, w) in selector.iter().enumerate() {
        put(rec, C_TO.saturating_add(j), *w);
    }
    if from == to {
        put(rec, C_SAME, 1);
        return plain();
    }
    converted(h, rec, &m, (i, k), (from, to), loc)
}

/// What selects the target codec of a lookup (see [`lookup`]): the bound codeset's address (0:
/// none), the `LC_CTYPE` identity and the charmap's address.
fn selector(codeset: Option<&[u8]>, loc: &Locale) -> [usize; 3] {
    [codeset.map_or(0, addr_of), ctype_identity(loc), loc.charmap().map_or(0, |c| core::ptr::from_ref(c).expose_provenance())]
}

/// The hit plan for the usable catalog `rec`, just searched with `codeset` and `loc` (see
/// [`Resolution::hit`]), when a hit in it needs no conversion: the fields [`lookup`] reads from
/// the record and the image header, and, when the catalog names a charset, the selector under
/// which its codecs were found the same. `None` when a hit converts (or may: no kept answer yet).
fn plan_of<H: Host>(h: &H, rec: &[AtomicUsize], codeset: Option<&[u8]>, loc: &Locale) -> Option<[usize; P_WORDS]> {
    let mut plan = [0usize; P_WORDS];
    let mut set = |j: usize, v: usize| {
        if let Some(w) = plan.get_mut(j) {
            *w = v;
        }
    };
    if get(rec, C_STATE) != GOOD {
        return None;
    }
    if get(rec, C_CHARSET) != 0 {
        if get(rec, C_SAME) != 1 || codeset == Some(b"") {
            return None;
        }
        let sel = selector(codeset, loc);
        if !sel.iter().enumerate().all(|(j, w)| get(rec, C_TO.saturating_add(j)) == *w) {
            return None;
        }
        set(P_CHECK, 1);
        for (j, w) in sel.into_iter().enumerate() {
            set(P_SELECTOR.saturating_add(j), w);
        }
    }
    let (image, len) = (get(rec, C_IMAGE), get(rec, C_IMAGE.saturating_add(1)));
    let m = mo::Mo::open(h.bytes_at(image, len))?;
    set(P_IMAGE, image);
    set(P_IMAGE.saturating_add(1), len);
    for (j, w) in m.fields().into_iter().enumerate() {
        set(P_MO.saturating_add(j), w);
    }
    for (from, to) in [(C_INDEX, P_INDEX), (C_PLURAL, P_PLURAL)] {
        set(to, get(rec, from));
        set(to.saturating_add(1), get(rec, from.saturating_add(1)));
    }
    Some(plan)
}

/// The codec kept in the two words of `rec` at `at`, else `resolve`'s, kept when there is one (a
/// name that denotes no codec now is resolved again at the next lookup: a charmap may be loaded
/// by then; one that does is never unloaded, so a kept codec stays right).
fn cached_codec<H: Host>(h: &H, rec: &[AtomicUsize], at: usize, resolve: impl FnOnce() -> Option<Codec>) -> Option<Codec> {
    let tag = get(rec, at);
    if tag != 0 {
        let w = get(rec, at.saturating_add(1));
        if let Some(c) = Conv::from_words([tag, w, tag, w, 0, 0], &|a| h.charmap_at(a)) {
            return Some(c.from);
        }
    }
    let c = resolve()?;
    let [t, w] = codec_words(c);
    put(rec, at.saturating_add(1), w);
    put(rec, at, t);
    Some(c)
}

/// The resolution cache's slots: a lookup's catalog candidates depend only on the domain (and its
/// binding), the category, the locale name, `LANGUAGE` and the settings generation ([`SETTINGS`],
/// which moves with every binding, domain and `setlocale` change; the alias file is read once), so the ordered list of catalog records a
/// search would try is kept per such key instead of being rebuilt (paths assembled, aliases and
/// variants worked out, the catalog list searched by path) at every lookup. Each record is still
/// checked against its file as before ([`catalog_of`]), so the answers are those of the uncached
/// search. Fixed memory, per process: [`R_SLOTS`] slots of [`R_KEY_WORDS`] key words and [`R_RECS`]
/// record addresses (about 2.5 KiB); a key or a list longer than that is not cached. Read and
/// written under `LOCK`.
const R_SLOTS: usize = 4;
/// The longest key: domain, NUL, locale name, NUL, `LANGUAGE`.
const R_KEY: usize = 256;
/// The key's words: each part (domain, locale name, list) starts a word, the last word of each
/// zero-padded (two more words than [`R_KEY`] bytes, so every key of up to `R_KEY` bytes fits).
const R_KEY_WORDS: usize = R_KEY / 8 + 2;
/// The list length a slot records when the list is the locale name itself (`LANGUAGE` unset or
/// empty): the list is then not stored or compared again.
const LIST_IS_LOCALE: usize = usize::MAX;
/// The most candidates a slot holds.
const R_RECS: usize = 32;

/// A slot's hit plan ([`Resolution::hit`], GETTEXT-4), word by word: the candidate it is for
/// plus one (0: no plan); the catalog image (address, length); the image header's fields
/// ([`mo::Mo::fields`], four words); the lookup index and the plural rule (address, 0: none;
/// length); whether the selector must match (1: the catalog names a charset) and the selector
/// (three words, see [`selector`]).
const P_AT: usize = 0;
const P_IMAGE: usize = 1;
const P_MO: usize = 3;
const P_INDEX: usize = 7;
const P_PLURAL: usize = 9;
const P_CHECK: usize = 11;
const P_SELECTOR: usize = 12;
const P_WORDS: usize = 15;
/// The entry hints a slot keeps for its plan's catalog (a power of two).
const HINTS: usize = 4;

/// Which hint a message id uses: from its length and its first and last byte (cheap; a wrong guess only
/// costs the hash the lookup would have made).
fn hint_of(msgid: &[u8]) -> usize {
    let (a, z) = (msgid.first().copied().unwrap_or(0), msgid.last().copied().unwrap_or(0));
    msgid.len().wrapping_add(usize::from(a)).wrapping_add(usize::from(z)) & (HINTS - 1)
}

struct Resolution {
    /// The settings generation it was made in (0: empty).
    generation: AtomicUsize,
    category: AtomicUsize,
    /// The lengths of the key's parts: domain, locale name, list ([`LIST_IS_LOCALE`]: the list
    /// is the locale name).
    key_len: [AtomicUsize; 3],
    /// The key's parts, eight bytes to a word ([`part_words`]), one after another: compared a
    /// word at a time, straight from the caller's texts (no key is assembled).
    key: [AtomicU64; R_KEY_WORDS],
    /// The addresses of the key's domain and locale name when they were texts that are never
    /// freed nor written (the interned default domain, an interned locale name), else 0: the
    /// same address is the same text, so [`Resolution::matches`] accepts it without a compare
    /// (GETTEXT-HIT).
    key_at: [AtomicUsize; 2],
    /// The domain's binding record (0: none).
    binding: AtomicUsize,
    /// The candidates, in search order; the search ends untranslated after the last.
    len: AtomicUsize,
    recs: [AtomicUsize; R_RECS],
    /// The candidate memo ([`Memo`]): the record generation it was made in (0: none), how many
    /// leading candidates it covers, which of them are usable (bit i: candidate i), and the
    /// earliest and latest last-check times among them.
    memo_generation: AtomicUsize,
    memo_upto: AtomicUsize,
    memo_good: AtomicUsize,
    memo_first: AtomicU64,
    memo_last: AtomicU64,
    /// The TSC gate (GETTEXT-6, [`recheck::tsc_holds`]): the counter read just before the clock read the memo
    /// was last confirmed at (0: none), and the ticks the memo then had left. While the counter is within them
    /// the memo holds without reading the clock.
    memo_tsc: AtomicU64,
    memo_allow: AtomicU64,
    /// The hit plan ([`P_AT`] and on): valid only while the memo covers its candidate as usable.
    plan: [AtomicUsize; P_WORDS],
    /// Entry numbers plus one (0: none) that [`Resolution::hit`] found for keys of the plan's catalog, by
    /// [`hint_of`] (GETTEXT-FOUND): the next lookup of the same key checks that entry first and skips the hash.
    /// Cleared whenever the plan is set; read and written under `LOCK`.
    hints: [AtomicUsize; HINTS],
}

/// What the last search through a slot's candidates found out about its leading candidates
/// (GETTEXT-3): each was usable or not, and none of them would be checked again before one second
/// after its last check (D-7.12: a missing absolute path and a loaded file are checked at most once
/// a second, and at once in a new record generation). While that holds for all of them (the
/// generation is the same and the second since the earliest check has not passed), a search skips
/// their checks and takes the same answers; the first candidate that does not qualify (a relative
/// path, retried at every lookup) ends the memo. Any other search that checks one of these records
/// again does so only when its check is due, which means this memo has lapsed as well.
struct Memo {
    upto: usize,
    good: usize,
    first: u64,
    last: u64,
}

impl Memo {
    const EMPTY: Memo = Memo { upto: 0, good: 0, first: u64::MAX, last: 0 };

    /// Whether candidate `i` is usable, if the memo covers it.
    fn covers(&self, i: usize) -> Option<bool> {
        (i < self.upto).then(|| self.good.checked_shr(u32::try_from(i).unwrap_or(u32::MAX)).unwrap_or(0) & 1 != 0)
    }

    /// Add candidate `i` (just checked; `usable` is what the check returned) when it is the next
    /// one and its state stays until its next due check.
    fn extend<H: Host>(&mut self, h: &H, i: usize, rec: &[AtomicUsize], usable: bool, generation: usize) {
        if i != self.upto || i >= R_RECS {
            return;
        }
        let stays = match get(rec, C_STATE) {
            ABSENT => get(rec, C_GEN) == generation && text(h, rec, C_PATH).first() == Some(&b'/'),
            state => get(rec, C_CHECKED_GEN) == generation && usable == (state == GOOD),
        };
        let at = u64::try_from(get(rec, C_CHECKED)).unwrap_or(0);
        if !stays || at == 0 {
            return;
        }
        if usable {
            self.good |= 1usize.checked_shl(u32::try_from(i).unwrap_or(u32::MAX)).unwrap_or(0);
        }
        self.upto = i.saturating_add(1);
        self.first = self.first.min(at);
        self.last = self.last.max(at);
    }
}

impl Resolution {
    const fn new() -> Self {
        Resolution {
            generation: AtomicUsize::new(0),
            category: AtomicUsize::new(0),
            key_len: [const { AtomicUsize::new(0) }; 3],
            key: [const { AtomicU64::new(0) }; R_KEY_WORDS],
            key_at: [const { AtomicUsize::new(0) }; 2],
            binding: AtomicUsize::new(0),
            len: AtomicUsize::new(0),
            recs: [const { AtomicUsize::new(0) }; R_RECS],
            memo_generation: AtomicUsize::new(0),
            memo_upto: AtomicUsize::new(0),
            memo_good: AtomicUsize::new(0),
            memo_first: AtomicU64::new(0),
            memo_last: AtomicU64::new(0),
            memo_tsc: AtomicU64::new(0),
            memo_allow: AtomicU64::new(0),
            plan: [const { AtomicUsize::new(0) }; P_WORDS],
            hints: [const { AtomicUsize::new(0) }; HINTS],
        }
    }

    /// The candidate the hit plan is for, if `memo` (this slot's, as [`Resolution::memo`] gave it
    /// now) covers it as usable: every candidate before it is then covered as unusable (the plan
    /// is made only for the first usable one of a search from the first candidate), and none of
    /// them is due for a check, so a search would reach this one first, unchecked.
    fn planned(&self, memo: &Memo) -> Option<usize> {
        let p = self.plan.get(P_AT)?.load(Relaxed).checked_sub(1)?;
        (memo.covers(p) == Some(true)).then_some(p)
    }

    /// [`lookup`] of `msgid` in the planned catalog `rec` through the plan (GETTEXT-4): the
    /// record's image and, for a catalog that names a charset, the codec selector must still be
    /// the plan's; then only the index probe and the form remain (no record fields, no header,
    /// no codec words). `None`: the plan does not apply (search as usual); `Some(found)`: what
    /// `lookup` returns.
    fn hit<H: Host>(&self, h: &H, rec: &[AtomicUsize], msgid: &[u8], n: Option<u64>, codeset: Option<&[u8]>, loc: &Locale) -> Option<Option<usize>> {
        let w = |j: usize| self.plan.get(j).map_or(0, |a| a.load(Relaxed));
        let image = w(P_IMAGE);
        if get(rec, C_IMAGE) != image || get(rec, C_STATE) != GOOD {
            return None;
        }
        if w(P_CHECK) != 0 {
            // Word by word: an array compare here became a call (GETTEXT-4).
            let [c, t, m] = selector(codeset, loc);
            if c != w(P_SELECTOR) || t != w(P_SELECTOR.saturating_add(1)) || m != w(P_SELECTOR.saturating_add(2)) {
                return None;
            }
        }
        let fields = [w(P_MO), w(P_MO.saturating_add(1)), w(P_MO.saturating_add(2)), w(P_MO.saturating_add(3))];
        let m = mo::Mo::from_fields(h.bytes_at(image, w(P_IMAGE.saturating_add(1))), fields);
        let i = match w(P_INDEX) {
            0 => m.find(msgid),
            a => {
                let hint = self.hints.get(hint_of(msgid));
                let guess = hint.map_or(0, |x| x.load(Relaxed));
                let found = m.find_in_hinted(h.bytes_at(a, w(P_INDEX.saturating_add(1))), msgid, guess);
                if let (Some(x), Some(e)) = (hint, found) {
                    if e.saturating_add(1) != guess {
                        x.store(e.saturating_add(1), Relaxed);
                    }
                }
                found
            }
        };
        let Some(i) = i else { return Some(None) };
        let k = n.map_or(0, |n| plural::evaluate(match w(P_PLURAL) {
            0 => None,
            a => Some(h.bytes_at(a, w(P_PLURAL.saturating_add(1)))),
        }, n));
        Some(m.form(i, k).and_then(|off| image.checked_add(off)))
    }

    /// Keep `plan` (`None`: no plan). The candidate word is written last.
    fn set_plan(&self, plan: Option<&[usize; P_WORDS]>) {
        for x in &self.hints {
            x.store(0, Relaxed);
        }
        if let Some(p) = plan {
            for (a, &v) in self.plan.iter().zip(p).skip(1) {
                a.store(v, Relaxed);
            }
        }
        if let Some(a) = self.plan.get(P_AT) {
            a.store(plan.and_then(|p| p.get(P_AT)).copied().unwrap_or(0), Relaxed);
        }
    }

    /// The slot's memo if it still holds at `now` in record generation `generation`, else an
    /// empty one: no covered candidate is due for a check (see [`recheck::due`]).
    fn memo(&self, generation: usize, now: u64) -> Memo {
        let (first, last) = (self.memo_first.load(Relaxed), self.memo_last.load(Relaxed));
        if self.memo_generation.load(Relaxed) != generation || recheck::due(first, now) || recheck::due(last, now) {
            return Memo::EMPTY;
        }
        Memo { upto: self.memo_upto.load(Relaxed), good: self.memo_good.load(Relaxed), first, last }
    }

    /// The slot's memo if the TSC gate holds at counter `tsc` (no clock read), else `None`.
    fn memo_gated(&self, generation: usize, tsc: u64) -> Option<Memo> {
        if !recheck::tsc_holds(self.memo_tsc.load(Relaxed), self.memo_allow.load(Relaxed), tsc) || self.memo_generation.load(Relaxed) != generation {
            return None;
        }
        Some(Memo { upto: self.memo_upto.load(Relaxed), good: self.memo_good.load(Relaxed), first: self.memo_first.load(Relaxed), last: self.memo_last.load(Relaxed) })
    }

    /// Keep `memo` (made in record generation `generation`) for the next search. `read` is the counter
    /// and the clock value of this search's clock read, if it made one: the gate restarts from it.
    fn keep(&self, generation: usize, memo: &Memo, read: Option<(u64, u64)>) {
        if memo.upto == 0 {
            self.memo_generation.store(0, Relaxed);
            self.memo_tsc.store(0, Relaxed);
            return;
        }
        if let Some((tsc, at)) = read {
            self.memo_allow.store(recheck::tsc_allow(memo.first.min(memo.last), at), Relaxed);
            self.memo_tsc.store(tsc, Relaxed);
        }
        self.memo_upto.store(memo.upto, Relaxed);
        self.memo_good.store(memo.good, Relaxed);
        self.memo_first.store(memo.first, Relaxed);
        self.memo_last.store(memo.last, Relaxed);
        self.memo_generation.store(generation, Relaxed);
    }

    fn matches(&self, generation: usize, category: usize, key: &Key<'_>) -> bool {
        if self.generation.load(Relaxed) != generation || self.category.load(Relaxed) != category {
            return false;
        }
        let [d, l, s] = key.lens();
        let [kd, kl, ks] = &self.key_len;
        if kd.load(Relaxed) != d || kl.load(Relaxed) != l || ks.load(Relaxed) != s {
            return false;
        }
        let locale_at = d.div_ceil(8);
        let list_at = locale_at.saturating_add(l.div_ceil(8));
        let [ad, al] = key.fixed_at();
        let [sd, sl] = &self.key_at;
        // A fixed text at the address the slot recorded is the slot's text (lengths matched).
        (ad != 0 && sd.load(Relaxed) == ad || same_part(&self.key, key.domain))
            && (al != 0 && sl.load(Relaxed) == al || same_part(self.key.get(locale_at..).unwrap_or(&[]), key.locale))
            && key.list.is_none_or(|list| same_part(self.key.get(list_at..).unwrap_or(&[]), list))
    }

    fn store(&self, generation: usize, category: usize, key: &Key<'_>, binding: usize, recs: &[usize]) {
        for (at, part) in key.parts() {
            for (k, w) in self.key.iter().skip(at).zip(part_words(part)) {
                k.store(w, Relaxed);
            }
        }
        for (k, l) in self.key_len.iter().zip(key.lens()) {
            k.store(l, Relaxed);
        }
        for (k, a) in self.key_at.iter().zip(key.fixed_at()) {
            k.store(a, Relaxed);
        }
        for (r, &a) in self.recs.iter().zip(recs) {
            r.store(a, Relaxed);
        }
        self.memo_generation.store(0, Relaxed);
        self.set_plan(None);
        self.category.store(category, Relaxed);
        self.binding.store(binding, Relaxed);
        self.len.store(recs.len().min(R_RECS), Relaxed);
        self.generation.store(generation, Relaxed);
    }
}

/// The words of `part`: eight bytes to a word, the last zero-padded (built a byte at a time: no
/// copy call; GETTEXT-3).
fn part_words(part: &[u8]) -> impl Iterator<Item = u64> + '_ {
    let chunks = part.chunks_exact(8);
    let rest = chunks.remainder();
    let last = (!rest.is_empty()).then(|| tail_word(part));
    chunks.map(|c| c.try_into().map_or(0, u64::from_ne_bytes)).chain(last)
}

/// Whether the words of `part` begin `key` (a word at a time, read straight from `part`).
fn same_part(key: &[AtomicU64], part: &[u8]) -> bool {
    let Some(words) = key.get(..part.len().div_ceil(8)) else { return false };
    let chunks = part.chunks_exact(8);
    let tail = !chunks.remainder().is_empty();
    for (k, c) in words.iter().zip(chunks) {
        if k.load(Relaxed) != c.try_into().map_or(0, u64::from_ne_bytes) {
            return false;
        }
    }
    !tail || words.last().is_some_and(|k| k.load(Relaxed) == tail_word(part))
}

/// The last `len % 8` bytes of `part` as a zero-padded little-endian word (one overlapping read
/// when `part` has eight bytes or more).
fn tail_word(part: &[u8]) -> u64 {
    let rem = part.len() & 7;
    let shift = u32::try_from(8usize.wrapping_sub(rem) << 3).unwrap_or(64);
    match part.len().checked_sub(8).and_then(|at| part.get(at..)) {
        Some(last) if rem != 0 => last.try_into().map_or(0, u64::from_le_bytes).checked_shr(shift).unwrap_or(0),
        _ => mo::short_word(part.get(part.len().wrapping_sub(rem)..).unwrap_or(&[])),
    }
}

/// A resolution key: the domain, the locale name and the list of locales searched (`None`: the
/// locale name).
struct Key<'a> {
    domain: &'a [u8],
    locale: &'a [u8],
    list: Option<&'a [u8]>,
    /// Whether `domain` is the interned default domain (leaked, never written).
    domain_fixed: bool,
}

impl Key<'_> {
    /// The addresses of the parts that are fixed texts (0: compare by content): the domain when
    /// interned, and the locale name (locale names are interned and leaked, D-7.8). The list
    /// (`LANGUAGE`, environment text the program may rewrite in place) is always compared.
    fn fixed_at(&self) -> [usize; 2] {
        [if self.domain_fixed { self.domain.as_ptr().addr() } else { 0 }, self.locale.as_ptr().addr()]
    }

    fn lens(&self) -> [usize; 3] {
        [self.domain.len(), self.locale.len(), self.list.map_or(LIST_IS_LOCALE, <[u8]>::len)]
    }

    /// The parts with the word each starts at.
    fn parts(&self) -> impl Iterator<Item = (usize, &[u8])> {
        let locale_at = self.domain.len().div_ceil(8);
        let list_at = locale_at.saturating_add(self.locale.len().div_ceil(8));
        [(0, self.domain), (locale_at, self.locale)].into_iter().chain(self.list.map(|l| (list_at, l)))
    }

    /// Whether the key fits a slot: the words it takes.
    fn fits(&self) -> bool {
        let words = |p: &[u8]| p.len().div_ceil(8);
        words(self.domain).saturating_add(words(self.locale)).saturating_add(self.list.map_or(0, words)) <= R_KEY_WORDS
    }
}

/// The slots, page aligned so that the table (smaller than a page, checked below) always costs
/// one page fault, never two: unaligned, the static link placed it across a page boundary and a
/// program's first lookup touched both pages (GETTEXT-HIT-3, ls -l: +3.5 faults per process).
#[repr(C, align(4096))]
struct Slots([Resolution; R_SLOTS]);
const _: () = assert!(size_of::<Slots>() == 4096);
static RESOLVED: Slots = Slots([const { Resolution::new() }; R_SLOTS]);
/// The slot the next new resolution replaces (round robin).
static R_NEXT: AtomicUsize = AtomicUsize::new(0);
/// The slot the last lookup used, compared first (only a hint: every slot is still compared
/// in full before it is used).
static R_LAST: AtomicUsize = AtomicUsize::new(0);
/// The slot used before [`R_LAST`], compared second (a hint like it): two keys used in turn
/// (two `LANGUAGE` values alternating) hit one of the two without a scan (GETTEXT-HIT).
static R_PREV: AtomicUsize = AtomicUsize::new(0);

/// The candidates of a search (see [`translate`]'s uncached loop, which this follows step by
/// step): the binding record's address, the catalog records in order and their count; `None` when
/// they do not fit a slot (or memory runs out), in which case the search is made uncached.
fn resolve<H: Host>(h: &H, domain: &[u8], cat: Cat, list: &[u8]) -> Option<(usize, [usize; R_RECS], usize)> {
    let binding = find_binding(h, domain);
    let dir: &[u8] = match binding.map_or(0, |r| get(r, B_DIR)) {
        0 => DEFAULT_DIR.split_at(17).0,
        _ => binding.map_or(&[][..], |r| text(h, r, B_DIR)),
    };
    let mut recs = [0usize; R_RECS];
    let mut len = 0usize;
    for element in list.split(|&b| b == b':') {
        if element.is_empty() {
            continue;
        }
        if catalog::untranslated_locale(element) {
            break;
        }
        let target = alias_of(h, element).unwrap_or(element);
        let mut full = false;
        for d in dirs(dir, cat) {
            let sep: &[u8] = if d.is_empty() { b"" } else { b"/" };
            variants::each(target, |variant| {
                let parts = [d, sep, variant, b"/", cat.name(), b"/", domain, b".mo"];
                let plen = parts.iter().try_fold(0usize, |a, p| a.checked_add(p.len()))?;
                if plen >= PATH_MAX {
                    return None;
                }
                let rec = if plen <= SHORT_PATH { record_for(h, join(&mut [0u8; SHORT_PATH], &parts)?) } else { record_for(h, join(&mut [0u8; PATH_MAX], &parts)?) };
                match (rec, recs.get_mut(len)) {
                    (Some(r), Some(slot)) => {
                        *slot = r.as_ptr().expose_provenance();
                        len = len.saturating_add(1);
                        None
                    }
                    _ => {
                        full = true;
                        Some(())
                    }
                }
            });
        }
        if full {
            return None;
        }
    }
    Some((binding.map_or(0, |r| r.as_ptr().expose_provenance()), recs, len))
}

/// The translation of `msgid` in `domain` (the default domain for `None`) for the locale of
/// `category` (a C `LC_*` number), with `n` for a plural lookup: the address of the translation
/// (a NUL-terminated string that stays valid), or `None` for the untranslated text.
pub fn translate<S: Sys, H: Host>(sys: &S, h: &H, domain: Option<&[u8]>, msgid: &[u8], n: Option<u64>, category: i32) -> Option<usize> {
    let cat = if category == 6 { None } else { Cat::from_number(category) }?;
    let loc = h.locale();
    let locale_name = loc.name(cat);
    if catalog::untranslated_locale(locale_name) {
        return None;
    }
    let _g = LOCK.lock(sys);
    let domain_fixed = domain.is_none();
    let domain = domain.unwrap_or_else(|| current_domain(h));
    if domain.is_empty() {
        return None;
    }
    // A changed `LANGUAGE` selects the cached resolution for its value and is not a setting
    // change: catalogs keep their one-second clock (owner ruling 2026-10-07, P4; D-7.12).
    let language = h.language();
    let clock = Clock::new(sys.tsc());
    // A locale name holds no `:`, so it is a list of one.
    let language = language.filter(|l| !l.is_empty());
    let list = language.unwrap_or(locale_name);
    let generation = SETTINGS.load(Acquire);
    let key = Key { domain, locale: locale_name, list: language, domain_fixed };
    if key.fits() {
        let cat_no = usize::try_from(category).unwrap_or(usize::MAX);
        let matches = |i: &usize| RESOLVED.0.get(*i).is_some_and(|s| s.matches(generation, cat_no, &key));
        let last = R_LAST.load(Relaxed);
        let found = Some(last).filter(matches).or_else(|| Some(R_PREV.load(Relaxed)).filter(matches)).or_else(|| (0..R_SLOTS).find(matches));
        let slot = match found {
            Some(i) => Some(i),
            None => resolve(h, domain, cat, list).and_then(|(binding, recs, len)| {
                let i = R_NEXT.fetch_add(1, Relaxed).checked_rem(R_SLOTS)?;
                RESOLVED.0.get(i)?.store(generation, cat_no, &key, binding, recs.get(..len)?);
                Some(i)
            }),
        };
        if let Some(s) = slot.and_then(|i| {
            if i != last {
                R_PREV.store(last, Relaxed);
                R_LAST.store(i, Relaxed);
            }
            RESOLVED.0.get(i)
        }) {
            let binding = s.binding.load(Relaxed);
            let binding = (binding != 0).then(|| h.words_at(binding, B_WORDS));
            let codeset = binding.filter(|r| get(r, B_CODESET) != 0).map(|r| text(h, r, B_CODESET));
            let count = s.len.load(Relaxed);
            let records = GENERATION.load(Acquire);
            let mut memo = s.memo_gated(records, clock.tsc).unwrap_or_else(|| s.memo(records, clock.now(sys)));
            // The hit plan (GETTEXT-4): the first usable candidate, unchecked (the memo covers
            // it), searched without its record's fields. A miss there goes on with the next one.
            let mut start = 0;
            if let Some(p) = s.planned(&memo) {
                let rec = h.words_at(s.recs.get(p).map_or(0, |r| r.load(Relaxed)), C_WORDS);
                match s.hit(h, rec, msgid, n, codeset, loc) {
                    Some(Some(found)) => return Some(found),
                    Some(None) => start = p.saturating_add(1),
                    None => {}
                }
            }
            let mut plan = None;
            let mut first = true;
            let mut found = None;
            // An index loop: a `skip` adapter here kept its state in memory and cost a call per
            // search (GETTEXT-4).
            for i in start..count.min(R_RECS) {
                let Some(r) = s.recs.get(i) else { break };
                let rec = h.words_at(r.load(Relaxed), C_WORDS);
                // A candidate the memo covers is not checked: no check of it is due yet.
                let usable = match memo.covers(i) {
                    Some(usable) => usable,
                    None => {
                        let usable = catalog_of(sys, h, rec, &clock).is_some();
                        memo.extend(h, i, rec, usable, records);
                        usable
                    }
                };
                if usable {
                    found = lookup(h, rec, msgid, n, codeset, loc);
                    // A plan only for the first usable candidate of a search from the first, and
                    // only while the memo covers it.
                    if start == 0 && first && memo.covers(i) == Some(true) {
                        plan = plan_of(h, rec, codeset, loc).map(|mut w| {
                            if let Some(at) = w.get_mut(P_AT) {
                                *at = i.saturating_add(1);
                            }
                            w
                        });
                    }
                    first = false;
                    if found.is_some() {
                        break;
                    }
                }
            }
            s.keep(records, &memo, clock.read());
            if start == 0 {
                s.set_plan(plan.as_ref());
            }
            return found;
        }
    }
    translate_unresolved(sys, h, cat, domain, list, msgid, n, loc, &clock)
}

/// The search of [`translate`] without a resolution slot (the key does not fit, or no slot could
/// be made): every candidate path is built and checked in turn. Out of line (GETTEXT-5): its path
/// buffers made `translate`'s frame several KiB, with a stack probe and spills on the found path.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn translate_unresolved<S: Sys, H: Host>(sys: &S, h: &H, cat: Cat, domain: &[u8], list: &[u8], msgid: &[u8], n: Option<u64>, loc: &Locale, clock: &Clock) -> Option<usize> {
    let binding = find_binding(h, domain);
    let dir: &[u8] = match binding.map_or(0, |r| get(r, B_DIR)) {
        0 => DEFAULT_DIR.split_at(17).0,
        _ => binding.map_or(&[][..], |r| text(h, r, B_DIR)),
    };
    let codeset = binding.filter(|r| get(r, B_CODESET) != 0).map(|r| text(h, r, B_CODESET));
    for element in list.split(|&b| b == b':') {
        if element.is_empty() {
            continue;
        }
        if catalog::untranslated_locale(element) {
            return None;
        }
        let target = alias_of(h, element).unwrap_or(element);
        for d in dirs(dir, cat) {
            let sep: &[u8] = if d.is_empty() { b"" } else { b"/" };
            let found = variants::each(target, |variant| {
                let parts = [d, sep, variant, b"/", cat.name(), b"/", domain, b".mo"];
                let len = parts.iter().try_fold(0usize, |a, p| a.checked_add(p.len()))?;
                if len >= PATH_MAX {
                    return None;
                }
                // The path is assembled in a buffer that fits it: the usual short one costs no
                // clearing of PATH_MAX bytes per candidate.
                let rec = if len <= SHORT_PATH { catalog_at(sys, h, join(&mut [0u8; SHORT_PATH], &parts)?, clock)? } else { catalog_at(sys, h, join(&mut [0u8; PATH_MAX], &parts)?, clock)? };
                lookup(h, rec, msgid, n, codeset, loc)
            });
            if found.is_some() {
                return found;
        }
        }
    }
    None
}

/// The directories a search tries for each language element, in order: the domain's own, then
/// for `LC_MESSAGES` the language-pack directory (#411; probed: also for a domain bound
/// elsewhere, never for another category).
fn dirs(dir: &[u8], cat: Cat) -> impl Iterator<Item = &[u8]> {
    core::iter::once(dir).chain((cat == Cat::Messages).then_some(LANGPACK_DIR))
}

// ------------------------------------------------------------------------------ catopen (§7.8)

/// `catopen(name, …)` with `nlspath` (`NLSPATH`, already read through `secure_getenv`) and the
/// locale name the call uses: the validated image, or the error. A name with `/` is a path;
/// otherwise the `NLSPATH` and default templates are tried in order: a missing or unreadable file
/// moves on (`ENOENT` at the end), anything else that is not a catalog (a directory, a FIFO, an
/// empty or damaged file) ends the search with `EINVAL` (as observed for directories and short
/// files; the reference leaves `errno` 0 for the others, spec C0.1 requires an error code).
pub fn catopen<H: Host>(h: &H, nlspath: Option<&[u8]>, name: &[u8], locale: &[u8]) -> Result<Image, Errno> {
    let open = |path: &[u8]| -> Result<Image, Option<Errno>> {
        match h.load(path) {
            Load::Missing(e) => Err(Some(e)),
            Load::NotRegular => Err(None),
            Load::Image(img) => {
                if xopen::Cat::parse(h.bytes_at(img.addr, img.len), img.file_len).is_some() {
                    Ok(img)
                } else {
                    h.unload(&img);
                    Err(None)
                }
            }
        }
    };
    if name.contains(&b'/') {
        return open(name).map_err(|e| e.unwrap_or(Errno::EINVAL));
    }
    xopen::each_path(nlspath, name, locale, |path| match open(path) {
        Ok(img) => Some(Ok(img)),
        Err(Some(_)) => None,
        Err(None) => Some(Err(Errno::EINVAL)),
    })
    .unwrap_or(Err(Errno::ENOENT))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    /// The slot's TSC gate with an injected counter (GETTEXT-6): the memo is served without the clock
    /// only while the counter is within the allowance of the read it was confirmed at; a counter that
    /// went backwards, wrapped, jumped far, or a slot never read all say "read the clock".
    #[test]
    fn memo_tsc_gate() {
        let s = Resolution::new();
        let memo = Memo { upto: 2, good: 0b01, first: 1_000, last: 1_500 };
        assert!(s.memo_gated(7, 100).is_none(), "never confirmed");
        s.keep(7, &memo, Some((100, 1_600)));
        let allow = s.memo_allow.load(Relaxed);
        assert!(allow > 1 << 27 && allow <= recheck::TSC_SPAN, "nearly the whole interval is left: {allow}");
        let got = s.memo_gated(7, 100 + 5).unwrap();
        assert_eq!((got.upto, got.good, got.first, got.last), (2, 0b01, 1_000, 1_500));
        assert!(s.memo_gated(8, 105).is_none(), "another record generation");
        assert!(s.memo_gated(7, 99).is_none(), "backwards");
        assert!(s.memo_gated(7, 100 + allow).is_none(), "the allowance is used up");
        assert!(s.memo_gated(7, 100 + recheck::TSC_SPAN * 8).is_none(), "a huge jump");
        assert!(s.memo_gated(7, 0).is_none(), "no counter");
        let wrap = Resolution::new();
        wrap.keep(7, &memo, Some((u64::MAX - 3, 1_600)));
        assert!(wrap.memo_gated(7, 4).is_none(), "wrapped");
        // A memo whose oldest check is nearly due has little left; one already due has none.
        let late = Resolution::new();
        late.keep(7, &memo, Some((100, 1_000 + recheck::INTERVAL_NS - 10)));
        assert!(late.memo_allow.load(Relaxed) <= 3);
        late.keep(7, &memo, Some((100, 1_000 + recheck::INTERVAL_NS)));
        assert!(late.memo_gated(7, 100).is_none(), "already due");
        // A search without a clock read leaves the gate as it was; an empty memo clears it.
        s.keep(7, &memo, None);
        assert!(s.memo_gated(7, 105).is_some());
        s.keep(7, &Memo::EMPTY, None);
        assert!(s.memo_gated(7, 105).is_none());
    }

    /// A slot accepts a part by address only when both the slot and the key took it from a fixed
    /// text (GETTEXT-HIT): a caller's domain text rewritten in place at the same address, or a
    /// different text at another address, is told apart by content.
    #[test]
    fn key_address_accept() {
        let r = Resolution::new();
        let mut caller = *b"domain_a";
        let (de, fr) = (b"de_DE.UTF-8".as_slice(), b"fr_FR.UTF-8".as_slice());
        r.store(1, 5, &Key { domain: &caller, locale: de, list: None, domain_fixed: false }, 0, &[]);
        caller[7] = b'b';
        // Same address, rewritten text: the slot recorded no address for a caller's text.
        assert!(!r.matches(1, 5, &Key { domain: &caller, locale: de, list: None, domain_fixed: true }));
        caller[7] = b'a';
        assert!(r.matches(1, 5, &Key { domain: &caller, locale: de, list: None, domain_fixed: true }));
        let fixed = b"domain_c".as_slice();
        r.store(1, 5, &Key { domain: fixed, locale: de, list: None, domain_fixed: true }, 0, &[]);
        assert!(r.matches(1, 5, &Key { domain: fixed, locale: de, list: None, domain_fixed: true }));
        assert!(!r.matches(1, 5, &Key { domain: b"domain_d", locale: de, list: None, domain_fixed: true }));
        assert!(!r.matches(1, 5, &Key { domain: fixed, locale: fr, list: None, domain_fixed: true }));
        let (d2, l2) = (b"domain_c".to_vec(), de.to_vec());
        assert!(r.matches(1, 5, &Key { domain: &d2, locale: &l2, list: None, domain_fixed: false }));
    }

    /// The tail word as the zero-padded little-endian word of the last `len % 8` bytes, whatever
    /// the length (one overlapping read from eight bytes on); `same_part` agrees with
    /// `part_words` (what `store` writes) and tells every other text of the length apart.
    #[test]
    fn key_words() {
        let text: Vec<u8> = (1..=40u8).collect();
        for len in 0..=text.len() {
            let part = &text[..len];
            let rem = len % 8;
            let mut padded = [0u8; 8];
            padded[..rem].copy_from_slice(&part[len - rem..]);
            if rem != 0 {
                assert_eq!(tail_word(part), u64::from_le_bytes(padded), "len {len}");
            }
            let stored: Vec<AtomicU64> = part_words(part).map(AtomicU64::new).collect();
            assert_eq!(stored.len(), len.div_ceil(8));
            assert!(same_part(&stored, part), "len {len}");
            for i in 0..len {
                let mut other = part.to_vec();
                other[i] ^= 0x80;
                assert!(!same_part(&stored, &other), "len {len}, byte {i}");
            }
            assert!(!same_part(&stored[..stored.len().saturating_sub(1)], part) || len == 0, "len {len}: too few words");
        }
    }
}
