//! `ld.so`'s `dlopen` (phase 14 DL-16a `DL-DLOPEN-LOAD`; plan/ph14/dl-16-dlopen.md §1, §3, §4;
//! D-27.34). The C library's `dlopen` reaches [`entry`] through `StartInfo::dlopen` with the name,
//! the mode and its caller's return address.
//!
//! * **State.** One [`Loader`] in a private anonymous mapping made at the first call (nothing at
//!   start: `link` only records a few words per object). It holds every object as the relocator
//!   sees it (the start-time objects read again from their memory images, as `link` read them),
//!   their lookup views, the link-map nodes (the public `struct link_map` prefix first), the global
//!   scope, and two bump arenas (paths, dependency lists). The chain only grows (DL-16 never
//!   unloads): a new set is linked in with one Release store.
//! * **Lock.** One futex lock, re-entrant for its owner (a constructor may call `dlopen`), held for
//!   the whole call. The `&mut Loader` borrow ends before any foreign code runs.
//! * **A new set.** The name is found (the caller's object as requester), every object of the set
//!   is opened, checked ([`liella_ld::dlopen::check`]), identified by (`st_dev`, `st_ino`) and
//!   mapped; versions are checked; pass 1 of the relocation resolves every reference of the whole
//!   set without running foreign code; only then pass 2 runs the IFUNC resolvers (ruling #5125), the
//!   RELRO pages are protected, the set is published, and the constructors run in
//!   `init::init_order`. Any failure before publication unmaps every new object and leaves the
//!   loader as it was, with one dlerror text set.
//!
//! Not here yet (plan §6): `dlsym`/`dlvsym` (DL-16b), the counters and `dl_iterate_phdr` over the
//! chain (DL-16c), objects with `PT_TLS` (DL-16d), unloading (DL-17), lazy binding (DL-20).

use super::{array_words, c_string, dyn_string, env_entries, image_view, open_whole, own_object, read_auxv, ProcImage, SysMapper};
use super::{AT_FDCWD, AUXV_WORDS_MAX, MAP_ANONYMOUS_I32, MAP_PRIVATE_I32, START_INTERP, START_N, START_OBJS, START_ARGS, START_PAGE};
use crate::{conv, dlerr, raw, tb};
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use core::sync::atomic::{AtomicU32, AtomicUsize};
use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::{O_CLOEXEC, O_RDONLY, SEEK_END};
use liella_ld::arch::{Arch, Native};
use liella_ld::cache::Cache;
use liella_ld::dlopen::{self as dl, Mode, Refusal};
use liella_ld::elf::{pt, Elf};
use liella_ld::reloc::{self, Obj, Objects, RelocError, VersionError};
use liella_ld::search::{self, Ctx, PathBuf, Scope};
use liella_ld::symbols::Object;
use liella_ld::{init, map, start};

/// Objects the process can hold, the start-time ones included; one more is refused with a text.
const CAP: usize = 512;
/// Objects one `dlopen` can add (the bound of `init::init_order`).
const NEW_MAX: usize = init::OBJECTS_MAX;
/// Bytes of the path arena.
const NAMES: usize = 128 * 1024;
/// Entries of the dependency-list arena.
const DEPS: usize = 16 * 1024;

const DT_SONAME: u64 = 14;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;
const S_IFMT: u32 = 0o170_000;
const S_IFDIR: u32 = 0o040_000;
const MAP_NORESERVE_I32: i32 = 0x4000;
const FUTEX_WAIT_PRIVATE: i64 = 128;
const FUTEX_WAKE_PRIVATE: i64 = 129;

/// One object's link map: `<link.h>`'s public `struct link_map` prefix, then Liella's own fields.
/// All-zero is a valid value (the loader's mapping starts zeroed).
#[repr(C)]
struct Node {
    /// `l_addr`: the load bias.
    l_addr: usize,
    /// `l_name`: the NUL-terminated name (`""` for the program).
    l_name: usize,
    /// `l_ld`: the run-time address of the dynamic section.
    l_ld: usize,
    /// `l_next`: the next node, published with Release.
    l_next: AtomicUsize,
    /// `l_prev`.
    l_prev: usize,
    /// The file's identity (start-time program: `u64::MAX`).
    dev: u64,
    ino: u64,
    /// The mapped range [`lo`, `hi`): which object a caller address is in.
    lo: usize,
    hi: usize,
    /// Direct `dlopen` calls holding it (saturating, DL-17 reads it).
    opens: u32,
    /// Never unloaded (initial, `RTLD_NODELETE`, `DF_1_NODELETE`, or `opens` saturated).
    nodelete: bool,
    /// In the global scope.
    global: bool,
    /// Its dependencies (indices), a span of the dependency arena.
    deps: (u32, u32),
    /// Its path (a span of the path arena, NUL after it): `$ORIGIN` and messages.
    path: (u32, u32),
    /// Its finalisation words: `DT_FINI_ARRAY` address and length, `DT_FINI`.
    fini: [u64; 3],
    /// A `dlopen`ed object's read-only file mapping (address, length), which its `Obj` reads.
    file: (usize, usize),
    /// A `dlopen`ed object's reservation (base, span): what DL-17 (and a failed attempt) unmaps.
    reserve: (u64, u64),
}

