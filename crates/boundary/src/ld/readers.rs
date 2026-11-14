//! The loader's lock-free readers (phase 14 DL-16c `DL-DLOPEN-CONCURRENT`; plan/ph14/dl-16-dlopen.md
//! §4; D-27.1, D-27.30). `dl_iterate_phdr` and `_dl_find_object` of a program `ld.so` started reach
//! [`iterate`] and [`find_object`] through `StartInfo::dl_iterate` and `dl_find_object`.
//!
//! * **The table.** One entry per loaded object in chain order (node `i` of `super::dlopen` is
//!   entry `i`): load bias, name, program header table and count. Append-only like the chain: a
//!   `dlopen` writes its new objects' entries while they are invisible ([`stage`]) and makes them
//!   visible with one Release store of [`PUBLISHED`] at the set's publication point, after pass 2
//!   and before the constructors ([`publish`]). Readers load [`PUBLISHED`] with Acquire and read
//!   only entries below it, which never change again; they never form a reference to the
//!   writer's `Loader`, so a `dlopen` in progress cannot be observed half done.
//! * **The start-time objects** fill their entries at the first reader or `dlopen` (nothing at
//!   start): every filler stores the same words, so racing first readers need no lock.
//! * **The walk is live** (probe `tools/probe/dl/readers`, R-2): each step reloads [`PUBLISHED`]
//!   and the counters, so a callback that calls `dlopen` sees the new object later in the same
//!   walk, and the callback runs without any lock (ruling #2607; s27_1_c_8 says the reference
//!   holds one: Liella's callbacks never block a `dlopen` in another thread).
//! * **Counters** (dynlink.md `== phdr`): `dlpi_adds` counts every object ever mapped (the start
//!   objects and the vDSO, then each object a `dlopen` maps, also in an attempt that fails),
//!   `dlpi_subs` every object unmapped (a failed attempt's).
//! * Every read goes through [`read_section`], empty in DL-16; DL-17 gives it per-thread reader
//!   epochs so `dlclose` can free what a reader may still see.

use super::dlopen::{node_address, CAP};
use super::{read_auxv, read_image, AUXV_WORDS_MAX, OWN_BASE, START_ARGS, START_INTERP, START_N, START_OBJS, THREAD_MODS, THREAD_MODS_LEN};
use crate::posix::s27_dynlink::{find_in, int_result, vdso_headers};
use crate::{conv, tb};
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use core::sync::atomic::{AtomicU64, AtomicUsize};
use liella_abi::layout::{struct_dl_find_object, struct_dl_phdr_info};
use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::AT_SYSINFO_EHDR;

const PT_DYNAMIC: u64 = 2;
const PT_TLS: u64 = 7;
const DT_STRTAB: u64 = 5;
const DT_SONAME: u64 = 14;

/// Per object: `[load bias, name address, program header table address, header count]`.
static TABLE: [[AtomicUsize; 4]; CAP] = [const { [const { AtomicUsize::new(0) }; 4] }; CAP];
/// Entries a reader may read (0: the start-time entries are not filled yet); only grows.
static PUBLISHED: AtomicUsize = AtomicUsize::new(0);
/// Objects `dlopen` calls have made visible or mapped and unmapped again ([`publish`],
/// [`count_unmapped`]); the start-time objects are counted apart.
static ADDS: AtomicU64 = AtomicU64::new(0);
static SUBS: AtomicU64 = AtomicU64::new(0);
/// The vDSO's address (`AT_SYSINFO_EHDR`, 0: none); `usize::MAX` until read.
static VDSO: AtomicUsize = AtomicUsize::new(usize::MAX);

/// What a reader sees: the published objects and the counters. DL-17 brackets the closure with a
/// reader epoch; in DL-16 nothing is ever freed, so the section is empty.
pub(super) fn read_section<R>(f: impl FnOnce(&View) -> R) -> R {
    f(&View { start: fill_start() })
}

/// A reader's view (see [`read_section`]).
pub(super) struct View {
    /// The start-time entries (the program, its dependencies, `ld.so`); 0 when `link` did not run.
    start: usize,
}

