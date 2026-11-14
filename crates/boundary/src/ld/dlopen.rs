//! `ld.so`'s `dlopen` (phase 14 DL-16a `DL-DLOPEN-LOAD`; plan/ph14/dl-16-dlopen.md §1, §3, §4;
//! D-27.34). The C library's `dlopen` reaches [`entry`] through `StartInfo::dlopen` with the name,
//! the mode and its caller's return address.
//!
//! * **State.** One [`Loader`] in a private anonymous mapping made at the first call (nothing at
//!   start: `link` only records a few words per object). It holds every object as the relocator
//!   sees it (the start-time objects read again from their memory images, as `link` read them),
//!   their lookup views, the link-map nodes (the public `struct link_map` prefix first), the global
//!   scope, and two bump arenas (paths, dependency lists). A new set is linked in with one Release
//!   store; `dlclose` ([`close_entry`], DL-17) unlinks what it unloads.
//! * **Locks.** The writer lock [`CALL`], re-entrant for its owner (a constructor may call
//!   `dlopen`), held for the whole call; inside it [`STATE`] while the call changes the loader's
//!   state, which `fork` waits for (DL-16c). The `&mut Loader` borrow ends before any constructor
//!   runs. Readers take neither (`super::readers`); the lock order is written at [`CALL`].
//! * **A new set.** The name is found (the caller's object as requester), every object of the set
//!   is opened, checked ([`liella_ld::dlopen::check`]), identified by (`st_dev`, `st_ino`) and
//!   mapped; versions are checked; pass 1 of the relocation resolves every reference of the whole
//!   set without running foreign code; only then pass 2 runs the IFUNC resolvers (ruling #5125), the
//!   RELRO pages are protected, the set is published, and the constructors run in
//!   `init::init_order`. Any failure before publication unmaps every new object and leaves the
//!   loader as it was, with one dlerror text set.
//!
//! * **Unloading** (DL-17, design in handoff/DL-DLCLOSE.md; probe `== close`). `dlclose` drops one
//!   open; an object stays while it is initial, `nodelete`, open, or reachable from such an object
//!   through its dependencies and its relocations' bindings ([`live_set`]). The others run their
//!   finalisers (the reverse of the constructor log; `DT_FINI_ARRAY` backwards, then `DT_FINI`)
//!   holding `CALL` only, still published; then they are unpublished under `STATE` and unmapped once
//!   every reader that may have seen them has left ([`reclaim`]); a slot is reused after that.
//!
//! Not here yet (plan §6): lazy binding (DL-20).

use super::{array_words, c_string, dyn_string, env_entries, image_view, open_whole, own_object, read_auxv, ProcImage, SysMapper};
use super::{AT_FDCWD, AUXV_WORDS_MAX, INITIAL_TAIL, MAP_ANONYMOUS_I32, MAP_PRIVATE_I32, START_INTERP, START_N, START_OBJS, START_ARGS, START_PAGE};
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
use liella_ld::elf::{pt, Elf, STB_GNU_UNIQUE};
use liella_ld::reloc::{self, Obj, Objects, RelocError, VersionError};
use liella_ld::search::{self, Ctx, PathBuf, Scope};
use liella_ld::symbols::{self, Found as SymFound, LookupError, Object, Request, UniqueRegistry, Want};
use liella_ld::tls::{self as ltls, Module, Placed};
use liella_ld::{init, map, start};

/// Objects the process can hold, the start-time ones included; one more is refused with a text.
pub(super) const CAP: usize = 512;
/// Objects one `dlopen` can add (the bound of `init::init_order`).
const NEW_MAX: usize = init::OBJECTS_MAX;
/// Bytes of the path arena.
const NAMES: usize = 128 * 1024;
/// Entries of the dependency-list arena.
const DEPS: usize = 16 * 1024;
/// The unique-symbol table: (name, version) keys and the bytes of their strings. A full table makes
/// the binding that needed one more entry fail with a text.
type Unique = reloc::UniqueTable<1024, { 64 * 1024 }>;
/// `SHN_ABS` (ELF gABI): an absolute symbol, not moved by the load bias.
const SHN_ABS: u16 = 0xfff1;