/// The loader's state (module documentation).
#[repr(C)]
struct Loader {
    /// Objects loaded (`objs[..n]`, `syms[..n]` initialised).
    n: usize,
    /// Entries of `global`.
    n_global: usize,
    names_top: usize,
    deps_top: usize,
    /// `dlopen`ed objects whose constructors started, in that order (destructors run backwards).
    n_log: AtomicUsize,
    log: [u16; CAP],
    global: [usize; CAP],
    nodes: [Node; CAP],
    objs: [MaybeUninit<Obj<'static>>; CAP],
    syms: [MaybeUninit<Object<'static>>; CAP],
    deps: [u16; DEPS],
    names: [u8; NAMES],
}

/// The loader's address (0: not made yet).
static LOADER: AtomicUsize = AtomicUsize::new(0);

/// The writer lock: 0 free, 1 held, 2 held with waiters (a three-state futex mutex).
static LOCK: AtomicU32 = AtomicU32::new(0);
/// The holder's thread pointer and its depth (written by the holder only).
static OWNER: AtomicUsize = AtomicUsize::new(0);
static DEPTH: AtomicUsize = AtomicUsize::new(0);

fn futex(op: i64, val: u32) {
    let addr = LOCK.as_ptr().cast::<c_void>();
    let _ = crate::ffi_sys::liella_sys_futex(addr, op, i64::from(val), core::ptr::null(), core::ptr::null_mut(), 0);
}

fn lock() {
    let me = tb::tp();
    if OWNER.load(Relaxed) == me {
        DEPTH.store(DEPTH.load(Relaxed).saturating_add(1), Relaxed);
        return;
    }
    if let Err(mut c) = LOCK.compare_exchange(0, 1, Acquire, Relaxed) {
        if c != 2 {
            c = LOCK.swap(2, Acquire);
        }
        while c != 0 {
            futex(FUTEX_WAIT_PRIVATE, 2);
            c = LOCK.swap(2, Acquire);
        }
    }
    OWNER.store(me, Relaxed);
    DEPTH.store(1, Relaxed);
}

fn unlock() {
    let d = DEPTH.load(Relaxed).saturating_sub(1);
    DEPTH.store(d, Relaxed);
    if d != 0 {
        return;
    }
    OWNER.store(0, Relaxed);
    if LOCK.swap(0, Release) == 2 {
        futex(FUTEX_WAKE_PRIVATE, 1);
    }
}

/// The address of [`entry`], for `StartInfo::dlopen`.
pub(super) fn entry_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (entry as *const ()).expose_provenance()
}

/// `dlopen(file, mode)` called from `caller` (a return address in the calling object): the handle,
/// or 0 with the calling thread's dlerror text set (none for an `RTLD_NOLOAD` miss).
extern "C" fn entry(file: usize, mode: i32, caller: usize) -> usize {
    lock();
    let (handle, ctors) = open_locked(file, mode, caller);
    // The loader borrow has ended: constructors may call back into the loader.
    ctors.run();
    unlock();
    handle
}

/// The loader, made (with the start-time objects) at the first call. The caller holds the lock
/// and drops the borrow before any foreign code runs, so no two `&mut Loader` are ever live.
fn loader() -> Option<&'static mut Loader> {
    let mut at = LOADER.load(Acquire);
    let fresh = at == 0;
    if fresh {
        let flags = MAP_PRIVATE_I32 | MAP_ANONYMOUS_I32 | MAP_NORESERVE_I32;
        at = raw::mmap(0, size_of::<Loader>(), map::PROT_READ | map::PROT_WRITE, flags, -1, 0).ok()?;
    }
    // UNSAFE(slice): the loader's state in its own mapping.
    #[allow(unsafe_code)]
    // SAFETY: `at` is a private read-write mapping of size_of::<Loader>() bytes, page-aligned, made
    // above or by an earlier call and never unmapped. A zeroed Loader is valid (integers, atomics,
    // bools, MaybeUninit). Only the lock holder calls this, and every caller drops the borrow before
    // foreign code (a constructor that may re-enter) runs, so this is the only live reference.
    let l = unsafe { &mut *core::ptr::with_exposed_provenance_mut::<Loader>(at) };
    if fresh {
        if !adopt_start(l) {
            let _ = raw::munmap(at, size_of::<Loader>());
            return None;
        }
        LOADER.store(at, Release);
    }
    Some(l)
}

impl Loader {
    fn node_addr(&self, i: usize) -> usize {
        self.nodes.get(i).map_or(0, |n| core::ptr::from_ref(n).expose_provenance())
    }

    /// Copy `s` and a NUL into the path arena: its span.
    fn keep(&mut self, s: &[u8]) -> Option<(u32, u32)> {
        let start = self.names_top;
        let end = start.checked_add(s.len())?;
        self.names.get_mut(start..end)?.copy_from_slice(s);
        *self.names.get_mut(end)? = 0;
        self.names_top = end.checked_add(1)?;
        Some((u32::try_from(start).ok()?, u32::try_from(s.len()).ok()?))
    }

