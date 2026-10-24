//! The process-global locale of `setlocale`, the interned locale values, and the mapped locale
//! files (spec §7.1–7.3; D-7.1, D-7.7, D-7.8).
//!
//! **Interned locales.** Every locale the process selects or creates is kept, deduplicated, for
//! the rest of the process: a handle (index + 1; 0 is `"C"`) names it, so a thread that read a
//! handle never sees its locale freed, and `setlocale` stays safe while other threads use the
//! global locale (D-7.1). Distinct locales are few (one per combination of categories a
//! process uses); the table holds [`CAPACITY`]. Names are interned the same way.
//!
//! **Files.** Locale files are mapped once, by path, and never unmapped (D-7.7). Charmaps
//! (D-6.8) are read from `/usr/share/i18n/charmaps`, parsed once, and kept.
//!
//! **Invalidation** (D-X.13). [`invalidate`] advances a generation. The next use of a mapped file
//! in a new generation compares the file's identity (`stat`) with the one recorded when it was
//! mapped: unchanged, the mapping is used again; changed, the file is mapped again (the old
//! mapping stays, for the locales that point into it); gone, it is missing. The mappings are
//! bounded ([`FILES`] distinct files, [`MAP_LIMIT`] mappings in all): past a limit, a load that
//! needs a new mapping fails as for a missing file. A charmap is parsed
//! again only when its file was mapped again. Then the global locale is loaded again by its names.
//!
//! The values live at addresses this module leaked; the POSIX layer turns an address back into
//! a reference ([`Objects`]) and maps files ([`Mapper`]).

use core::sync::atomic::{fence, AtomicU64, AtomicUsize, Ordering::AcqRel, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};

use liella::charmap::{self, inflate, Charmap};
use liella::iconv::{self, Codec};
use liella::text::CharmapRef;
use liella::error::{Errno, KResult};
use liella::locale::file::Cat;
use liella::locale::view::LocaleView;
use liella::locale::{self, name, Data, Locale, Source};
use liella::mem::{TryBox, TryVec};
use liella_kernel::sys::Sys;
use liella_rt::sync::RawMutex;

/// The most distinct locales, and separately names, a process can intern.
pub const CAPACITY: usize = 1024;
/// The most locale files a process maps (each distinct path once; D-7.7).
pub const FILES: usize = 1024;
/// The most mappings a process makes in all: every distinct file once, and a file replaced and
/// mapped again after an invalidation (D-X.13) counts again. Past either limit a load that would
/// need a new mapping fails (the locale is not found), so the mapped memory is bounded; nothing
/// is ever mapped without being recorded.
pub const MAP_LIMIT: usize = 2 * FILES;
static MAPS: AtomicUsize = AtomicUsize::new(0);