/// One published object.
#[derive(Clone, Copy)]
struct Entry {
    bias: usize,
    name: usize,
    phdr: usize,
    phnum: usize,
}

impl View {
    /// Entries visible now (reloaded at every step: the walk is live).
    fn len(&self) -> usize {
        PUBLISHED.load(Acquire).min(CAP)
    }

    /// Entry `i`, below [`View::len`].
    fn entry(&self, i: usize) -> Entry {
        let w: [usize; 4] = core::array::from_fn(|j| TABLE.get(i).and_then(|e| e.get(j)).map_or(0, |x| x.load(Relaxed)));
        Entry { bias: w[0], name: w[1], phdr: w[2], phnum: w[3] }
    }

    /// `(dlpi_adds, dlpi_subs)` now: the start-time objects and the vDSO, then the `dlopen`s'.
    fn counters(&self) -> (u64, u64) {
        let start = u64::try_from(self.start).unwrap_or(0).saturating_add(u64::from(vdso() != 0));
        (start.saturating_add(ADDS.load(Acquire)), SUBS.load(Acquire))
    }
}

/// The vDSO's address from the start's auxiliary vector (0: none).
fn vdso() -> usize {
    let v = VDSO.load(Relaxed);
    if v != usize::MAX {
        return v;
    }
    let auxv = START_ARGS.get(3).map_or(0, |w| w.load(Relaxed));
    let mut copy = [0u64; AUXV_WORDS_MAX];
    let words = if auxv == 0 { 0 } else { read_auxv(auxv, &mut copy).unwrap_or(0) };
    let aux = Auxv::from_words(copy.get(..words).unwrap_or(&[]));
    let v = aux.get(AT_SYSINFO_EHDR).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    VDSO.store(v, Relaxed);
    v
}

/// The program headers of an object as `(p_type, p_vaddr, p_memsz)` triples.
fn headers(phdr: usize, phnum: usize) -> impl Iterator<Item = (u64, u64, u64)> {
    // UNSAFE(slice): a loaded object's program header table (⚑ KERN for the program, `map::load`
    // or the start's loader for the others), mapped for the process's lifetime (DL-16 never unmaps a
    // published object).
    #[allow(unsafe_code)]
    // SAFETY: `phdr`/`phnum` come from a published entry: the table of an object that stays mapped.
    let w = unsafe { conv::words_at(phdr, phnum.saturating_mul(7)) };
    w.chunks_exact(7).map(|h| (h.first().copied().unwrap_or(0) & 0xffff_ffff, h.get(2).copied().unwrap_or(0), h.get(5).copied().unwrap_or(0)))
}

/// The `DT_SONAME` of the object (`bias`, program headers at `phdr`) as a run-time address, from its
/// dynamic section in memory (link-time values, as `super::dlopen::adopt_start` reads it); 0: none.
fn soname_at(bias: usize, phdr: usize, phnum: usize) -> usize {
    let Some((_, vaddr, memsz)) = headers(phdr, phnum).find(|h| h.0 == PT_DYNAMIC) else { return 0 };
    let at = bias.wrapping_add(usize::try_from(vaddr).unwrap_or(0));
    // UNSAFE(slice): the object's dynamic section, inside one of its mapped PT_LOADs.
    #[allow(unsafe_code)]
    // SAFETY: PT_DYNAMIC of a start-time object, which `link` read and relocated; `memsz / 8` words.
    let d = unsafe { conv::words_at(at, usize::try_from(memsz / 8).unwrap_or(0)) };
    let (mut strtab, mut soname) = (None, None);
    for e in d.chunks_exact(2) {
        match e.first().copied().unwrap_or(0) {
            0 => break,
            DT_STRTAB => strtab = e.get(1).copied(),
            DT_SONAME => soname = e.get(1).copied(),
            _ => {}
        }
    }
    match (strtab, soname) {
        (Some(s), Some(o)) => bias.wrapping_add(usize::try_from(s.wrapping_add(o)).unwrap_or(0)),
        _ => 0,
    }
}