    fn path(&self, i: usize) -> &[u8] {
        let Some(&Node { path: (a, b), .. }) = self.nodes.get(i) else { return b"" };
        let a = usize::try_from(a).unwrap_or(0);
        self.names.get(a..a.saturating_add(usize::try_from(b).unwrap_or(0))).unwrap_or(b"")
    }

    fn deps_of(&self, i: usize) -> &[u16] {
        let Some(&Node { deps: (a, b), .. }) = self.nodes.get(i) else { return &[] };
        let a = usize::try_from(a).unwrap_or(0);
        self.deps.get(a..a.saturating_add(usize::try_from(b).unwrap_or(0))).unwrap_or(&[])
    }

    /// The object whose `DT_SONAME` is `name`, among the first `k`.
    fn by_soname(&self, name: &[u8], k: usize) -> Option<usize> {
        let objs = self.objs_upto(k);
        objs.iter().position(|o| soname(o) == Some(name))
    }

    /// `objs[..k]` (`k` may pass `n` by the objects of the attempt in progress).
    fn objs_upto(&self, k: usize) -> &[Obj<'static>] {
        let k = k.min(CAP);
        // UNSAFE(slice): the initialised prefix of a MaybeUninit array.
        #[allow(unsafe_code)]
        // SAFETY: every caller passes a `k` whose slots are written (the loaded objects and the
        // attempt's new ones, written before `k` is advanced).
        unsafe {
            core::slice::from_raw_parts(self.objs.as_ptr().cast::<Obj<'static>>(), k)
        }
    }

    fn syms_upto(&self, k: usize) -> &[Object<'static>] {
        let k = k.min(CAP);
        // UNSAFE(slice): the initialised prefix of a MaybeUninit array.
        #[allow(unsafe_code)]
        // SAFETY: as `objs_upto`.
        unsafe {
            core::slice::from_raw_parts(self.syms.as_ptr().cast::<Object<'static>>(), k)
        }
    }
}

fn soname(o: &Obj<'static>) -> Option<&'static [u8]> {
    dyn_string(&o.dynm, DT_SONAME)
}

/// The mapped range of `o`: lowest and highest run-time address of its `PT_LOAD`s.
fn range(o: &Obj<'_>) -> (usize, usize) {
    let loads = || o.elf.program_headers().filter(|p| p.p_type == pt::LOAD);
    let lo = loads().map(|p| p.vaddr).min().unwrap_or(0);
    let hi = loads().map(|p| p.vaddr.saturating_add(p.memsz)).max().unwrap_or(0);
    let at = |v: u64| usize::try_from(o.bias.wrapping_add(v)).unwrap_or(0);
    (at(lo), at(hi))
}

/// Fill node `i` of `o`, not yet linked.
fn describe(l: &mut Loader, i: usize, o: &Obj<'static>, name: usize, id: (u64, u64)) {
    let (lo, hi) = range(o);
    let dynv = o.elf.find(pt::DYNAMIC).map_or(0, |p| p.vaddr);
    let prev = if i == 0 { 0 } else { l.node_addr(i.wrapping_sub(1)) };
    if let Some(node) = l.nodes.get_mut(i) {
        node.l_addr = usize::try_from(o.bias).unwrap_or(0);
        node.l_name = name;
        node.l_ld = usize::try_from(o.bias.wrapping_add(dynv)).unwrap_or(0);
        node.l_next.store(0, Relaxed);
        node.l_prev = prev;
        (node.dev, node.ino) = id;
        (node.lo, node.hi) = (lo, hi);
    }
}

/// Read the start-time objects again from their memory images ([`image_view`], as `link` did) and
/// make them the first nodes, all global, never unloaded. `ld.so` itself joins (not global) when
/// no object named it at start. False when `link` did not run (no records).
// Indices stay below CAP (checked) and OBJECTS_MAX (START_N's bound).
#[allow(clippy::indexing_slicing)]
fn adopt_start(l: &mut Loader) -> bool {
    let n = START_N.load(Acquire);
    if n == 0 || n >= CAP {
        return false;
    }
    let mut interp = START_INTERP.load(Relaxed);
    let total = if interp < n { n } else { n.saturating_add(1) };
    for k in 0..total {
        // The views are kept for the process's lifetime: they are the loader's copies.
        let mut views = [(0usize, 0usize); 3];
        let w = START_OBJS.get(k).map_or([0; 5], |s| [0, 1, 2, 3, 4].map(|j| s[j].load(Relaxed)));
        let o = if k == interp || k == n {
            interp = k;
            own_object(Native::INTERP.as_bytes(), &mut views)
        } else {
            let at = |v: u64| usize::try_from(v).unwrap_or(0);
            let prog = start::Program { phdr: at(w[0]), phnum: at(w[1]), entry: 0 };
            let (head, view) = image_view(&prog, at(w[2]), b"", &mut views);
            let Ok(elf) = Elf::from_image::<Native>(head, view) else { return false };
            let Ok(Some(dynm)) = elf.dynamic() else { return false };
            let mut o = Obj { elf, dynm, bias: w[2], name: b"", tls: None };
            o.name = if k == 0 { b"" } else { soname(&o).unwrap_or(b"") };
            o
        };
        let Ok(s) = Object::new(o.dynm) else { return false };
        l.objs[k] = MaybeUninit::new(o);
        l.syms[k] = MaybeUninit::new(s);
        // `l_name`: the program's is "", another object's its DT_SONAME (NUL-terminated in the
        // string table the view holds).
        let name = if k == 0 { c"".as_ptr().expose_provenance() } else { o.name.as_ptr().expose_provenance() };
        let id = if k == interp { (u64::MAX, u64::MAX) } else { (w[3], w[4]) };
        describe(l, k, &o, name, id);
        l.nodes[k].nodelete = true;
        // At start every object was in the global scope; an `ld.so` that joins now is not.
        if k < n {
            l.nodes[k].global = true;
            l.global[l.n_global] = k;
            l.n_global = l.n_global.saturating_add(1);
        }
        if k > 0 {
            let me = l.node_addr(k);
            l.nodes[k.wrapping_sub(1)].l_next.store(me, Relaxed);
        }
        l.n = k.saturating_add(1);
    }
    true
}

/// The start's `argc`, `argv`, `envp`, auxiliary vector (`i` = 0..4) as `run_init` recorded them.
fn start_arg(i: usize) -> usize {
    START_ARGS.get(i).map_or(0, |w| w.load(Relaxed))
}

/// The constructors of one call's new set, in order, run after the loader borrow ends.
struct Ctors {
    calls: [(u64, u64, u64); NEW_MAX],
    len: usize,
}

impl Ctors {
    const NONE: Ctors = Ctors { calls: [(0, 0, 0); NEW_MAX], len: 0 };

    /// Run each object's `DT_INIT` then `DT_INIT_ARRAY` with the initial `(argc, argv, envp)`.
    fn run(&self) {
        let [a, b, c] = [0, 1, 2].map(|i| i64::from_ne_bytes(u64::try_from(start_arg(i)).unwrap_or(0).to_ne_bytes()));
        for &(f, addr, len) in self.calls.get(..self.len).unwrap_or(&[]) {
            for f in init::init_calls(f, array_words(init::Array { addr, len })) {
                let _ = crate::ffi::liella_machine_call3(usize::try_from(f).unwrap_or(0), a, b, c);
            }
        }
    }
}

/// Run the destructors of the `dlopen`ed objects, the last initialised first (from `fini_all`).
pub(super) fn fini_loaded() {
    let at = LOADER.load(Acquire);
    if at == 0 {
        return;
    }
    // UNSAFE(slice): the loader's state, read at exit.
    #[allow(unsafe_code)]
    // SAFETY: `at` is the loader's live mapping (never unmapped); the log entries below `n_log` (read
    // with Acquire after their Release) and their nodes' finalisation words are written once,
    // before the entry is counted, and never again; only shared reads happen here.
    let l = unsafe { &*core::ptr::with_exposed_provenance::<Loader>(at) };
    let n = l.n_log.load(Acquire).min(CAP);
    for &i in l.log.get(..n).unwrap_or(&[]).iter().rev() {
        let Some(node) = l.nodes.get(usize::from(i)) else { continue };
        let [addr, len, fini] = node.fini;
        for f in init::fini_calls(fini, array_words(init::Array { addr, len })) {
            let _ = crate::ffi::liella_machine_call0(usize::try_from(f).unwrap_or(0));
        }
    }
}

/// A failed call whose dlerror text is set (always before the attempt is unwound: the text's parts
/// may lie in the attempt's mappings and arena spans).
struct Fail;

/// What the search for one name found.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Found {
    /// Not found (or, under `RTLD_NOLOAD`, not loaded).
    Missing,
    /// An object already in the loader (or earlier in this attempt).
    Loaded(usize),
    /// A new object, now at index `k` of the attempt.
    New(usize),
    /// A candidate refused: the reason and the path it was found at (a span of the path arena).
    Refused(Refusal, (u32, u32)),
    /// Out of room (objects, paths or dependency entries) or a mapping failure: Liella's own text.
    Full(&'static [u8]),
}

/// The search context of one call: the start's environment, auxiliary vector and the cache.
struct Env {
    secure: bool,
    library_path: Option<&'static [u8]>,
    origin_path: Option<&'static [u8]>,
    features: liella_rt::cpu::Features,
    cache: Option<(&'static [u8], Cache<'static>)>,
}

impl Env {
    fn read() -> Env {
        let (envp, auxv) = (start_arg(2), start_arg(3));
        let mut copy = [0u64; AUXV_WORDS_MAX];
        let words = if auxv == 0 { 0 } else { read_auxv(auxv, &mut copy).unwrap_or(0) };
        let aux = Auxv::from_words(copy.get(..words).unwrap_or(&[]));
        let secure = start::secure(&aux);
        let ld = start::ld_env(env_entries(envp));
        let features = Native::features(&aux, &crate::sys::CSys);
        let mut path = PathBuf::default();
        let file = if search::prefixed(b"etc/ld.so.cache", &mut path) { path.c_str().and_then(open_whole) } else { None };
        let cache = file.and_then(|(f, fd)| {
            let _ = crate::ffi_sys::liella_sys_close(fd);
            match Cache::parse(f) {
                Some(c) => Some((f, c)),
                None => {
                    let _ = raw::munmap(f.as_ptr().expose_provenance(), f.len());
                    None
                }
            }
        });
        Env { secure, library_path: ld.library_path, origin_path: ld.origin_path, features, cache }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            secure: self.secure,
            library_path: if self.secure { None } else { self.library_path },
            origin_path: if self.secure { None } else { self.origin_path },
            platform: Native::platform(&self.features),
            hwcaps: Native::hwcaps(&self.features),
            cache: self.cache.as_ref().map(|(_, c)| c),
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        if let Some((f, _)) = self.cache {
            let _ = raw::munmap(f.as_ptr().expose_provenance(), f.len());
        }
    }
}

/// The search scope of object `i`: its `DT_RPATH`, `DT_RUNPATH` and `$ORIGIN` (the directory of
/// the path it was opened by; unknown for a start-time object without one).
fn scope_of(l: &Loader, i: usize) -> Scope<'static> {
    let Some(o) = l.objs_upto(l.n.max(i.saturating_add(1))).get(i).copied() else { return Scope::default() };
    let path = l.path(i);
    let origin = path.iter().rposition(|&b| b == b'/').and_then(|p| path.get(..p.max(1)));
    // UNSAFE(slice): a span of the loader's path arena, which only grows (DL-16 never frees).
    #[allow(unsafe_code)]
    // SAFETY: the bytes stay in the loader's mapping for the process's lifetime and a written span
    // is never rewritten while it is in use: a failed attempt rewinds only past its own spans.
    let origin: Option<&'static [u8]> = origin.map(|s| unsafe { conv::bytes_at(s.as_ptr().expose_provenance(), s.len()) });
    Scope { rpath: dyn_string(&o.dynm, DT_RPATH), runpath: dyn_string(&o.dynm, DT_RUNPATH), origin }
}

/// Open the candidate `path` for the attempt whose next index is `k`; see [`Found`].
// The arena spans and indices are checked before use.
#[allow(clippy::indexing_slicing)]
fn candidate(l: &mut Loader, path: &[u8], given: &'static [u8], k: usize, searched: bool, noload: bool, page: u64) -> Option<Found> {
    let cpath = path.as_ptr().expose_provenance();
    let fd = crate::ffi_sys::liella_sys_openat(AT_FDCWD, core::ptr::with_exposed_provenance(cpath), i64::from(O_RDONLY | O_CLOEXEC), 0);
    let fd32 = i32::try_from(fd).ok().filter(|&f| f >= 0)?;
    let close = || {
        let _ = crate::ffi_sys::liella_sys_close(fd);
    };
    let shown = path.strip_suffix(b"\0").unwrap_or(path);
    let mut st = liella_kernel::uapi::Stat::default();
    if crate::ffi_sys::liella_sys_fstat(fd, core::ptr::from_mut(&mut st).cast::<c_void>()) != 0 {
        close();
        return None;
    }
    if let Some(j) = l.nodes[..k].iter().position(|nd| (nd.dev, nd.ino) == (st.dev, st.ino)) {
        close();
        return Some(Found::Loaded(j));
    }
    if noload {
        close();
        return Some(Found::Missing);
    }
    let refused = |l: &mut Loader, r: Refusal| Some(l.keep(shown).map_or(Found::Full(b"out of room for object names"), |s| Found::Refused(r, s)));
    if st.mode & S_IFMT == S_IFDIR {
        close();
        return refused(l, Refusal::Directory);
    }
    let size = crate::ffi_sys::liella_sys_lseek(fd, 0, i64::from(SEEK_END));
    let Some(len) = usize::try_from(size).ok().filter(|&s| s >= liella_ld::elf::EHDR_SIZE) else {
        close();
        return refused(l, Refusal::TooShort);
    };
    let Ok(at) = raw::mmap(0, len, map::PROT_READ, MAP_PRIVATE_I32, fd32, 0) else {
        close();
        return Some(Found::Full(b"cannot read file data"));
    };
    // UNSAFE(slice): the candidate's file, mapped read-only above.
    #[allow(unsafe_code)]
    // SAFETY: `len` bytes from `at` are a private read-only mapping of the file. It is unmapped on
    // every path that drops the object (refusal, failed attempt) after the last use of the slice,
    // and otherwise kept for the process's lifetime (the loaded object's `Obj` reads it).
    let file: &'static [u8] = unsafe { conv::bytes_at(at, len) };
    let drop_file = || {
        let _ = raw::munmap(at, len);
        close();
    };
    let elf = match dl::check::<Native>(file, page) {
        Ok(e) => e,
        Err(r) if r.skipped(searched) => {
            drop_file();
            return None;
        }
        Err(r) => {
            drop_file();
            return refused(l, r);
        }
    };
    let Ok(Some(dynm)) = elf.dynamic() else {
        drop_file();
        return refused(l, Refusal::NoDynamic);
    };
    // `ld.so` by any path is the start-time `ld.so` (ruling #5126).
    if dyn_string(&dynm, DT_SONAME) == Some(Native::INTERP.as_bytes()) {
        drop_file();
        return Some(l.by_soname(Native::INTERP.as_bytes(), k).map_or(Found::Missing, Found::Loaded));
    }
    if k >= CAP {
        drop_file();
        return Some(Found::Full(b"cannot load more objects"));
    }
    let loaded = map::load::<Native, _>(&mut SysMapper { fd: fd32 }, file, page);
    close();
    let loaded = match loaded {
        Ok(x) => x,
        Err(e) => {
            let _ = raw::munmap(at, len);
            return refused(l, dl::of_map_error(e));
        }
    };
    let Ok(sym) = Object::new(dynm) else {
        loaded.unmap(&mut SysMapper { fd: -1 });
        let _ = raw::munmap(at, len);
        return refused(l, Refusal::Malformed);
    };
    let Some(span) = l.keep(shown) else {
        loaded.unmap(&mut SysMapper { fd: -1 });
        let _ = raw::munmap(at, len);
        return Some(Found::Full(b"out of room for object names"));
    };
    let o = Obj { elf, dynm, bias: loaded.bias, name: given, tls: None };
    l.objs[k] = MaybeUninit::new(o);
    l.syms[k] = MaybeUninit::new(sym);
    let name = l.names.as_ptr().expose_provenance().wrapping_add(usize::try_from(span.0).unwrap_or(0));
    describe(l, k, &o, name, (st.dev, st.ino));
    let node = &mut l.nodes[k];
    node.path = span;
    node.deps = (0, 0);
    node.opens = 0;
    node.global = false;
    node.nodelete = dynm.get(dl::DT_FLAGS_1).is_some_and(|f| f & dl::DF_1_NODELETE != 0);
    node.fini = [0; 3];
    node.file = (at, len);
    node.reserve = (loaded.base, loaded.span);
    Some(Found::New(k))
}

/// Find `name` (needed by object `by`, or by the caller for the root) for the attempt whose next
/// index is `k`: a bare name first matches a loaded object's `DT_SONAME` (P-2), then the search runs
/// with `chain` as requesters.
fn find(l: &mut Loader, name: &'static [u8], chain: &[Scope<'_>], env: &Env, k: usize, noload: bool, page: u64) -> Found {
    let bare = dl::bare(name);
    if bare {
        if let Some(j) = l.by_soname(name, k) {
            return Found::Loaded(j);
        }
    }
    let ctx = env.ctx();
    let mut out = Found::Missing;
    search::search::<Native>(name, chain, &ctx, &mut |p: &mut PathBuf| {
        let Some(c) = p.c_str() else { return false };
        match candidate(l, c, name, k, bare, noload, page) {
            Some(f) => {
                out = f;
                true
            }
            None => false,
        }
    });
    out
}

/// One `dlopen` under the lock: the handle (0 on failure) and the constructors to run.
fn open_locked(file: usize, mode: i32, caller: usize) -> (usize, Ctors) {
    let given: &'static [u8] = if file == 0 { b"" } else { c_string(file) };
    let m = match dl::mode(mode) {
        Ok(m) => m,
        Err(e) => {
            if e.prefixed() {
                dlerr::set(&[given, b": ", e.text()]);
            } else {
                dlerr::set(&[e.text()]);
            }
            return (0, Ctors::NONE);
        }
    };
    let Some(l) = loader() else {
        dlerr::set(&[given, b": cannot set up the dynamic loader's state"]);
        return (0, Ctors::NONE);
    };
    if given.is_empty() {
        reopen(l, 0, m);
        dlerr::clear();
        return (l.node_addr(0), Ctors::NONE);
    }
    match load_set(l, given, m, caller) {
        // Also an `RTLD_NOLOAD` miss (handle 0): it clears the pending text (tools/probe/dl/gaps).
        Ok((h, ctors)) => {
            dlerr::clear();
            (h, ctors)
        }
        Err(Fail) => (0, Ctors::NONE),
    }
}

/// One more `dlopen` of loaded object `i`: count, `RTLD_NODELETE`, `RTLD_GLOBAL` promotion.
fn reopen(l: &mut Loader, i: usize, m: Mode) {
    let promote = m.global && l.nodes.get(i).is_some_and(|n| !n.global);
    if let Some(node) = l.nodes.get_mut(i) {
        let (opens, pin) = dl::reopen(node.opens);
        node.opens = opens;
        node.nodelete |= pin || m.nodelete;
    }
    if promote {
        let mut local = [0u16; CAP];
        let len = local_scope(l, i, &mut local);
        make_global(l, local.get(..len).unwrap_or(&[]));
    }
}

/// The breadth-first local scope of `root` (itself first) into `out`: its length.
// `len` stays at most CAP: every index is below CAP and appears once.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn local_scope(l: &Loader, root: usize, out: &mut [u16; CAP]) -> usize {
    let Ok(r) = u16::try_from(root) else { return 0 };
    out[0] = r;
    let mut len = 1usize;
    let mut q = 0usize;
    while q < len {
        let deps = l.deps_of(usize::from(out[q]));
        for &d in deps {
            if len < CAP && !out[..len].contains(&d) {
                out[len] = d;
                len += 1;
            }
        }
        q += 1;
    }
    len
}