/// `objs[..k]` of the loader's object array (a field borrow, so another field may be borrowed
/// mutably beside it).
fn objs_of<'l>(objs: &'l [MaybeUninit<Obj<'static>>; CAP], k: usize) -> &'l [Obj<'static>] {
    let k = k.min(CAP);
    // UNSAFE(slice): the initialised prefix of a MaybeUninit array.
    #[allow(unsafe_code)]
    // SAFETY: every caller passes a `k` whose slots are written (the loaded objects and the
    // attempt's new ones, written before `k` is advanced).
    unsafe {
        core::slice::from_raw_parts(objs.as_ptr().cast::<Obj<'static>>(), k)
    }
}

/// `syms[..k]`, as [`objs_of`].
fn syms_of<'l>(syms: &'l [MaybeUninit<Object<'static>>; CAP], k: usize) -> &'l [Object<'static>] {
    let k = k.min(CAP);
    // UNSAFE(slice): the initialised prefix of a MaybeUninit array.
    #[allow(unsafe_code)]
    // SAFETY: as `objs_of`.
    unsafe {
        core::slice::from_raw_parts(syms.as_ptr().cast::<Object<'static>>(), k)
    }
}

/// The unique table as one lookup sees it: an entry recorded by this loader, or else the start's
/// binding, which is the first unique definition in the start's global scope (the start bound every
/// reference in that order); it is not recorded, so nothing is copied at the first `dl*` call.
struct Uniq<'a> {
    table: &'a mut Unique,
    syms: &'a [Object<'static>],
    start: &'a [usize],
    /// A `dlopen`'s record of its relocations' bindings (DL-17); `None` for a lookup (`dlsym`).
    binds: Option<&'a mut Binds>,
}

/// The bindings one `dlopen`'s relocations made (DL-17): per new object (index - `base`), a bit
/// per object it bound to.
struct Binds {
    base: usize,
    bits: [[u64; CAP / 64]; NEW_MAX],
}

impl Binds {
    fn has(&self, by: usize, obj: usize) -> bool {
        let Some(row) = by.checked_sub(self.base).and_then(|r| self.bits.get(r)) else { return false };
        row.get(obj / 64).is_some_and(|w| w & (1u64 << (obj % 64)) != 0)
    }
}

/// A registry that records nothing (the start's scope has no unique table of its own here).
struct NoUnique;

impl UniqueRegistry for NoUnique {
    fn find(&self, _: &[u8], _: &[u8]) -> Option<SymFound> {
        None
    }
    fn record(&mut self, _: &[u8], _: &[u8], _: SymFound) -> Result<(), LookupError> {
        Ok(())
    }
}

impl UniqueRegistry for Uniq<'_> {
    fn find(&self, name: &[u8], version: &[u8]) -> Option<SymFound> {
        self.table.find(name, version).or_else(|| {
            let want = if version.is_empty() { Want::Default } else { Want::Version(version) };
            let f = symbols::lookup(self.syms, symbols::Scope::global(self.start), &Request::new(name, want), &mut NoUnique).ok()??;
            (f.def.st.bind() == STB_GNU_UNIQUE).then_some(f)
        })
    }
    fn record(&mut self, name: &[u8], version: &[u8], found: SymFound) -> Result<(), LookupError> {
        self.table.record(name, version, found)
    }
    fn bound(&mut self, by: usize, found: &SymFound) {
        let Some(b) = self.binds.as_deref_mut() else { return };
        let Some(row) = by.checked_sub(b.base).and_then(|r| b.bits.get_mut(r)) else { return };
        if found.obj != by {
            if let Some(w) = row.get_mut(found.obj / 64) {
                *w |= 1u64 << (found.obj % 64);
            }
        }
    }
}

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
    /// A `dlopen`ed object: the root of the call that loaded it, whose local scope it was relocated
    /// with (`RTLD_NEXT` and `RTLD_DEFAULT` from it search that scope, probe P-5); itself once that
    /// root is unloaded.
    root: u16,
    /// The objects its relocations bound to outside itself (indices), a span of the dependency
    /// arena: they stay loaded while it does (DL-17, probe `== close` C-3).
    binds: (u32, u32),
    /// `dlclose` (or the exit) is running its finalisers (DL-17).
    finalising: bool,
    /// Unloaded (DL-17): out of the chain, the scopes and the readers' table; its memory is
    /// unmapped and the slot reusable once `freed`.
    gone: bool,
    freed: bool,
}

/// The loader's state (module documentation).
#[repr(C)]
struct Loader {
    /// Objects loaded (`objs[..n]`, `syms[..n]` initialised).
    n: usize,
    /// Entries of `global`.
    n_global: usize,
    /// The start-time objects (`ld.so` included when it joined at adoption), and how many of them
    /// are global: the start's own global scope, `global[..n_start_global]`.
    n_start: usize,
    n_start_global: usize,
    /// The namespace's `STB_GNU_UNIQUE` table (plan §2): the first definition bound for a (name,
    /// version) is every later binding's; `unique_mark` is its length when a `dlopen` began.
    unique: Unique,
    unique_mark: usize,
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
    /// The static surplus's places for `dlopen`ed modules (DL-16d) and their count when the
    /// current `load_set` began ([`unwind`] takes back the later ones).
    surplus: ltls::Surplus<{ ltls::MODULES_MAX }>,
    tls_mark: usize,
    /// `__liella_thread_walk`'s address once looked up (0: not yet).
    thread_walk: usize,
    /// The TLS module IDs of `dlopen`ed modules (DL-17: an unloaded module's ID is reused).
    ids: ltls::ModuleIds,
    /// Unloaded objects still mapped: (index, whether the reader parity flipped since it was
    /// unpublished; [`reclaim`]).
    n_retired: usize,
    retired: [(u16, bool); CAP],
}

/// The loader's address (0: not made yet).
static LOADER: AtomicUsize = AtomicUsize::new(0);

/// A futex mutex re-entrant for its owner: `word` 0 free, 1 held, 2 held with waiters; the
/// holder's thread pointer and depth are written by the holder only.
struct Reentrant {
    word: AtomicU32,
    owner: AtomicUsize,
    depth: AtomicUsize,
}

impl Reentrant {
    const fn new() -> Reentrant {
        Reentrant { word: AtomicU32::new(0), owner: AtomicUsize::new(0), depth: AtomicUsize::new(0) }
    }

    fn futex(&self, op: i64, val: u32) {
        let addr = self.word.as_ptr().cast::<c_void>();
        let _ = crate::ffi_sys::liella_sys_futex(addr, op, i64::from(val), core::ptr::null(), core::ptr::null_mut(), 0);
    }

    fn lock(&self) {
        let me = tb::tp();
        if self.owner.load(Relaxed) == me {
            self.depth.store(self.depth.load(Relaxed).saturating_add(1), Relaxed);
            return;
        }
        if let Err(mut c) = self.word.compare_exchange(0, 1, Acquire, Relaxed) {
            if c != 2 {
                c = self.word.swap(2, Acquire);
            }
            while c != 0 {
                self.futex(FUTEX_WAIT_PRIVATE, 2);
                c = self.word.swap(2, Acquire);
            }
        }
        self.owner.store(me, Relaxed);
        self.depth.store(1, Relaxed);
    }