/// Fill the start-time entries (the order of `super::dlopen::adopt_start`: the objects `link`
/// recorded, `ld.so` at its index or after them) and publish them unless more is published: the
/// count of start-time entries (0: `link` did not run). Idempotent and lock-free.
fn fill_start() -> usize {
    let n = START_N.load(Acquire);
    if n == 0 || n >= CAP {
        return 0;
    }
    let interp = START_INTERP.load(Relaxed);
    let total = if interp < n { n } else { n.saturating_add(1) };
    if PUBLISHED.load(Acquire) != 0 {
        return total;
    }
    for k in 0..total {
        let (phdr, phnum, bias) = if k == interp || k == n {
            let base = OWN_BASE.load(Relaxed);
            let mut header = [0u8; liella_ld::elf::EHDR_SIZE];
            read_image(base, &mut header);
            let (phoff, phnum) = liella_ld::boot::own_headers(&header).unwrap_or((0, 0));
            (base.wrapping_add(usize::try_from(phoff).unwrap_or(0)), phnum, base)
        } else {
            let w: [usize; 3] = core::array::from_fn(|j| START_OBJS.get(k).and_then(|s| s.get(j)).map_or(0, |x| usize::try_from(x.load(Relaxed)).unwrap_or(0)));
            (w[0], w[1], w[2])
        };
        let name = if k == 0 { c"".as_ptr().expose_provenance() } else { soname_at(bias, phdr, phnum) };
        let name = if name == 0 { c"".as_ptr().expose_provenance() } else { name };
        stage(k, bias, name, phdr, phnum);
    }
    let _ = PUBLISHED.compare_exchange(0, total, Release, Relaxed);
    total
}

/// Write entry `k` of an object not visible yet (`super::dlopen::candidate`).
pub(super) fn stage(k: usize, bias: usize, name: usize, phdr: usize, phnum: usize) {
    if let Some(e) = TABLE.get(k) {
        for (w, v) in e.iter().zip([bias, name, phdr, phnum]) {
            w.store(v, Relaxed);
        }
    }
}

/// The publication point of one `dlopen` (`super::dlopen::load_set`, after pass 2, before the
/// constructors): entries below `len` become visible, `added` of them new.
pub(super) fn publish(len: usize, added: usize) {
    fill_start();
    ADDS.fetch_add(u64::try_from(added).unwrap_or(0), Relaxed);
    PUBLISHED.store(len, Release);
}

/// A failed attempt mapped `n` objects and unmapped them again (dynlink.md: both counters move).
pub(super) fn count_unmapped(n: usize) {
    let n = u64::try_from(n).unwrap_or(0);
    ADDS.fetch_add(n, Relaxed);
    SUBS.fetch_add(n, Release);
}

/// The TLS module ID and the calling thread's block of the object, when it has a `PT_TLS` segment:
/// DL-15's records of the start-time modules, which DL-16d's `dlopen` extends with each new module
/// before [`publish`] lists its object; `(0, 0)` otherwise.
fn tls_of(e: &Entry) -> (u64, usize) {
    let Some((_, vaddr, _)) = headers(e.phdr, e.phnum).find(|h| h.0 == PT_TLS) else { return (0, 0) };
    let image = u64::try_from(e.bias).unwrap_or(0).wrapping_add(vaddr);
    let n = THREAD_MODS_LEN.load(Acquire);
    for m in THREAD_MODS.iter().take(n) {
        let [src, _, _, module, off]: [u64; 5] = core::array::from_fn(|i| m.get(i).map_or(0, |w| w.load(Relaxed)));
        if src == image {
            let tp = u64::try_from(tb::tp()).unwrap_or(0);
            return (module, usize::try_from(tp.wrapping_add_signed(i64::from_ne_bytes(off.to_ne_bytes()))).unwrap_or(0));
        }
    }
    (0, 0)
}

/// The address of [`iterate`], for `StartInfo::dl_iterate`.
pub(super) fn iterate_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (iterate as *const ()).expose_provenance()
}