/// Entry `i` of the three tables: the `i`th interned locale, the `i`th interned name and the
/// `i`th mapped file (each table counts on its own). The tables are interleaved, one record per
/// index, so a process that loads one locale touches one page of them, not one per column
/// (separate 8 KiB columns cost one page fault each; task PAGEFAULTS).
struct Slot {
    interned: AtomicUsize,
    name_addr: AtomicUsize,
    name_len: AtomicUsize,
    file_name: AtomicUsize,
    file_addr: AtomicUsize,
    file_len: AtomicUsize,
    /// The generation the file was last checked in.
    file_gen: AtomicUsize,
    /// The file's identity ([`Identity`]).
    file_id: [AtomicU64; 7],
}
const _: () = assert!(CAPACITY == FILES);
static SLOTS: [Slot; CAPACITY] = [const {
    Slot {
        interned: AtomicUsize::new(0),
        name_addr: AtomicUsize::new(0),
        name_len: AtomicUsize::new(0),
        file_name: AtomicUsize::new(0),
        file_addr: AtomicUsize::new(0),
        file_len: AtomicUsize::new(0),
        file_gen: AtomicUsize::new(0),
        file_id: [const { AtomicU64::new(0) }; 7],
    }
}; CAPACITY];
static COUNT: AtomicUsize = AtomicUsize::new(0);
static NAME_COUNT: AtomicUsize = AtomicUsize::new(0);
static FILE_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The most charmaps a process loads.
const CHARMAPS: usize = 32;
static CHARMAP_ADDR: [AtomicUsize; CHARMAPS] = [const { AtomicUsize::new(0) }; CHARMAPS];
static CHARMAP_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Each charmap's file (its interned path's slot), the address of the mapping it was parsed
/// from, and the generation it was last checked in.
static CHARMAP_PATH: [AtomicUsize; CHARMAPS] = [const { AtomicUsize::new(0) }; CHARMAPS];
static CHARMAP_SRC: [AtomicUsize; CHARMAPS] = [const { AtomicUsize::new(0) }; CHARMAPS];
static CHARMAP_GEN: [AtomicUsize; CHARMAPS] = [const { AtomicUsize::new(0) }; CHARMAPS];
/// The invalidation generation (D-X.13).
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// A file's identity: device, inode, size, modification and change times (seconds and
/// nanoseconds).
pub type Identity = [u64; 7];
/// The installed charmaps (D-6.8).
const CHARMAP_DIR: &[u8] = b"/usr/share/i18n/charmaps/";
/// The largest charmap text inflated (the largest installed is a few MiB).
const CHARMAP_LIMIT: usize = 32 << 20;
static GLOBAL: AtomicUsize = AtomicUsize::new(0);
/// Serializes interning, mapping and `setlocale`/`newlocale` (loading may read files).
static LOCK: RawMutex = RawMutex::new();

/// Address-to-reference conversions for values this module leaked (the POSIX layer).
pub trait Objects {
    /// The locale view at `addr`, an address [`intern`] recorded (D-7.13).
    fn view_at(&self, addr: usize) -> &'static LocaleView;
    /// The `len` bytes at `addr`: a name this module recorded, or a mapping [`Mapper::map`]
    /// returned.
    fn bytes_at(&self, addr: usize, len: usize) -> &'static [u8];
    /// The charmap at `addr`, an address [`charmap`] recorded.
    fn charmap_at(&self, addr: usize) -> &'static Charmap;
}

/// The file system access locale loading needs (the POSIX layer).
pub trait Mapper: Objects {
    /// Map the file at `path` read-only, for the rest of the process: its address, length and
    /// identity.
    fn map(&self, path: &[u8]) -> Option<(usize, usize, Identity)>;
    /// Unmap a mapping [`Mapper::map`] just returned and nothing has used (it could not be
    /// recorded).
    fn unmap(&self, addr: usize, len: usize);
    /// The identity of the file at `path` now (`None`: it cannot be examined).
    fn identity(&self, path: &[u8]) -> Option<Identity>;
    /// `LOCPATH`, unless unset or in secure mode.
    fn locpath(&self) -> Option<&[u8]>;
    /// Read the first bytes of the file at `path` into `buf` (without mapping it): the count.
    fn read_prefix(&self, path: &[u8], buf: &mut [u8]) -> Option<usize>;
    /// Call `f` with the name of each entry of the directory `path`.
    fn list_dir(&self, path: &[u8], f: &mut dyn FnMut(&[u8]));
}

fn addr_of<T: ?Sized>(r: &'static T) -> usize {
    core::ptr::from_ref(r).cast::<u8>().expose_provenance()
}

fn load(a: &[AtomicUsize], i: usize) -> usize {
    a.get(i).map_or(0, |x| x.load(Acquire))
}

/// Field `f` of slot `i` (0 past the end).
fn field(f: fn(&'static Slot) -> &'static AtomicUsize, i: usize) -> usize {
    SLOTS.get(i).map_or(0, |s| f(s).load(Acquire))
}

/// The view of the `"C"` locale, as a static for [`view`].
static C: LocaleView = LocaleView::c();

/// The interned locale named by `handle` (0: `"C"`).
pub fn get(objs: &impl Objects, handle: usize) -> &'static Locale {
    view(objs, handle).locale()
}