/// Append `set`'s objects that are not global yet to the global scope (`RTLD_GLOBAL`).
fn make_global(l: &mut Loader, set: &[u16]) {
    for &i in set {
        let i = usize::from(i);
        let Some(node) = l.nodes.get_mut(i) else { continue };
        if node.global {
            continue;
        }
        node.global = true;
        if let Some(slot) = l.global.get_mut(l.n_global) {
            *slot = i;
            l.n_global = l.n_global.saturating_add(1);
        }
    }
}

/// Unmap the objects `base..k` of a failed attempt and rewind the arenas.
fn unwind(l: &mut Loader, base: usize, k: usize, names_top: usize, deps_top: usize) {
    for node in l.nodes.get(base..k).unwrap_or(&[]) {
        map::Mapper::unmap(&mut SysMapper { fd: -1 }, node.reserve.0, node.reserve.1);
        let _ = raw::munmap(node.file.0, node.file.1);
    }
    l.names_top = names_top;
    l.deps_top = deps_top;
}

/// The text of a failed attempt that found `f` for `name`.
fn found_text<'a>(l: &'a Loader, name: &'a [u8], f: Found, out: &'a mut [&'a [u8]; 3]) -> &'a [&'a [u8]] {
    match f {
        Found::Missing => *out = [name, b": ", dl::FILE_MISSING],
        Found::Refused(r, (a, b)) => {
            let a = usize::try_from(a).unwrap_or(0);
            let p = l.names.get(a..a.saturating_add(usize::try_from(b).unwrap_or(0))).unwrap_or(name);
            *out = [p, b": ", r.text()];
        }
        Found::Full(t) => *out = [name, b": ", t],
        Found::Loaded(_) | Found::New(_) => return &[],
    }
    &out[..]
}

/// Load `given` and the objects it needs as one new set, or re-open it (module documentation).
// Indices below CAP and NEW_MAX are checked before each store.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::too_many_lines)]
fn load_set(l: &mut Loader, given: &'static [u8], m: Mode, caller: usize) -> Result<(usize, Ctors), Fail> {
    let page = START_PAGE.load(Relaxed);
    let base = l.n;
    let (names_top, deps_top) = (l.names_top, l.deps_top);
    let who = (0..base).find(|&i| (l.nodes[i].lo..l.nodes[i].hi).contains(&caller)).unwrap_or(0);
    let env = Env::read();
    let chain = [scope_of(l, who), scope_of(l, 0)];
    let chain = if who == 0 { &chain[..1] } else { &chain[..] };
    let root = find(l, given, chain, &env, base, m.noload, page);
    let mut k = base;
    match root {
        Found::Loaded(i) => {
            reopen(l, i, m);
            return Ok((l.node_addr(i), Ctors::NONE));
        }
        Found::Missing if m.noload => return Ok((0, Ctors::NONE)),
        Found::New(_) => k += 1,
        f => {
            let t = fail_text(l, given, f);
            unwind(l, base, base, names_top, deps_top);
            return Err(t);
        }
    }
    // Breadth-first over the new objects' DT_NEEDED (each the requester of its own).
    let mut edges = [(0u8, 0u8); 1024];
    let mut ne = 0usize;
    let mut q = base;
    while q < k {
        let o = l.objs_upto(k)[q];
        let dep_start = l.deps_top;
        for needed in o.dynm.needed() {
            let Ok(needed) = needed else {
                let t = fail_parts(&[l_path_static(l, q), b": ", Refusal::Malformed.text()]);
                unwind(l, base, k, names_top, deps_top);
                return Err(t);
            };
            let chain = [scope_of(l, q), scope_of(l, 0)];
            let f = if needed == Native::INTERP.as_bytes() {
                l.by_soname(needed, k).map_or_else(|| find(l, needed, &chain, &env, k, false, page), Found::Loaded)
            } else {
                find(l, needed, &chain, &env, k, false, page)
            };
            let j = match f {
                Found::Loaded(j) => j,
                Found::New(j) => {
                    k += 1;
                    j
                }
                Found::Missing => {
                    let t = fail_parts(&[needed, b": ", dl::FILE_MISSING]);
                    unwind(l, base, k, names_top, deps_top);
                    return Err(t);
                }
                f => {
                    let t = fail_text(l, needed, f);
                    unwind(l, base, k, names_top, deps_top);
                    return Err(t);
                }
            };
            if k - base > NEW_MAX || l.deps_top >= DEPS || ne == edges.len() {
                let t = fail_parts(&[given, b": cannot load more objects in one call"]);
                unwind(l, base, k, names_top, deps_top);
                return Err(t);
            }
            l.deps[l.deps_top] = u16::try_from(j).unwrap_or(0);
            l.deps_top += 1;
            if j >= base {
                edges[ne] = (u8::try_from(q - base).unwrap_or(0), u8::try_from(j - base).unwrap_or(0));
                ne += 1;
            }
        }
        l.nodes[q].deps = (u32::try_from(dep_start).unwrap_or(0), u32::try_from(l.deps_top - dep_start).unwrap_or(0));
        q += 1;
    }
    // Versions, then pass 1 over the whole set: no foreign code yet.
    if let Err(e) = reloc::check_versions(l.objs_upto(k)) {
        let t = version_text(l, k, e);
        unwind(l, base, k, names_top, deps_top);
        return Err(t);
    }
    let mut local = [0u16; CAP];
    let llen = local_scope(l, base, &mut local);
    let mut scope = [0usize; CAP];
    let mut sn = l.n_global;
    scope[..sn].copy_from_slice(&l.global[..sn]);
    for &i in &local[..llen] {
        let i = usize::from(i);
        if !l.nodes[i].global && sn < CAP {
            scope[sn] = i;
            sn += 1;
        }
    }
    let (objs, syms) = (l.objs_upto(k), l.syms_upto(k));
    let set = Objects { objs, syms, global: &scope[..sn] };
    let mut unique = reloc::UniqueTable::<64, 4096>::default();
    let mut deferred = [false; NEW_MAX];
    for i in (base..k).rev() {
        match reloc::relocate_first::<Native, _>(&set, i, &mut ProcImage, page, &mut unique) {
            Ok(d) => deferred[i - base] = d,
            Err(e) => {
                let t = reloc_text(l, i, e);
                unwind(l, base, k, names_top, deps_top);
                return Err(t);
            }
        }
    }
    // Pass 2: the IFUNC resolvers, only now (ruling #5125).
    for i in (base..k).rev().filter(|&i| deferred[i - base]) {
        if let Err(e) = reloc::relocate_deferred::<Native, _>(&set, i, &mut ProcImage, &mut unique) {
            let t = reloc_text(l, i, e);
            unwind(l, base, k, names_top, deps_top);
            return Err(t);
        }
    }
    if let Err(i) = reloc::protect_relro(&objs[base..k], &mut ProcImage, page) {
        let t = fail_parts(&[l_path_static(l, base + i), b": cannot apply additional memory protection after relocation"]);
        unwind(l, base, k, names_top, deps_top);
        return Err(t);
    }
    // Publication: one Release store links the whole new set (plan §4).
    for i in base..k {
        let next = if i + 1 < k { l.node_addr(i + 1) } else { 0 };
        l.nodes[i].l_next.store(next, Relaxed);
    }
    l.n = k;
    let first = l.node_addr(base);
    l.nodes[base - 1].l_next.store(first, Release);
    l.nodes[base].opens = 1;
    l.nodes[base].nodelete |= m.nodelete;
    if m.global {
        make_global(l, &local[..llen]);
    }
    // Constructors of the new set, dependencies first (D-27.19).
    let mut order = [0u8; init::OBJECTS_MAX];
    let len = init::init_order(k - base, &edges[..ne], &mut order).unwrap_or(0);
    let mut ctors = Ctors::NONE;
    for &j in &order[..len] {
        let i = base + usize::from(j);
        let o = l.objs_upto(k)[i];
        let h = init::hooks(&o.dynm, o.bias, false);
        l.nodes[i].fini = [h.fini_array.addr, h.fini_array.len, h.fini];
        ctors.calls[ctors.len] = (h.init, h.init_array.addr, h.init_array.len);
        ctors.len += 1;
        let n = l.n_log.load(Relaxed);
        if n < CAP {
            l.log[n] = u16::try_from(i).unwrap_or(0);
            l.n_log.store(n + 1, Release);
        }
    }
    Ok((l.node_addr(base), ctors))
}

/// Set the calling thread's dlerror text to `parts` (copied at once into its record).
fn fail_parts(parts: &[&[u8]]) -> Fail {
    dlerr::set(parts);
    Fail
}

/// The path of object `i` (its name when it has none) as a `'static` slice.
fn l_path_static(l: &Loader, i: usize) -> &'static [u8] {
    let p = l.path(i);
    let p = if p.is_empty() { l.objs_upto(l.n.max(i.saturating_add(1))).get(i).map_or(&b"?"[..], |o| o.name) } else { p };
    // UNSAFE(slice): bytes in the loader's mapping or an object's image, both kept for the process.
    #[allow(unsafe_code)]
    // SAFETY: the slice is copied into the dlerror record (`fail_parts`) before any arena rewind.
    unsafe {
        conv::bytes_at(p.as_ptr().expose_provenance(), p.len())
    }
}