    fn unlock(&self) {
        let d = self.depth.load(Relaxed).saturating_sub(1);
        self.depth.store(d, Relaxed);
        if d != 0 {
            return;
        }
        self.owner.store(0, Relaxed);
        if self.word.swap(0, Release) == 2 {
            self.futex(FUTEX_WAKE_PRIVATE, 1);
        }
    }

    /// Whether the calling thread holds the lock (at any depth).
    fn held_by_caller(&self) -> bool {
        self.owner.load(Relaxed) == tb::tp()
    }

    /// In a `fork` child, whose only thread is the caller: free the lock when another thread (one
    /// that does not exist here) held it; the caller's own hold stays.
    fn reset_in_child(&self) {
        if self.owner.load(Relaxed) != tb::tp() {
            self.owner.store(0, Relaxed);
            self.depth.store(0, Relaxed);
            self.word.store(0, Relaxed);
        }
    }
}

// Lock order (plan/ph14/dl-16-dlopen.md §4, one rule for every story): the loader's writer lock
// [`CALL`] -> its state lock [`STATE`] -> DL-15's thread-list lock (`thread::list_lock`) -> the
// loader arena's lock (DL-19's dynamic blocks; the dlerror records take none) -> the C library's
// internal locks (heap, stdio, ...). A constructor runs holding `CALL` only, so it may call
// `malloc`, stdio, `pthread_create` and the loader itself (both locks are re-entrant); `fork`'s
// prepare step takes `STATE` before every library lock (`crate::fork::lock_all`). Readers take no
// lock (`super::readers`). Checked by crates/ld/tests/dlopen_concurrent.rs (constructors that call
// into the library and the loader; `fork` during another thread's `dlopen`).

/// The writer lock: held for a whole `dlopen` call, constructors included, so `dlopen`s run one at
/// a time (probe P-8: a second thread's `dlopen` waits for the first's constructors).
static CALL: Reentrant = Reentrant::new();
/// The state lock: held (inside `CALL`) only while a call changes the loader's state, which runs
/// no foreign code but IFUNC resolvers. `fork` takes it, so a child never sees half a change, and
/// never waits for a constructor (probe P-9: `fork` returns while another thread's constructor
/// runs, and the child can `dlopen`).
static STATE: Reentrant = Reentrant::new();

pub(super) mod dlsym;

/// The address of [`entry`], for `StartInfo::dlopen`.
pub(super) fn entry_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (entry as *const ()).expose_provenance()
}

/// The address of [`fork_step`], for `StartInfo::dl_fork`.
pub(super) fn fork_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (fork_step as *const ()).expose_provenance()
}

/// `fork`'s loader step (`crate::fork::lock_all`/`unlock_all`, while other threads exist): 0 takes
/// [`STATE`] before the clone, 1 (parent) releases it, 2 (child) releases it and frees [`CALL`]
/// when a thread that is gone held it, and clears the gone threads' reader counts
/// (`readers::after_fork_child`). The child keeps every published object, those whose
/// constructors that thread was running included (probe P-9: listed, `RTLD_NOLOAD` finds them).
extern "C" fn fork_step(step: usize) -> usize {
    match step {
        0 => STATE.lock(),
        1 => STATE.unlock(),
        _ => {
            STATE.unlock();
            CALL.reset_in_child();
            super::readers::after_fork_child();
        }
    }
    0
}

/// `dlopen(file, mode)` called from `caller` (a return address in the calling object): the handle,
/// or 0 with the calling thread's dlerror text set (none for an `RTLD_NOLOAD` miss).
extern "C" fn entry(file: usize, mode: i32, caller: usize) -> usize {
    room_wait();
    CALL.lock();
    STATE.lock();
    let (handle, ctors) = open_locked(file, mode, caller);
    STATE.unlock();
    // The loader borrow has ended: constructors may call back into the loader.
    ctors.run();
    CALL.unlock();
    handle
}