/// The view of the interned locale named by `handle` (0, and a value that names none: `"C"`'s;
/// D-7.13).
pub fn view(objs: &impl Objects, handle: usize) -> &'static LocaleView {
    let addr = handle.checked_sub(1).map_or(0, |i| field(|s| &s.interned, i));
    if addr == 0 { &C } else { objs.view_at(addr) }
}

/// The handle of `l`, interning it if new: its view is built here, once, before the handle is
/// published (D-7.13). The caller holds [`LOCK`].
fn intern_locked(objs: &impl Objects, l: Locale) -> KResult<usize> {
    if l == Locale::C {
        return Ok(0);
    }
    let n = COUNT.load(Acquire);
    if let Some(i) = (0..n).find(|&i| *objs.view_at(field(|s| &s.interned, i)).locale() == l) {
        return Ok(i.saturating_add(1));
    }
    let slot = &SLOTS.get(n).ok_or(Errno::ENOMEM)?.interned;
    // A category file an earlier view already tabulated is not tabulated again (LV-3).
    let reuse = |cat: Cat, d: &Data| {
        (0..n).find_map(|i| {
            let earlier = objs.view_at(field(|s| &s.interned, i)).locale();
            if earlier.data(cat).same(d) { earlier.table(cat) } else { None }
        })
    };
    let leaked: &'static LocaleView = TryBox::new(LocaleView::new(l, n.saturating_add(1), &reuse)?)?.leak();
    slot.store(addr_of(leaked), Release);
    COUNT.store(n.saturating_add(1), Release);
    Ok(n.saturating_add(1))
}

/// The handle of `l`, interning it if new.
pub fn intern<S: Sys>(sys: &S, objs: &impl Objects, l: Locale) -> KResult<usize> {
    let _g = LOCK.lock(sys);
    intern_locked(objs, l)
}

/// The interned copy of `name` (deduplicated), `'static` and followed by a NUL, and its slot.
/// The caller holds [`LOCK`].
fn intern_name_locked(objs: &impl Objects, name: &[u8]) -> KResult<(usize, &'static [u8])> {
    let n = NAME_COUNT.load(Acquire);
    for i in 0..n {
        let stored = objs.bytes_at(field(|s| &s.name_addr, i), field(|s| &s.name_len, i));
        if stored == name {
            return Ok((i, stored));
        }
    }
    let e = SLOTS.get(n).ok_or(Errno::ENOMEM)?;
    let (addr, len) = (&e.name_addr, &e.name_len);
    let mut v = TryVec::with_capacity(name.len().saturating_add(1))?;
    v.extend_from_slice(name)?;
    v.push(0)?;
    let leaked: &'static [u8] = v.leak();
    addr.store(addr_of(leaked), Release);
    len.store(name.len(), Release);
    NAME_COUNT.store(n.saturating_add(1), Release);
    Ok((n, leaked.get(..name.len()).unwrap_or(&[])))
}

/// `name` interned, followed by a NUL in memory (for `_NL_LOCALE_NAME`, whose result is a C
/// string).
pub fn name_z<S: Sys>(sys: &S, objs: &impl Objects, name: &[u8]) -> KResult<&'static [u8]> {
    let _g = LOCK.lock(sys);
    intern_name_locked(objs, name).map(|(_, s)| s)
}

/// The global locale's handle.
pub fn global_handle() -> usize {
    GLOBAL.load(Acquire)
}

/// The global locale.
pub fn global(objs: &impl Objects) -> &'static Locale {
    get(objs, global_handle())
}

/// A locale [`Source`] over the mapper, through the file cache. Used with [`LOCK`] held.
struct Files<'a, M: Mapper> {
    m: &'a M,
    /// `m.locpath()`, read at the first need and kept for the selection (SETLOCALE-3: read
    /// once per category before).
    locpath: core::cell::OnceCell<Option<&'a [u8]>>,
}