/// The address of [`find_object`], for `StartInfo::dl_find_object`.
pub(super) fn find_object_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (find_object as *const ()).expose_provenance()
}

/// Call `cb(info, size, data)`: its `int` result.
fn call(cb: usize, info: &struct_dl_phdr_info, data: usize) -> i32 {
    let w = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
    let at = core::ptr::from_ref(info).expose_provenance();
    int_result(crate::ffi::liella_machine_call3(cb, w(at), w(size_of::<struct_dl_phdr_info>()), w(data)))
}

/// `dl_iterate_phdr(cb, data)` in a program `ld.so` started: every published object in chain
/// order, the vDSO second (probe `== phdr`, P-12), until a callback returns non-zero (returned).
extern "C" fn iterate(cb: usize, data: usize) -> i64 {
    read_section(|v| {
        let mut i = 0usize;
        while i < v.len() {
            let e = v.entry(i);
            let (adds, subs) = v.counters();
            let (modid, block) = tls_of(&e);
            let info = struct_dl_phdr_info { dlpi_addr: u64::try_from(e.bias).unwrap_or(0), dlpi_name: e.name, dlpi_phdr: e.phdr, dlpi_phnum: u16::try_from(e.phnum).unwrap_or(0), dlpi_adds: adds, dlpi_subs: subs, dlpi_tls_modid: modid, dlpi_tls_data: block };
            let r = call(cb, &info, data);
            if r != 0 {
                return i64::from(r);
            }
            let vd = vdso();
            if i == 0 && vd != 0 {
                let (vphdr, vphnum, bias) = vdso_headers(vd);
                let (adds, subs) = v.counters();
                let info = struct_dl_phdr_info { dlpi_addr: u64::try_from(bias).unwrap_or(0), dlpi_name: c"linux-vdso.so.1".as_ptr().expose_provenance(), dlpi_phdr: vphdr, dlpi_phnum: u16::try_from(vphnum).unwrap_or(0), dlpi_adds: adds, dlpi_subs: subs, dlpi_tls_modid: 0, dlpi_tls_data: 0 };
                let r = call(cb, &info, data);
                if r != 0 {
                    return i64::from(r);
                }
            }
            i = i.wrapping_add(1);
        }
        0
    })
}

/// `_dl_find_object(pc, result)` in a program `ld.so` started: the published object holding `pc`
/// (a `PT_LOAD`), its whole `PT_LOAD` span, `PT_GNU_EH_FRAME` and link map (probe R-1; the link
/// map is 0 before the first `dlopen` makes the loader's nodes, and for the vDSO); 0, or -1 with
/// `*result` untouched.
extern "C" fn find_object(pc: usize, result: usize) -> i64 {
    let found = read_section(|v| {
        (0..v.len()).find_map(|i| {
            let e = v.entry(i);
            find_in(e.phdr, e.phnum, e.bias, pc, true).map(|(lo, hi, eh)| (lo, hi, eh, node_address(i)))
        })
    });
    let found = found.or_else(|| {
        let vd = vdso();
        if vd == 0 {
            return None;
        }
        let (vphdr, vphnum, bias) = vdso_headers(vd);
        find_in(vphdr, vphnum, bias, pc, true).map(|(lo, hi, eh)| (lo, hi, eh, 0))
    });
    let Some((lo, hi, eh, map)) = found else { return -1 };
    // UNSAFE(slice): the caller's `struct dl_find_object` (§27.1).
    #[allow(unsafe_code)]
    // SAFETY: `result` is the caller's pointer to a writable `struct dl_find_object` (the contract of
    // `_dl_find_object`); NULL is refused.
    let Some(r) = (unsafe { core::ptr::with_exposed_provenance_mut::<struct_dl_find_object>(result).as_mut() }) else { return -1 };
    *r = struct_dl_find_object { dlfo_flags: 0, dlfo_map_start: lo, dlfo_map_end: hi, dlfo_link_map: map, dlfo_eh_frame: eh, __dflo_reserved: [0; 7] };
    0
}