/// The address of node `i` (its link map) once the loader's state exists, else 0; no reference to
/// the state is made (readers call this while a `dlopen` may hold `&mut Loader`).
pub(super) fn node_address(i: usize) -> usize {
    let at = LOADER.load(Acquire);
    if at == 0 || i >= CAP {
        return 0;
    }
    at.wrapping_add(core::mem::offset_of!(Loader, nodes)).wrapping_add(i.wrapping_mul(size_of::<Node>()))
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

    fn binds_of(&self, i: usize) -> &[u16] {
        let Some(&Node { binds: (a, b), .. }) = self.nodes.get(i) else { return &[] };
        let a = usize::try_from(a).unwrap_or(0);
        self.deps.get(a..a.saturating_add(usize::try_from(b).unwrap_or(0))).unwrap_or(&[])
    }

    /// Whether object `j` is unloaded (its memory may be gone: never read its `Obj`).
    fn gone(&self, j: usize) -> bool {
        self.nodes.get(j).is_none_or(|nd| nd.gone)
    }

    /// The index of the node at `addr`.
    fn index_of(&self, addr: usize) -> Option<usize> {
        let off = addr.checked_sub(self.node_addr(0))?;
        (off.checked_rem(size_of::<Node>())? == 0).then_some(off.checked_div(size_of::<Node>())?).filter(|&i| i < CAP)
    }

    /// The object whose `DT_SONAME` is `name`, among the first `k`.
    fn by_soname(&self, name: &[u8], k: usize) -> Option<usize> {
        let objs = self.objs_upto(k);
        objs.iter().enumerate().position(|(j, o)| self.nodes.get(j).is_some_and(|nd| !nd.gone) && soname(o) == Some(name))
    }

    /// `objs[..k]` (`k` may pass `n` by the objects of the attempt in progress).
    fn objs_upto(&self, k: usize) -> &[Obj<'static>] {
        objs_of(&self.objs, k)
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
            // Its static block, so a `dlopen`ed object's TLS relocation against one of its
            // symbols finds the place (DL-16d).
            let tls = elf.find(pt::TLS).and_then(|ph| super::initial_place(w[2].wrapping_add(ph.vaddr)));
            let mut o = Obj { elf, dynm, bias: w[2], name: b"", tls };
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
    l.n_start = total;
    l.n_start_global = l.n_global;
    adopt_tls(l, n, interp) && adopt_deps(l, total)
}

/// The start's static TLS placement of each start-time object, computed again from the same
/// segments in the same order as `link` placed them (`ltls::layout` is deterministic), so `dlsym` of
/// an `STT_TLS` symbol finds the calling thread's block (plan §2).
#[allow(clippy::indexing_slicing)] // n < CAP <= OBJECTS_MAX bounds (START_N's bound)
fn adopt_tls(l: &mut Loader, n: usize, interp: usize) -> bool {
    let mut mods: [Option<Module>; init::OBJECTS_MAX] = [None; init::OBJECTS_MAX];
    let mut placed: [Option<Placed>; init::OBJECTS_MAX] = [None; init::OBJECTS_MAX];
    let Some(objs) = l.objs_upto(n).get(..n) else { return false };
    if n > mods.len() {
        return false;
    }
    for (k, o) in objs.iter().enumerate() {
        if k != interp {
            mods[k] = o.elf.find(pt::TLS).map(|ph| Module::of(&ph));
        }
    }
    if ltls::layout::<Native>(&mods[..n], &mut placed[..n], INITIAL_TAIL, ltls::SURPLUS).is_err() {
        return false;
    }
    for (k, p) in placed[..n].iter().enumerate() {
        let o = Obj { tls: *p, ..l.objs_upto(n)[k] };
        l.objs[k] = MaybeUninit::new(o);
    }
    true
}

/// Each start-time object's dependencies (its `DT_NEEDED` entries in order), for its local scope (a
/// handle's `dlsym`, `RTLD_GLOBAL` promotion): a name matches a start-time object's `DT_SONAME`, a
/// name with a `/` the object at that path by (`st_dev`, `st_ino`). A name that matches neither
/// (a start-time object without `DT_SONAME` found by a bare name) is left out.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // deps_top < DEPS is checked
fn adopt_deps(l: &mut Loader, total: usize) -> bool {
    for q in 0..total {
        let o = l.objs_upto(total)[q];
        let start = l.deps_top;
        for needed in o.dynm.needed().map_while(Result::ok) {
            let j = l.by_soname(needed, total).or_else(|| {
                if !needed.contains(&b'/') {
                    return None;
                }
                let id = file_id(needed)?;
                l.nodes[..total].iter().position(|nd| (nd.dev, nd.ino) == id)
            });
            let Some(j) = j else { continue };
            if l.deps_top >= DEPS {
                return false;
            }
            l.deps[l.deps_top] = u16::try_from(j).unwrap_or(0);
            l.deps_top += 1;
        }
        l.nodes[q].deps = (u32::try_from(start).unwrap_or(0), u32::try_from(l.deps_top - start).unwrap_or(0));
    }
    true
}

/// The (`st_dev`, `st_ino`) of the file at `path` (no NUL in it), or `None`.
fn file_id(path: &[u8]) -> Option<(u64, u64)> {
    let mut buf = PathBuf::default();
    if !buf.push(path) {
        return None;
    }
    let c = buf.c_str()?;
    let fd = crate::ffi_sys::liella_sys_openat(AT_FDCWD, core::ptr::with_exposed_provenance(c.as_ptr().expose_provenance()), i64::from(O_RDONLY | O_CLOEXEC), 0);
    let _ = i32::try_from(fd).ok().filter(|&f| f >= 0)?;
    let mut st = liella_kernel::uapi::Stat::default();
    let ok = crate::ffi_sys::liella_sys_fstat(fd, core::ptr::from_mut(&mut st).cast::<c_void>()) == 0;
    let _ = crate::ffi_sys::liella_sys_close(fd);
    ok.then_some((st.dev, st.ino))
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

/// Run the destructors of the `dlopen`ed objects still loaded, the last initialised first (from
/// `fini_all`). Each object is marked finalising first, so a `dlclose` that a destructor or a later
/// exit step makes never runs them again.
pub(super) fn fini_loaded() {
    if LOADER.load(Acquire) == 0 {
        return;
    }
    let mut f = Finis::NONE;
    STATE.lock();
    if let Some(l) = loader() {
        let n = l.n_log.load(Relaxed).min(CAP);
        for k in (0..n).rev() {
            let i = l.log.get(k).map_or(CAP, |&i| usize::from(i));
            f.add(l, i);
        }
    }
    STATE.unlock();
    f.run();
}

/// The finalisers of the objects one `dlclose` (or the exit) unloads, in the order they run.
struct Finis {
    calls: [[u64; 3]; CAP],
    idx: [u16; CAP],
    len: usize,
}

impl Finis {
    const NONE: Finis = Finis { calls: [[0; 3]; CAP], idx: [0; CAP], len: 0 };

    /// Add object `i` (loaded, not finalising yet) and mark it finalising.
    fn add(&mut self, l: &mut Loader, i: usize) {
        let Some(node) = l.nodes.get_mut(i) else { return };
        if node.gone || node.finalising || i >= l.n {
            return;
        }
        let (Some(c), Some(x)) = (self.calls.get_mut(self.len), self.idx.get_mut(self.len)) else { return };
        node.finalising = true;
        *c = node.fini;
        *x = u16::try_from(i).unwrap_or(0);
        self.len = self.len.saturating_add(1);
    }

    fn set(&self) -> &[u16] {
        self.idx.get(..self.len).unwrap_or(&[])
    }

    /// Each object's `DT_FINI_ARRAY` in reverse, then its `DT_FINI` (probe `== order`).
    fn run(&self) {
        for &[addr, len, fini] in self.calls.get(..self.len).unwrap_or(&[]) {
            for f in init::fini_calls(fini, array_words(init::Array { addr, len })) {
                let _ = crate::ffi::liella_machine_call0(usize::try_from(f).unwrap_or(0));
            }
        }
    }
}

/// The address of [`close_entry`], for `StartInfo::dlclose`.
pub(super) fn close_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (close_entry as *const ()).expose_provenance()
}

/// `dlclose(handle)` (DL-17; module documentation): 0, or -1 with the text `dlclose: invalid
/// handle` for NULL, a pointer that is not a handle, or a handle with no open left (pending
/// D-27.32, ruling #5006). A success clears a pending text (probe `== close` C-2).
extern "C" fn close_entry(handle: usize) -> i64 {
    room_wait();
    CALL.lock();
    STATE.lock();
    let mut f = Finis::NONE;
    let r = close_begin(handle, &mut f);
    STATE.unlock();
    if r.is_ok() && f.len > 0 {
        // The loader borrow has ended: a destructor may call `dlopen` or `dlclose` (C-8).
        f.run();
        STATE.lock();
        close_end(&f);
        STATE.unlock();
    }
    CALL.unlock();
    match r {
        Ok(()) => {
            dlerr::clear();
            0
        }
        Err(Fail) => -1,
    }
}

/// Drop one open of `handle`; fill `f` with what is to be unloaded (marked finalising).
fn close_begin(handle: usize, f: &mut Finis) -> Result<(), Fail> {
    let invalid = || fail_parts(&[b"dlclose: invalid handle"]);
    let Some(l) = loader() else { return Err(invalid()) };
    reclaim(l);
    let Some(i) = dlsym::handle_index(l, handle) else { return Err(invalid()) };
    let start = i < l.n_start;
    let Some(node) = l.nodes.get_mut(i) else { return Err(invalid()) };
    if start {
        // The program and its start-time objects are never unloaded (C-9: 0).
        node.opens = node.opens.saturating_sub(1);
        return Ok(());
    }
    if node.opens == 0 {
        return Err(invalid());
    }
    node.opens = node.opens.saturating_sub(1);
    if node.opens > 0 || node.nodelete {
        return Ok(());
    }
    let keep = live_set(l, &[false; CAP]);
    let n = l.n_log.load(Relaxed).min(CAP);
    for k in (0..n).rev() {
        let j = l.log.get(k).map_or(CAP, |&j| usize::from(j));
        if !keep.get(j).copied().unwrap_or(true) {
            f.add(l, j);
        }
    }
    // A published object the log could not hold: after the logged ones.
    for j in (0..l.n).rev() {
        if !keep.get(j).copied().unwrap_or(true) {
            f.add(l, j);
        }
    }
    Ok(())
}

/// Which objects stay (plan §1): every object that is initial, `nodelete`, open, or finalising in
/// an outer call (`set`'s own excepted), and every object reachable from one through its
/// dependencies and its relocations' bindings.
fn live_set(l: &Loader, set: &[bool; CAP]) -> [bool; CAP] {
    let mut keep = [false; CAP];
    let mut stack = [0u16; CAP];
    let mut sp = 0usize;
    let push = |keep: &mut [bool; CAP], stack: &mut [u16; CAP], sp: &mut usize, i: usize| {
        if let (Some(k), Some(s)) = (keep.get_mut(i), stack.get_mut(*sp)) {
            if !*k {
                *k = true;
                *s = u16::try_from(i).unwrap_or(0);
                *sp = sp.saturating_add(1);
            }
        }
    };
    for (i, nd) in l.nodes.iter().enumerate().take(l.n) {
        let mine = set.get(i).copied().unwrap_or(false);
        if !nd.gone && (i < l.n_start || nd.nodelete || nd.opens > 0 || (nd.finalising && !mine)) {
            push(&mut keep, &mut stack, &mut sp, i);
        }
    }
    while sp > 0 {
        sp = sp.saturating_sub(1);
        let i = stack.get(sp).map_or(0, |&i| usize::from(i));
        for &d in l.deps_of(i).iter().chain(l.binds_of(i)) {
            let d = usize::from(d);
            if !l.gone(d) {
                push(&mut keep, &mut stack, &mut sp, d);
            }
        }
    }
    keep
}

/// After the finalisers of `f` ran: unpublish and retire every object of the set that is still not
/// reachable (one a destructor opened again stays, its destructors run), then [`reclaim`].
fn close_end(f: &Finis) {
    let Some(l) = loader() else { return };
    let mut set = [false; CAP];
    for &j in f.set() {
        if let Some(b) = set.get_mut(usize::from(j)) {
            *b = true;
        }
    }
    let keep = live_set(l, &set);
    let mut tls = super::NewTls::NONE;
    let mut generation = super::tls_generation();
    let mut out = [0u16; CAP];
    let mut n_out = 0usize;
    for &j in f.set() {
        let j = usize::from(j);
        if let Some(nd) = l.nodes.get_mut(j) {
            nd.finalising = false;
        }
        if keep.get(j).copied().unwrap_or(true) {
            continue;
        }
        if let Some(id) = unload(l, j) {
            generation = l.ids.release(id, generation);
            if let Some(m) = tls.mods.get_mut(tls.len) {
                m[3] = id;
                tls.len = tls.len.saturating_add(1);
            }
        }
        if let Some(o) = out.get_mut(n_out) {
            *o = u16::try_from(j).unwrap_or(0);
            n_out = n_out.saturating_add(1);
        }
    }
    if n_out == 0 {
        return;
    }
    // A survivor relocated with an unloaded root's scope now searches its own (`dlsym`'s
    // `RTLD_DEFAULT`/`RTLD_NEXT`).
    for i in l.n_start..l.n {
        let r = l.nodes.get(i).map_or(0, |nd| usize::from(nd.root));
        if !l.gone(i) && l.gone(r) {
            if let Some(nd) = l.nodes.get_mut(i) {
                nd.root = u16::try_from(i).unwrap_or(0);
            }
        }
    }
    if tls.len > 0 {
        super::release_tls(thread_walk(l), &mut tls, generation);
    }
    for &j in out.get(..n_out).unwrap_or(&[]) {
        if let Some(r) = l.retired.get_mut(l.n_retired) {
            *r = (j, false);
            l.n_retired = l.n_retired.saturating_add(1);
        }
    }
    reclaim(l);
}

/// Unpublish object `j`: the readers' table (and `dlpi_subs`), the chain, the global scope, the
/// constructor log, its static TLS place; its TLS module ID when it had one.
fn unload(l: &mut Loader, j: usize) -> Option<u64> {
    super::readers::unpublish(j);
    let (prev, next) = l.nodes.get(j).map_or((0, 0), |nd| (nd.l_prev, nd.l_next.load(Relaxed)));
    if let Some(p) = l.index_of(prev).and_then(|p| l.nodes.get(p)) {
        p.l_next.store(next, Release);
    }
    if let Some(nx) = l.index_of(next).and_then(|x| l.nodes.get_mut(x)) {
        nx.l_prev = prev;
    }
    let mut w = 0usize;
    for r in 0..l.n_global.min(CAP) {
        let g = l.global.get(r).copied().unwrap_or(j);
        if g != j {
            if let Some(slot) = l.global.get_mut(w) {
                *slot = g;
            }
            w = w.saturating_add(1);
        }
    }
    l.n_global = w;
    let n = l.n_log.load(Relaxed).min(CAP);
    let mut w = 0usize;
    for r in 0..n {
        let g = l.log.get(r).copied().unwrap_or(0);
        if usize::from(g) != j {
            if let Some(slot) = l.log.get_mut(w) {
                *slot = g;
            }
            w = w.saturating_add(1);
        }
    }
    l.n_log.store(w, Release);
    let tls = l.objs_upto(l.n).get(j).and_then(|o| o.tls);
    if let Some(nd) = l.nodes.get_mut(j) {
        nd.gone = true;
        nd.global = false;
        nd.opens = 0;
        (nd.dev, nd.ino) = (u64::MAX, u64::MAX.wrapping_sub(1));
        (nd.lo, nd.hi) = (0, 0);
        nd.l_next.store(0, Relaxed);
    }
    let p = tls?;
    let _ = l.surplus.free(p.tp_offset);
    Some(p.module)
}

/// Unmap every retired object no reader can still see, then give the slots at the end of the table
/// back: `n` and the arenas shrink over freed slots. Never waits. Two steps per object: the parity
/// flips once the readers of the one before have left ([`readers::drained`](super::readers::drained)),
/// and the object is unmapped when the readers counted before that flip have left in turn. With no
/// reader inside, both steps happen at once; otherwise at a later `dlopen`/`dlclose`.
fn reclaim(l: &mut Loader) {
    for _ in 0..2 {
        if !super::readers::drained() {
            break;
        }
        let (mut w, mut waiting) = (0usize, false);
        for r in 0..l.n_retired.min(CAP) {
            let (j, flipped) = l.retired.get(r).copied().unwrap_or((0, false));
            if flipped {
                if let Some(nd) = l.nodes.get_mut(usize::from(j)) {
                    map::Mapper::unmap(&mut SysMapper { fd: -1 }, nd.reserve.0, nd.reserve.1);
                    let _ = raw::munmap(nd.file.0, nd.file.1);
                    nd.freed = true;
                }
                continue;
            }
            if let Some(slot) = l.retired.get_mut(w) {
                *slot = (j, true);
            }
            (w, waiting) = (w.saturating_add(1), true);
        }
        l.n_retired = w;
        if !waiting {
            break;
        }
        super::readers::flip();
    }
    let mut n = l.n;
    while n > l.n_start && l.nodes.get(n.wrapping_sub(1)).is_some_and(|nd| nd.gone && nd.freed) {
        n = n.wrapping_sub(1);
    }
    if n < l.n {
        l.n = n;
        let end = |(a, b): (u32, u32)| usize::try_from(a).unwrap_or(0).saturating_add(usize::try_from(b).unwrap_or(0));
        let live = l.nodes.get(..n).unwrap_or(&[]);
        l.deps_top = live.iter().filter(|nd| !nd.gone).map(|nd| end(nd.deps).max(end(nd.binds))).max().unwrap_or(0);
        l.names_top = live.iter().filter(|nd| !nd.gone && nd.path.1 > 0).map(|nd| end(nd.path).saturating_add(1)).max().unwrap_or(0);
    }
    RETIRED.store(l.n_retired, Relaxed);
}

/// Retired objects past which a `dlopen` or `dlclose` first waits for the readers to drain
/// (ruling #5651): a quarter of the table. A reader preempted inside `dl_iterate_phdr` stalls every
/// unmap, so without the wait a loop of opens and closes would fill the table and refuse a valid
/// load.
const RETIRED_WAIT: usize = CAP / 4;
/// `Loader::n_retired` as [`reclaim`] last left it, read without the locks by [`room_wait`].
static RETIRED: AtomicUsize = AtomicUsize::new(0);
/// The poll interval of [`room_wait`] (`CLOCK_MONOTONIC`, nanoseconds).
const ROOM_POLL_NS: i64 = 50_000;
const CLOCK_MONOTONIC: i64 = 1;

/// At the entry of `dlopen` and `dlclose`, holding no loader lock: while more than
/// [`RETIRED_WAIT`] objects are retired, reclaim, and wait (poll) for the readers to drain when
/// that freed too few. Never waits when the calling thread holds [`CALL`] (a constructor or
/// destructor: a reader may be blocked on that lock) or is inside a read section (it may be the
/// reader); the call then goes on and fails as it would have (`cannot load more objects`) only
/// if the table is full. Each drain lets [`reclaim`] unmap what was flipped before it, so the loop
/// ends once the readers present now have left.
fn room_wait() {
    while RETIRED.load(Relaxed) > RETIRED_WAIT {
        if CALL.held_by_caller() || super::readers::inside() {
            return;
        }
        CALL.lock();
        STATE.lock();
        if let Some(l) = loader() {
            reclaim(l);
        }
        STATE.unlock();
        CALL.unlock();
        if RETIRED.load(Relaxed) <= RETIRED_WAIT {
            return;
        }
        while !super::readers::drained() {
            let ts = [0i64, ROOM_POLL_NS];
            let _ = crate::ffi_sys::liella_sys_clock_nanosleep(CLOCK_MONOTONIC, 0, ts.as_ptr().cast::<c_void>(), core::ptr::null_mut());
        }
    }
}

/// Record, for each new object `base..k`, the objects its relocations bound to that its own
/// dependencies do not list (a start-time object never unloads: left out). A full arena makes the
/// definer `nodelete` instead (it then stays, never unmapped under its user).
fn record_binds(l: &mut Loader, base: usize, k: usize, b: &Binds) {
    for i in base..k {
        let start = l.deps_top;
        for j in l.n_start..k {
            if !b.has(i, j) || l.deps_of(i).contains(&u16::try_from(j).unwrap_or(u16::MAX)) {
                continue;
            }
            match l.deps.get_mut(l.deps_top) {
                Some(slot) => {
                    *slot = u16::try_from(j).unwrap_or(0);
                    l.deps_top = l.deps_top.saturating_add(1);
                }
                None => {
                    if let Some(nd) = l.nodes.get_mut(j) {
                        nd.nodelete = true;
                    }
                }
            }
        }
        if let Some(nd) = l.nodes.get_mut(i) {
            nd.binds = (u32::try_from(start).unwrap_or(0), u32::try_from(l.deps_top.saturating_sub(start)).unwrap_or(0));
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
    if let Some(j) = l.nodes[..k].iter().position(|nd| !nd.gone && (nd.dev, nd.ino) == (st.dev, st.ino)) {
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
        super::readers::count_unmapped(1);
        return refused(l, Refusal::Malformed);
    };
    let Some(span) = l.keep(shown) else {
        loaded.unmap(&mut SysMapper { fd: -1 });
        let _ = raw::munmap(at, len);
        super::readers::count_unmapped(1);
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
    node.binds = (0, 0);
    (node.finalising, node.gone, node.freed) = (false, false, false);
    // Its reader entry, invisible until the set is published.
    super::readers::stage(k, node.l_addr, name, usize::try_from(loaded.phdr).unwrap_or(0), usize::from(loaded.phnum));
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
    reclaim(l);
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

/// Unmap the objects `base..k` of a failed attempt and rewind the arenas and the unique table.
fn unwind(l: &mut Loader, base: usize, k: usize, names_top: usize, deps_top: usize) {
    for node in l.nodes.get(base..k).unwrap_or(&[]) {
        map::Mapper::unmap(&mut SysMapper { fd: -1 }, node.reserve.0, node.reserve.1);
        let _ = raw::munmap(node.file.0, node.file.1);
    }
    super::readers::count_unmapped(k.saturating_sub(base));
    for i in (base..k).rev() {
        if let Some(p) = l.objs_upto(k).get(i).and_then(|o| o.tls) {
            l.ids.put_back(p.module);
        }
    }
    l.names_top = names_top;
    l.deps_top = deps_top;
    l.unique.truncate(l.unique_mark);
    l.surplus.rollback(l.tls_mark);
}

/// Pin the definers of the unique-table entries recorded since mark `from`: an object whose
/// `STB_GNU_UNIQUE` definition was bound is never unloaded (plan §1, design §5).
fn pin_unique(l: &mut Loader, from: usize) {
    let mut owners = [0u16; 64];
    let mut from = from;
    loop {
        let mut n = 0usize;
        for (slot, o) in owners.iter_mut().zip(l.unique.owners(from)) {
            *slot = u16::try_from(o).unwrap_or(u16::MAX);
            n = n.saturating_add(1);
        }
        for &o in owners.get(..n).unwrap_or(&[]) {
            if let Some(node) = l.nodes.get_mut(usize::from(o)) {
                node.nodelete = true;
            }
        }
        if n < owners.len() {
            return;
        }
        from = from.saturating_add(n);
    }
}

/// Give each new object (`base..k`) with `PT_TLS` a module ID and a place in the static surplus
/// (DL-16d, plan/ph14/dl-16-dlopen.md §5), recorded in its `Obj` for relocation and in `new` for
/// [`super::publish_tls`]; `new.len` stays 0 for a set without TLS. Nothing is written to any
/// thread yet. A block that has no place fails with `<path>: cannot allocate memory in static TLS
/// block` (dynamic blocks are DL-19's), as does a C library without the thread walk.
fn place_tls(l: &mut Loader, base: usize, k: usize, new: &mut super::NewTls) -> Result<(), Fail> {
    let (lo, hi, tp_align) = super::surplus();
    for i in base..k {
        let Some(mut o) = l.objs_upto(k).get(i).copied() else { continue };
        let Some(ph) = o.elf.find(pt::TLS) else { continue };
        let m = Module::of(&ph);
        let refused = |l: &Loader| fail_parts(&[l_path_static(l, i), b": ", Refusal::StaticTls.text()]);
        if !ltls::image_in_load(&o.elf, &m) {
            return Err(fail_parts(&[l_path_static(l, i), b": ", Refusal::Malformed.text()]));
        }
        if thread_walk(l) == 0 {
            return Err(refused(l));
        }
        let (floor, max) = super::module_room();
        let Some(off) = l.surplus.place::<Native>(lo, hi, tp_align, &m) else { return Err(refused(l)) };
        let (Some(id), Some(slot)) = (l.ids.take(floor, max), new.mods.get_mut(new.len)) else {
            let _ = l.surplus.free(off);
            return Err(refused(l));
        };
        *slot = [o.bias.wrapping_add(m.vaddr), m.filesz, m.memsz, id, u64::from_ne_bytes(off.to_ne_bytes())];
        new.len = new.len.wrapping_add(1);
        o.tls = Some(Placed { module: id, tp_offset: off });
        if let Some(s) = l.objs.get_mut(i) {
            *s = MaybeUninit::new(o);
        }
    }
    Ok(())
}

/// The C library's `__liella_thread_walk` (a start object's, at `LIELLA_PRIVATE`), looked up once
/// by the first `dlopen` that loads TLS; 0 when no start object has it.
fn thread_walk(l: &mut Loader) -> usize {
    if l.thread_walk == 0 {
        let req = Request::new(start::private::THREAD_WALK, Want::Version(start::private::NODE));
        let n = START_N.load(Acquire).min(l.n);
        let found = l.objs_upto(n).iter().zip(syms_of(&l.syms, n)).find_map(|(o, s)| s.lookup(&req).ok().flatten().map(|d| o.bias.wrapping_add(d.st.value)));
        l.thread_walk = found.and_then(|a| usize::try_from(a).ok()).unwrap_or(0);
    }
    l.thread_walk
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
    l.unique_mark = l.unique.len();
    l.tls_mark = l.surplus.len();
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
    if let Err(e) = reloc::check_versions_live(l.objs_upto(k), |j| !l.gone(j)) {
        let t = version_text(l, k, e);
        unwind(l, base, k, names_top, deps_top);
        return Err(t);
    }
    // Static TLS places before relocation (its TPOFF / DTPMOD values need them, DL-16d).
    let mut new_tls = super::NewTls::NONE;
    if let Err(t) = place_tls(l, base, k, &mut new_tls) {
        unwind(l, base, k, names_top, deps_top);
        return Err(t);
    }
    let mut local = [0u16; CAP];
    let llen = local_scope(l, base, &mut local);
    // The new set's scope (plan §2): the global scope, then the root's local scope; with
    // `RTLD_DEEPBIND` the local scope first. Each object once.
    let mut scope = [0usize; CAP];
    let mut sn = 0usize;
    let mut seen = [false; CAP];
    let global = l.global[..l.n_global].iter().copied();
    let local_it = local[..llen].iter().map(|&i| usize::from(i));
    let mut push = |i: usize| {
        if i < CAP && !seen[i] && sn < CAP {
            seen[i] = true;
            scope[sn] = i;
            sn += 1;
        }
    };
    if m.deepbind {
        local_it.chain(global).for_each(&mut push);
    } else {
        global.chain(local_it).for_each(&mut push);
    }
    for i in base..k {
        l.nodes[i].root = u16::try_from(base).unwrap_or(0);
    }
    let start = &l.global[..l.n_start_global];
    let (objs, syms) = (objs_of(&l.objs, k), syms_of(&l.syms, k));
    let set = Objects { objs, syms, global: &scope[..sn] };
    let mut binds = Binds { base, bits: [[0; CAP / 64]; NEW_MAX] };
    let mut unique = Uniq { table: &mut l.unique, syms, start, binds: Some(&mut binds) };
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
    record_binds(l, base, k, &binds);
    // Every thread gets the new modules' blocks, and later threads the table, before any code
    // can reach them (DL-16d; nothing below fails).
    if new_tls.len > 0 {
        super::publish_tls(l.thread_walk, &mut new_tls);
    }
    // Publication, after pass 2 and before the constructors (plan §4): one Release store links the
    // whole new set into the chain, one makes it visible to the readers (`dl_iterate_phdr`,
    // `_dl_find_object`) with `dlpi_adds`.
    for i in base..k {
        let next = if i + 1 < k { l.node_addr(i + 1) } else { 0 };
        l.nodes[i].l_next.store(next, Relaxed);
    }
    l.n = k;
    let first = l.node_addr(base);
    // The chain's tail: the last object below the set that is not unloaded (the program at least).
    let tail = (0..base).rev().find(|&i| !l.nodes[i].gone).unwrap_or(0);
    l.nodes[base].l_prev = l.node_addr(tail);
    l.nodes[tail].l_next.store(first, Release);
    super::readers::publish(base, k);
    l.nodes[base].opens = 1;
    l.nodes[base].nodelete |= m.nodelete;
    pin_unique(l, l.unique_mark);
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
        match l.objs_upto(k).iter().enumerate().position(|(j, o)| !l.gone(j) && (soname(o) == Some(file) || o.name == file)) {
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