/// Record file slot `i`: the mapping, its identity, checked in `generation`. The caller holds
/// [`LOCK`].
fn record_file(i: usize, addr: usize, len: usize, id: &Identity, generation: usize) {
    if let Some(e) = SLOTS.get(i) {
        let (a, l, w, g) = (&e.file_addr, &e.file_len, &e.file_id, &e.file_gen);
        a.store(addr, Release);
        l.store(len, Release);
        for (slot, v) in w.iter().zip(id) {
            slot.store(*v, Release);
        }
        g.store(generation, Release);
    }
}

fn file_id(i: usize) -> Identity {
    let mut id = [0u64; 7];
    if let Some(w) = SLOTS.get(i).map(|e| &e.file_id) {
        for (v, slot) in id.iter_mut().zip(w) {
            *v = slot.load(Acquire);
        }
    }
    id
}

/// `liella_locale_invalidate`'s first half (D-X.13): every mapped file and parsed charmap is
/// checked against its file before its next use. The new generation.
pub fn invalidate() -> usize {
    GENERATION.fetch_add(1, Release).wrapping_add(1)
}

impl<M: Mapper> Source for Files<'_, M> {
    fn file(&self, path: &[u8]) -> Option<&'static [u8]> {
        let generation = GENERATION.load(Acquire);
        let n = FILE_COUNT.load(Acquire);
        for i in 0..n {
            let slot = field(|s| &s.file_name, i);
            if self.m.bytes_at(field(|s| &s.name_addr, slot), field(|s| &s.name_len, slot)) != path {
                continue;
            }
            let current = self.m.bytes_at(field(|s| &s.file_addr, i), field(|s| &s.file_len, i));
            if field(|s| &s.file_gen, i) == generation {
                return Some(current);
            }
            // The first use since an invalidation: the same file is used again, a changed one
            // mapped again (the old mapping stays for the locales that point into it).
            if self.m.identity(path) == Some(file_id(i)) {
                if let Some(g) = SLOTS.get(i).map(|e| &e.file_gen) {
                    g.store(generation, Release);
                }
                return Some(current);
            }
            if MAPS.load(Acquire) >= MAP_LIMIT {
                return None;
            }
            let (addr, len, id) = self.m.map(path)?;
            MAPS.fetch_add(1, Release);
            record_file(i, addr, len, &id, generation);
            return Some(self.m.bytes_at(addr, len));
        }
        // A new file: only when it can be recorded (a full table fails the load cleanly, as a
        // missing file does, instead of mapping again at every call).
        let fname = &SLOTS.get(n)?.file_name;
        if MAPS.load(Acquire) >= MAP_LIMIT {
            return None;
        }
        let (addr, len, id) = self.m.map(path)?;
        let Ok((slot, _)) = intern_name_locked(self.m, path) else {
            self.m.unmap(addr, len);
            return None;
        };
        MAPS.fetch_add(1, Release);
        fname.store(slot, Release);
        record_file(n, addr, len, &id, generation);
        FILE_COUNT.store(n.saturating_add(1), Release);
        Some(self.m.bytes_at(addr, len))
    }

    fn locpath(&self) -> Option<&[u8]> {
        *self.locpath.get_or_init(|| self.m.locpath())
    }

    fn charmap(&self, name: &[u8]) -> Option<&'static Charmap> {
        let n = CHARMAP_COUNT.load(Acquire);
        if let Some(i) = (0..n).find(|&i| names(self.m.charmap_at(load(&CHARMAP_ADDR, i)), name)) {
            return self.current_charmap(i);
        }
        let slot = CHARMAP_ADDR.get(n)?;
        let parsed = match self.read_charmap(name) {
            Some(cm) => cm,
            None => self.read_charmap(&self.find_charmap(name)?)?,
        };
        // The file found for an alias may be one already loaded under its own name.
        if let Some(i) = (0..n).find(|&i| self.m.charmap_at(load(&CHARMAP_ADDR, i)).name() == parsed.0.name()) {
            return self.current_charmap(i);
        }
        let cm = TryBox::new(parsed.0).ok()?.leak();
        slot.store(addr_of(cm), Release);
        record_charmap(n, parsed.1, parsed.2);
        CHARMAP_COUNT.store(n.saturating_add(1), Release);
        Some(cm)
    }
}