fn fail_text(l: &Loader, name: &[u8], f: Found) -> Fail {
    let mut out: [&[u8]; 3] = [b"", b"", b""];
    let parts = found_text(l, name, f, &mut out);
    fail_parts(parts)
}

fn version_text(l: &Loader, k: usize, e: VersionError<'_>) -> Fail {
    fn by_file<'a>(l: &'a Loader, k: usize, file: &'a [u8]) -> &'a [u8] {
        match l.objs_upto(k).iter().position(|o| soname(o) == Some(file) || o.name == file) {
            Some(j) if !l.path(j).is_empty() => l.path(j),
            _ => file,
        }
    }
    match e {
        VersionError::NoVersions { by, file, version } | VersionError::Missing { by, file, version } => {
            fail_parts(&[by_file(l, k, file), b": version `", version, b"' not found (required by ", l_path_static(l, by), b")"])
        }
        VersionError::Elf(by, _) => fail_parts(&[l_path_static(l, by), b": ", Refusal::Malformed.text()]),
    }
}

fn reloc_text(l: &Loader, i: usize, e: RelocError<'_>) -> Fail {
    let who = l_path_static(l, i);
    match e {
        RelocError::Undefined(sym) => fail_parts(&[who, b": undefined symbol: ", sym]),
        e => {
            let msg = e.message();
            fail_parts(&[who, b": ", msg.strip_suffix(b"\n").unwrap_or(msg)])
        }
    }
}