/// Record charmap slot `i`'s file (the interned path's slot) and the mapping it was parsed from,
/// checked in the current generation. The caller holds [`LOCK`].
fn record_charmap(i: usize, path: usize, src: usize) {
    if let (Some(p), Some(s), Some(g)) = (CHARMAP_PATH.get(i), CHARMAP_SRC.get(i), CHARMAP_GEN.get(i)) {
        p.store(path, Release);
        s.store(src, Release);
        g.store(GENERATION.load(Acquire), Release);
    }
}

/// Whether `name` names `cm`: its name or an alias, compared like codeset names (case and
/// punctuation aside: `eucjp` is `EUC-JP`).
fn names(cm: &Charmap, name: &[u8]) -> bool {
    core::iter::once(cm.name()).chain(cm.aliases()).any(|n| name::same_codeset(n, name))
}

/// A charmap file name (without `.gz`).
#[derive(Clone, Copy)]
struct Stem([u8; 128], usize);

impl core::ops::Deref for Stem {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.0.get(..self.1).unwrap_or(&[])
    }
}

impl<M: Mapper> Files<'_, M> {
    /// The charmap file whose name or header names `name`, by a scan of the directory: first a
    /// file name that matches, then the header names (`<code_set_name>`, aliases).
    fn find_charmap(&self, name: &[u8]) -> Option<Stem> {
        let mut entries: TryVec<Stem> = TryVec::new();
        self.m.list_dir(CHARMAP_DIR.strip_suffix(b"/").unwrap_or(CHARMAP_DIR), &mut |e| {
            let stem = e.strip_suffix(b".gz").unwrap_or(e);
            if stem.first() == Some(&b'.') || stem.is_empty() {
                return;
            }
            let mut s = Stem([0; 128], stem.len());
            if let Some(dst) = s.0.get_mut(..stem.len()) {
                dst.copy_from_slice(stem);
                let _ = entries.push(s);
            }
        });
        if let Some(i) = entries.iter().position(|s| name::same_codeset(s, name)) {
            return entries.get(i).copied();
        }
        let mut raw = [0u8; 4096];
        let mut path = [0u8; 256];
        for &s in entries.iter() {
            let mut found = false;
            for ext in [&b".gz"[..], b""] {
                let total = CHARMAP_DIR.len().saturating_add(s.len());
                let Some(p) = path.get_mut(..total.saturating_add(ext.len())) else { continue };
                let (dir, rest) = p.split_at_mut(CHARMAP_DIR.len());
                dir.copy_from_slice(CHARMAP_DIR);
                let (stem, tail) = rest.split_at_mut(s.len());
                stem.copy_from_slice(&s);
                tail.copy_from_slice(ext);
                let Some(n) = self.m.read_prefix(p, &mut raw) else { continue };
                let data = raw.get(..n).unwrap_or(&[]);
                let mut check = |t: &[u8]| charmap::header_names(t, &mut |h| found |= name::same_codeset(h, name));
                if ext.is_empty() {
                    check(data);
                } else if let Ok(t) = inflate::gunzip_prefix(data, 8192) {
                    check(&t);
                }
                break;
            }
            if found {
                return Some(s);
            }
        }
        None
    }

    /// Parse the charmap file for `name` (`<name>.gz`, else `<name>`): the charmap, its path's
    /// interned slot, and the address of the mapping it was parsed from. A name holding `/`, or
    /// `.` or `..`, is not a file name.
    fn read_charmap(&self, name: &[u8]) -> Option<(Charmap, usize, usize)> {
        if !name::component(name) {
            return None;
        }
        let mut buf = [0u8; 256];
        let total = CHARMAP_DIR.len().checked_add(name.len())?;
        buf.get_mut(..CHARMAP_DIR.len())?.copy_from_slice(CHARMAP_DIR);
        buf.get_mut(CHARMAP_DIR.len()..total)?.copy_from_slice(name);
        buf.get_mut(total..total.checked_add(3)?)?.copy_from_slice(b".gz");
        for path in [buf.get(..total.checked_add(3)?)?, buf.get(..total)?] {
            if let Some(raw) = self.file(path) {
                let cm = parse_charmap(path, raw)?;
                let (slot, _) = intern_name_locked(self.m, path).ok()?;
                return Some((cm, slot, raw.as_ptr().expose_provenance()));
            }
        }
        None
    }

    /// Charmap slot `i`, checked against its file in the current generation: parsed again (in
    /// the same slot; the old charmap stays for its users) only when the file was mapped again.
    fn current_charmap(&self, i: usize) -> Option<&'static Charmap> {
        let cm = self.m.charmap_at(load(&CHARMAP_ADDR, i));
        let generation = GENERATION.load(Acquire);
        if load(&CHARMAP_GEN, i) == generation {
            return Some(cm);
        }
        let slot = load(&CHARMAP_PATH, i);
        let path = self.m.bytes_at(field(|s| &s.name_addr, slot), field(|s| &s.name_len, slot));
        let raw = self.file(path)?;
        let src = raw.as_ptr().expose_provenance();
        if src != load(&CHARMAP_SRC, i) {
            let fresh: &'static Charmap = TryBox::new(parse_charmap(path, raw)?).ok()?.leak();
            CHARMAP_ADDR.get(i)?.store(addr_of(fresh), Release);
            record_charmap(i, slot, src);
            return Some(fresh);
        }
        record_charmap(i, slot, src);
        Some(cm)
    }
}

/// Parse the charmap text `raw` read from `path` (inflated first for a `.gz` file).
fn parse_charmap(path: &[u8], raw: &[u8]) -> Option<Charmap> {
    if path.ends_with(b".gz") {
        let text = inflate::gunzip(raw, CHARMAP_LIMIT).ok()?;
        return Charmap::parse(&text).ok();
    }
    Charmap::parse(raw).ok()
}

/// The codec `name` (without suffixes) denotes for `iconv_open`: built in, else an installed
/// charmap found by its name or an alias (D-6.8, D-6.9).
pub fn codec<S: Sys, M: Mapper>(sys: &S, m: &M, name: &[u8]) -> Option<Codec> {
    if let Some(c) = iconv::builtin(name) {
        return Some(c);
    }
    let _g = LOCK.lock(sys);
    Files { m, locpath: core::cell::OnceCell::new() }.charmap(name).map(|c| Codec::Charmap(CharmapRef(c)))
}

/// The name `setlocale(cat, "")` takes for `cat`: `LC_ALL`, then `LC_<cat>`, then `LANG`
/// (empty values ignored), else `"C"`.
pub fn env_name<'e>(cat: Cat, getenv: &dyn Fn(&[u8]) -> Option<&'e [u8]>) -> &'e [u8] {
    let nonempty = |k: &[u8]| getenv(k).filter(|v| !v.is_empty());
    nonempty(b"LC_ALL").or_else(|| nonempty(cat.name())).or_else(|| nonempty(b"LANG")).unwrap_or(b"C")
}

/// What `setlocale` is asked.
pub enum Request<'a> {
    /// Query the current name.
    Query,
    /// Take each category's name from the environment (values live while the call runs).
    Environment(&'a dyn Fn(&[u8]) -> Option<&'static [u8]>),
    /// Use this name: for `LC_ALL`, a single name or a composite name.
    Name(&'a [u8]),
}

/// `base` with the categories in `cats` replaced by the locales `names` gives them (indexed
/// like [`Cat::ALL`]). The caller holds [`LOCK`].
fn replace<M: Mapper>(m: &M, base: Locale, cats: &[Cat], names: &[&[u8]; 12]) -> Option<Locale> {
    let src = Files { m, locpath: core::cell::OnceCell::new() };
    let mut l = base;
    // The alias and the archive search are shared by the categories of one name.
    let mut cache = locale::FindCache::default();
    for &c in cats {
        let (_, stored) = intern_name_locked(m, names.get(c.index()).copied()?).ok()?;
        let data = locale::find_cached(&src, c, stored, &mut cache)?;
        let reported = locale::builtin(stored).map_or(stored, |(_, r)| r);
        l = l.with(c, reported, data)?;
    }
    locale::attach_charmap(&src, l)
}

/// [`replace`] for `setlocale` (§7.2, s7_2_c_4): a category whose current name is the name
/// asked for keeps its data, without a lookup (no file is read, `LOCPATH` is not consulted
/// again), as the reference does (SETLOCALE-4 probe). `current` itself when every category asked
/// keeps its name. The caller holds [`LOCK`]. `current` by reference: one copy of the locale
/// fewer on the first `setlocale` of a process (SETLOCALE-4 trace).
fn replace_changed<M: Mapper>(m: &M, current: &Locale, cats: &[Cat], names: &[&[u8]; 12]) -> Option<Locale> {
    let mut changed = [Cat::Ctype; 12];
    let mut n = 0;
    for &c in cats {
        if names.get(c.index()).copied()? != current.name(c) {
            *changed.get_mut(n)? = c;
            n = n.saturating_add(1);
        }
    }
    if n == cats.len() {
        // Nothing kept (the common first call): the caller's list as it is.
        return replace(m, *current, cats, names);
    }
    match changed.get(..n)? {
        [] => Some(*current),
        some => replace(m, *current, some, names),
    }
}

/// The query cache (WL-HOT-LS): for the global locale `QUERY_HANDLE`, each category's answer
/// to `setlocale(cat, NULL)` (index 12: `LC_ALL`'s composite name) as its interned name's
/// slot + 1 (0: not cached yet). Programs query once per output line (`ls -l`: gnulib's
/// `hard_locale (LC_TIME)`), and the locked path costs ~1,200 instructions against ~100.
/// Written only with [`LOCK`] held; a handle change runs between two `QUERY_SEQ` increments
/// (odd while it runs), so a lock-free reader that sees the same even sequence number before
/// and after its reads saw one handle's slots. A name for a handle never changes (the handle
/// names one interned locale) and name slots are never reused, so a hit is always right.
static QUERY_SEQ: AtomicUsize = AtomicUsize::new(0);
static QUERY_HANDLE: AtomicUsize = AtomicUsize::new(0);
static QUERY_SLOT: [AtomicUsize; 13] = [const { AtomicUsize::new(0) }; 13];

/// The query cache's index for `cat` (`None`: `LC_ALL`).
fn query_index(cat: Option<Cat>) -> usize {
    cat.map_or(12, Cat::index)
}

/// The cached answer to `setlocale(cat, NULL)` for the current global locale, without the lock.
fn query_cached(objs: &impl Objects, cat: Option<Cat>) -> Option<&'static [u8]> {
    let seq = QUERY_SEQ.load(Acquire);
    if seq & 1 != 0 {
        return None;
    }
    let handle = QUERY_HANDLE.load(Acquire);
    let slot = load(&QUERY_SLOT, query_index(cat)).checked_sub(1)?;
    fence(Acquire);
    if QUERY_SEQ.load(Relaxed) != seq || handle != GLOBAL.load(Acquire) {
        return None;
    }
    Some(objs.bytes_at(field(|s| &s.name_addr, slot), field(|s| &s.name_len, slot)))
}

/// Record `slot` (an interned name's) as the answer for `cat` in the global locale `handle`.
/// The caller holds [`LOCK`].
fn query_record(handle: usize, cat: Option<Cat>, slot: usize) {
    if QUERY_HANDLE.load(Acquire) != handle {
        QUERY_SEQ.fetch_add(1, AcqRel);
        for s in &QUERY_SLOT {
            s.store(0, Release);
        }
        QUERY_HANDLE.store(handle, Release);
        QUERY_SEQ.fetch_add(1, AcqRel);
    }
    if let Some(s) = QUERY_SLOT.get(query_index(cat)) {
        s.store(slot.saturating_add(1), Release);
    }
}

/// `setlocale(cat, …)` (`cat` `None` is `LC_ALL`). Returns the category's name or the
/// composite name (`'static`, followed by a NUL), or `None` if a name is unknown, in which
/// case nothing changes. A query is answered from the query cache when it can be.
pub fn setlocale<S: Sys, M: Mapper>(sys: &S, m: &M, cat: Option<Cat>, req: Request<'_>) -> Option<&'static [u8]> {
    match req {
        Request::Query => {
            if let Some(s) = query_cached(m, cat) {
                return Some(s);
            }
        }
        // The name the query would answer (for `LC_ALL`: the single or composite name) names
        // every category asked as it is: nothing to look up (s7_2_c_4), no lock.
        Request::Name(n) => {
            if let Some(s) = query_cached(m, cat).filter(|&s| s == n) {
                return Some(s);
            }
        }
        Request::Environment(_) => {}
    }
    let _g = LOCK.lock(sys);
    let current = *global(m);
    let one = [cat.unwrap_or(Cat::Ctype)];
    let cats: &[Cat] = if cat.is_some() { &one } else { &Cat::ALL };
    let next = match req {
        Request::Query => current,
        Request::Name(n) => {
            let names = match (cat, name::parse_composite(n)) {
                (None, Some(all)) => all,
                _ => [n; 12],
            };
            replace_changed(m, &current, cats, &names)?
        }
        Request::Environment(getenv) => {
            // `env_name` for the categories asked, with `LC_ALL` and `LANG` each looked up once
            // (SETLOCALE-START: three environment scans per category before).
            let nonempty = |k: &[u8]| getenv(k).filter(|v| !v.is_empty());
            let all = nonempty(b"LC_ALL");
            let mut lang: Option<Option<&'static [u8]>> = None;
            let mut names: [&[u8]; 12] = [b"C"; 12];
            for &c in cats {
                let n = match all.or_else(|| nonempty(c.name())) {
                    Some(n) => n,
                    None => lang.get_or_insert_with(|| nonempty(b"LANG")).unwrap_or(b"C"),
                };
                *names.get_mut(c.index())? = n;
            }
            replace_changed(m, &current, cats, &names)?
        }
    };
    if next != current {
        GLOBAL.store(intern_locked(m, next).ok()?, Release);
    } else if let Some(s) = query_cached(m, cat) {
        return Some(s);
    }
    let (slot, s) = match cat {
        // Interned for its NUL: built-in names are plain literals.
        Some(c) => intern_name_locked(m, next.name(c)).ok()?,
        None => {
            let mut buf = [0u8; 12 * (name::MAX_NAME + 20)];
            let len = name::composite(next.names(), &mut buf)?;
            intern_name_locked(m, buf.get(..len)?).ok()?
        }
    };
    query_record(GLOBAL.load(Acquire), cat, slot);
    Some(s)
}

/// `liella_locale_invalidate` (D-X.13): advance the generation, then load the global locale
/// again by its names (the files are checked, and read again where they changed). `ENOENT` when
/// a name no longer resolves; the global locale is then unchanged.
pub fn invalidate_and_reload<S: Sys, M: Mapper>(sys: &S, m: &M) -> KResult<()> {
    let _g = LOCK.lock(sys);
    invalidate();
    let current = *global(m);
    let mut names: [&[u8]; 12] = [b"C"; 12];
    for c in Cat::ALL {
        *names.get_mut(c.index()).ok_or(Errno::EINVAL)? = current.name(c);
    }
    let next = replace(m, Locale::C, &Cat::ALL, &names).ok_or(Errno::ENOENT)?;
    if next != current {
        GLOBAL.store(intern_locked(m, next)?, Release);
    }
    Ok(())
}

/// `newlocale`: `base` with the categories `cats` taken from the locale `name` (a composite
/// name when `cats` is every category). The new locale's handle, or `ENOENT`.
pub fn newlocale<S: Sys, M: Mapper>(sys: &S, m: &M, cats: &[Cat], name: &[u8], base: Locale) -> KResult<usize> {
    let _g = LOCK.lock(sys);
    let names = match name::parse_composite(name) {
        Some(all) if cats.len() == Cat::ALL.len() => all,
        _ => [name; 12],
    };
    let l = replace(m, base, cats, &names).ok_or(Errno::ENOENT)?;
    intern_locked(m, l)
}
