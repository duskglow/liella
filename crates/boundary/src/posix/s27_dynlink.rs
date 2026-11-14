//! Translation for spec §27: dynamic linking.
//!
//! Implementation phase: 14 (implementation-plan.md §6). Not yet implemented.
//!
//! `dl_iterate_phdr` (D-27.4): a static program has two objects, as glibc reports them: the
//! main program (name "", `AT_PHDR`/`AT_PHNUM`, bias from `PT_PHDR` or without one from its ELF
//! header as `ld.so` finds it (D-27.18), or in a static PIE the bias its start relocated it by,
//! D-27.12) and the vDSO
//! ("linux-vdso.so.1") when the kernel provided one. `dlpi_adds` is the number of objects,
//! `dlpi_subs` 0; the main program's `dlpi_tls_modid` is 1 and `dlpi_tls_data` the calling
//! thread's block when it has a `PT_TLS` segment. A non-zero callback result ends the walk and is
//! returned. The walk reads immutable startup data only, so it is async-signal-safe.
//!
//! `_dl_find_object` (D-27.5; the unwinder's lookup, libgcc_eh calls it): for a `pc` in one of
//! the main program's `PT_LOAD` segments, that segment's range (a static glibc program reports
//! the segment, not the whole object: probe PTHREAD-ONCE-UNWIND-2), for one in the vDSO the
//! vDSO's whole mapped range; `dlfo_eh_frame` is the object's `PT_GNU_EH_FRAME` (0 without
//! one), `dlfo_flags` 0, 0 returned. Else -1 and `*result` untouched. Gap: `dlfo_link_map` is 0
//! (Liella has no `struct link_map` before phase 14; glibc gives a non-null one).
//!
//! In a program Liella's `ld.so` started, both forward to the loader's lock-free readers
//! (`crates/boundary/src/ld/readers.rs`, DL-16c): every loaded object in load order, the vDSO
//! second, the probed `dlpi_adds`/`dlpi_subs`, and `_dl_find_object`'s whole mapped range with the
//! object's link map (probe `tools/probe/dl/readers`).

use crate::{conv, export, process, tb};
use crate::sys::CSys;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use liella_abi::ctypes::{c_char, c_int, c_uint, c_void};
use liella_rt::cpu::{feature_leaves, FEATURE_LEAVES};
use liella_abi::layout::{struct_dl_find_object, struct_dl_phdr_info};
use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::{AT_PHDR, AT_PHNUM, AT_SYSINFO_EHDR};

type Callback = Option<extern "C" fn(&struct_dl_phdr_info, usize, *mut c_void) -> c_int>;

const PT_LOAD: u32 = 1;
const PT_TLS: u32 = 7;
const PT_GNU_EH_FRAME: u32 = 0x6474_e550;

fn word(w: &[u64], i: usize) -> usize {
    w.get(i).map_or(0, |&v| usize::try_from(v).unwrap_or(0))
}

/// Bias and TLS segment (memsz, align) of the headers at `phdr`: the executable's (`None`; its
/// bias from the start, D-27.12) or the vDSO's (`Some(base)`, bias from its first `PT_LOAD`).
pub(crate) fn scan(phdr: usize, phnum: usize, first_load_bias_from: Option<usize>) -> (usize, Option<(usize, usize)>) {
    // UNSAFE(slice): program headers the kernel mapped (⚑ KERN) and that stay mapped.
    #[allow(unsafe_code)]
    // SAFETY: `AT_PHDR`/`AT_PHNUM` (or the vDSO image's e_phoff/e_phnum) describe mapped headers.
    let w = unsafe { conv::words_at(phdr, phnum.saturating_mul(7)) };
    let hs = || w.chunks_exact(7).take(phnum);
    let ty = |h: &[u64]| u32::try_from(h.first().copied().unwrap_or(0) & 0xffff_ffff).unwrap_or(0);
    let bias = match first_load_bias_from {
        Some(base) => hs().find(|h| ty(h) == PT_LOAD).map_or(0, |h| base.wrapping_sub(word(h, 2))),
        None => process::exe_bias(w, phnum, phdr),
    };
    let tls = hs().find(|h| ty(h) == PT_TLS).map(|h| (word(h, 5), word(h, 6)));
    (bias, tls)
}

/// The vDSO's program header table (address, count) and load bias, from its ELF64 header at
/// `vdso` (`AT_SYSINFO_EHDR`): e_phoff u64 at 32, e_phnum u16 at 56.
pub(crate) fn vdso_headers(vdso: usize) -> (usize, usize, usize) {
    // UNSAFE(slice): the vDSO's ELF64 header (64 bytes), mapped by the kernel.
    #[allow(unsafe_code)]
    // SAFETY: `AT_SYSINFO_EHDR` is the kernel's own vDSO image (KERN).
    let h = unsafe { conv::words_at(vdso, 8) };
    let vphdr = vdso.wrapping_add(word(h, 4));
    let vphnum = usize::try_from(h.get(7).copied().unwrap_or(0) & 0xffff).unwrap_or(0);
    (vphdr, vphnum, scan(vphdr, vphnum, Some(vdso)).0)
}

/// The low 32 bits of a call's `rax` as the `int` a C function returned there.
pub(crate) fn int_result(r: i64) -> c_int {
    i32::from_ne_bytes(u32::try_from(r & 0xffff_ffff).unwrap_or(0).to_ne_bytes())
}

/// The loader's readers and its `fork` step (`StartInfo::dl_iterate`, `dl_find_object`,
/// `dl_fork`, stored by `runtime::early_init`; DL-16c): 0 in a program `ld.so` did not start.
pub static LOADER_ITERATE: AtomicUsize = AtomicUsize::new(0);
/// The loader's `_dl_find_object` (see [`LOADER_ITERATE`]).
pub static LOADER_FIND: AtomicUsize = AtomicUsize::new(0);
/// The loader's `fork` step (see [`LOADER_ITERATE`] and [`loader_fork`]).
pub static LOADER_FORK: AtomicUsize = AtomicUsize::new(0);

/// `fork`'s loader step (`crate::fork`): 0 before the clone, 1 in the parent, 2 in the child.
/// Nothing in a program `ld.so` did not start.
pub(crate) fn loader_fork(step: i64) {
    let f = LOADER_FORK.load(Ordering::Acquire);
    if f != 0 {
        let _ = crate::ffi::liella_machine_call3(f, step, 0, 0);
    }
}

export! { "GLIBC_2.2.5";
/// `dl_iterate_phdr(cb, data)` (§27.1, s27_1_c_8). In a program `ld.so` started, the loader's
/// walk over every loaded object, `dlopen`'d ones included (`crates/boundary/src/ld/readers.rs`,
/// DL-16c: no lock; the callback may call `dlopen`). Elsewhere the static program's two objects.
pub extern "C" fn dl_iterate_phdr(cb: Callback, data: *mut c_void) -> c_int {
    let Some(cb) = cb else { return 0 };
    let walk = LOADER_ITERATE.load(Ordering::Acquire);
    if walk != 0 {
        #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
        let f = (cb as *const ()).expose_provenance();
        let w = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
        return int_result(crate::ffi::liella_machine_call3(walk, w(f), w(data.expose_provenance()), 0));
    }
    let words = process::AUXV_WORDS.load(core::sync::atomic::Ordering::Acquire);
    let addr = process::AUXV_ADDR.load(core::sync::atomic::Ordering::Acquire);
    let aux = Auxv::from_words(conv::startup_auxv(addr, words));
    let get = |k| aux.get(k).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let (phdr, phnum, vdso) = (get(AT_PHDR), get(AT_PHNUM), get(AT_SYSINFO_EHDR));
    let adds = u64::from(phdr != 0).saturating_add(u64::from(vdso != 0));
    let size = core::mem::size_of::<struct_dl_phdr_info>();
    if phdr != 0 {
        let (bias, tls) = scan(phdr, phnum, None);
        let (modid, data_ptr): (_, usize) = match tls {
            Some((memsz, align)) => (1, tb::tp().wrapping_sub(memsz.next_multiple_of(align.max(1)))),
            None => (0, 0),
        };
        let info = struct_dl_phdr_info { dlpi_addr: u64::try_from(bias).unwrap_or(0), dlpi_name: c"".as_ptr().addr(), dlpi_phdr: phdr, dlpi_phnum: u16::try_from(phnum).unwrap_or(0), dlpi_adds: adds, dlpi_subs: 0, dlpi_tls_modid: modid, dlpi_tls_data: data_ptr };
        let r = cb(&info, size, data);
        if r != 0 {
            return r;
        }
    }
    if vdso != 0 {
        // UNSAFE(slice): the vDSO's ELF64 header (e_phoff u64 at 32, e_phentsize u16 at 54,
        // e_phnum u16 at 56; 64 bytes in all), mapped by the kernel.
        #[allow(unsafe_code)]
        // SAFETY: `AT_SYSINFO_EHDR` is the kernel's own vDSO image (KERN).
        let h = unsafe { conv::words_at(vdso, 8) };
        let phoff = word(h, 4);
        let hw = h.get(6).copied().unwrap_or(0); // bytes 48..56: e_flags, e_ehsize, e_phentsize
        let w7 = h.get(7).copied().unwrap_or(0); // bytes 56..64: e_phnum, e_shentsize, e_shnum, e_shstrndx
        let (phentsize, vphnum) = (usize::try_from((hw >> 48) & 0xffff).unwrap_or(0), usize::try_from(w7 & 0xffff).unwrap_or(0));
        let vphdr = vdso.wrapping_add(phoff);
        debug_assert!(phentsize == 56 || phentsize == 0);
        let (bias, _) = scan(vphdr, vphnum, Some(vdso));
        let info = struct_dl_phdr_info { dlpi_addr: u64::try_from(bias).unwrap_or(0), dlpi_name: c"linux-vdso.so.1".as_ptr().addr(), dlpi_phdr: vphdr, dlpi_phnum: u16::try_from(vphnum).unwrap_or(0), dlpi_adds: adds, dlpi_subs: 0, dlpi_tls_modid: 0, dlpi_tls_data: 0 };
        return cb(&info, size, data);
    }
    0
}}

/// The range and `PT_GNU_EH_FRAME` address of the object (headers `phdr`, `phnum`, load bias
/// `bias`) that holds `pc`: the `PT_LOAD` segment's range, or the object's whole range when
/// `whole`.
pub(crate) fn find_in(phdr: usize, phnum: usize, bias: usize, pc: usize, whole: bool) -> Option<(usize, usize, usize)> {
    // UNSAFE(slice): program headers the kernel mapped (⚑ KERN) and that stay mapped.
    #[allow(unsafe_code)]
    // SAFETY: `AT_PHDR`/`AT_PHNUM` (or the vDSO image's e_phoff/e_phnum) describe mapped headers.
    let w = unsafe { conv::words_at(phdr, phnum.saturating_mul(7)) };
    let ty = |h: &[u64]| u32::try_from(h.first().copied().unwrap_or(0) & 0xffff_ffff).unwrap_or(0);
    let loads = || w.chunks_exact(7).take(phnum).filter(|h| ty(h) == PT_LOAD).map(|h| (bias.wrapping_add(word(h, 2)), bias.wrapping_add(word(h, 2)).wrapping_add(word(h, 5))));
    let seg = loads().find(|&(lo, hi)| lo <= pc && pc < hi)?;
    let (lo, hi) = if whole { (loads().map(|r| r.0).min().unwrap_or(seg.0), loads().map(|r| r.1).max().unwrap_or(seg.1)) } else { seg };
    let eh = w.chunks_exact(7).take(phnum).find(|h| ty(h) == PT_GNU_EH_FRAME).map_or(0, |h| bias.wrapping_add(word(h, 2)));
    Some((lo, hi, eh))
}

// `runtime`: it describes only a process Liella started (its startup auxv); interposed on a
// host process it would hide the host's shared objects from that process's unwinder.
export! { runtime "GLIBC_2.35";
pub extern "C" fn _dl_find_object(pc: *mut c_void, result: *mut struct_dl_find_object) -> c_int {
    let find = LOADER_FIND.load(Ordering::Acquire);
    if find != 0 {
        let w = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
        return int_result(crate::ffi::liella_machine_call3(find, w(pc.expose_provenance()), w(result.expose_provenance()), 0));
    }
    let pc = pc.addr();
    let words = process::AUXV_WORDS.load(core::sync::atomic::Ordering::Acquire);
    let addr = process::AUXV_ADDR.load(core::sync::atomic::Ordering::Acquire);
    let aux = Auxv::from_words(conv::startup_auxv(addr, words));
    let get = |k| aux.get(k).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let (phdr, phnum, vdso) = (get(AT_PHDR), get(AT_PHNUM), get(AT_SYSINFO_EHDR));
    let mut found = None;
    if phdr != 0 {
        found = find_in(phdr, phnum, scan(phdr, phnum, None).0, pc, false);
    }
    if found.is_none() && vdso != 0 {
        let (vphdr, vphnum, bias) = vdso_headers(vdso);
        found = find_in(vphdr, vphnum, bias, pc, true);
    }
    let Some((lo, hi, eh)) = found else { return -1 };
    // UNSAFE(slice) SAFETY: result points to the caller's struct dl_find_object (§27.1).
    let Some(r) = (unsafe { result.as_mut() }) else { return -1 };
    *r = struct_dl_find_object { dlfo_flags: 0, dlfo_map_start: lo, dlfo_map_end: hi, dlfo_link_map: 0, dlfo_eh_frame: eh, __dflo_reserved: [0; 7] };
    0
}}

/// The `__x86_get_cpuid_feature_leaf` table: entries `0..FEATURE_LEAVES`, then the all-zero
/// entry every other index gets (glibc hands out one shared zero entry). Filled on first use;
/// racing first callers store the same words, so a plain flag suffices.
static LEAVES: [[AtomicU32; 8]; FEATURE_LEAVES + 1] = [const { [const { AtomicU32::new(0) }; 8] }; FEATURE_LEAVES + 1];
static LEAVES_READY: AtomicBool = AtomicBool::new(false);

export! { "GLIBC_2.33";
/// `__x86_get_cpuid_feature_leaf(index)` (§27.5, `<sys/platform/x86.h>`): the
/// `struct cpuid_feature` (raw CPUID words, then the active words) of leaf `index`
/// (`liella_rt::cpu::feature_leaves`); any other index gets an all-zero entry, never null.
pub extern "C" fn __x86_get_cpuid_feature_leaf(index: c_uint) -> *const c_void {
    if !LEAVES_READY.load(Ordering::Acquire) {
        for (entry, words) in LEAVES.iter().zip(feature_leaves(&CSys)) {
            for (slot, w) in entry.iter().zip(words) {
                slot.store(w, Ordering::Relaxed);
            }
        }
        LEAVES_READY.store(true, Ordering::Release);
    }
    let i = usize::try_from(index).unwrap_or(FEATURE_LEAVES).min(FEATURE_LEAVES);
    LEAVES.get(i).map_or(core::ptr::null(), |e| e.as_ptr().cast())
}}

/// The loader's `dlopen` entry (`StartInfo::dlopen`, stored by `runtime::early_init`): 0 in a
/// program `ld.so` did not start (a static program; DL-16a, ruling #5005).
pub static LOADER_DLOPEN: AtomicUsize = AtomicUsize::new(0);

crate::internal_entry! {
/// `dlopen(file, mode)` (§27.1, dlopen(3); phase 14 DL-16a): `csys/src/machine/dl.S` passes the
/// caller's return address. In a program `ld.so` started, the loader's entry does the work
/// (`crates/boundary/src/ld/dlopen.rs`). Elsewhere (a static program, ruling #5005: loading
/// objects there is DL-16S) NULL with the text `<file>: shared objects cannot be loaded into a
/// statically linked program`, never a crash.
pub extern "C" fn liella_dl_open(file: *const c_char, mode: c_int, caller: usize) -> *mut c_void {
    let f = LOADER_DLOPEN.load(Ordering::Acquire);
    if f == 0 {
        // UNSAFE(slice): the caller's NUL-terminated name (dlopen(3)), or NULL.
        #[allow(unsafe_code)]
        // SAFETY: a non-NULL `file` is a NUL-terminated string (the C contract); `strz` reads up to
        // its NUL only.
        let name = if file.is_null() { &[][..] } else { unsafe { crate::strz::strz(file.cast()) }.unwrap_or(&[]) };
        crate::dlerr::set(&[name, b": shared objects cannot be loaded into a statically linked program"]);
        return core::ptr::null_mut();
    }
    let w = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
    let r = crate::ffi::liella_machine_call3(f, w(file.expose_provenance()), i64::from(mode), w(caller));
    core::ptr::with_exposed_provenance_mut(usize::from_ne_bytes(r.to_ne_bytes()))
}}

export! { asm "GLIBC_2.34";
/// `dlopen(file, mode)`: the symbol is `csys/src/machine/dl.S`'s (it captures the caller); this is
/// its Rust twin for tests, with no caller (the program is the requester).
pub extern "C" fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void {
    liella_dl_open(file, mode, 0)
}}

/// The loader's `dlsym`/`dlvsym` entry (`StartInfo::dlsym`, stored by `runtime::early_init`): 0 in a
/// program `ld.so` did not start (DL-16b; a static program, ruling #5005).
pub static LOADER_DLSYM: AtomicUsize = AtomicUsize::new(0);

crate::internal_entry! {
/// `dlsym(handle, name)` (`version` NULL) and `dlvsym(handle, name, version)` (§27.1, dlsym(3),
/// dlvsym(3); phase 14 DL-16b): `csys/src/machine/dl.S` passes the caller's return address. In a
/// program `ld.so` started, the loader's entry does the work (`crates/boundary/src/ld/dlopen/dlsym.rs`).
/// Elsewhere (a static program, ruling #5005, D-27.33) NULL with the text `<dlsym|dlvsym>: no
/// symbols can be looked up in a statically linked program`, never a crash.
pub extern "C" fn liella_dl_sym(handle: *mut c_void, name: *const c_char, version: *const c_char, caller: usize) -> *mut c_void {
    let f = LOADER_DLSYM.load(Ordering::Acquire);
    if f == 0 {
        let who: &[u8] = if version.is_null() { b"dlsym" } else { b"dlvsym" };
        crate::dlerr::set(&[who, b": no symbols can be looked up in a statically linked program"]);
        return core::ptr::null_mut();
    }
    let w = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
    let r = crate::ffi::liella_machine_call4(f, w(handle.expose_provenance()), w(name.expose_provenance()), w(version.expose_provenance()), w(caller));
    core::ptr::with_exposed_provenance_mut(usize::from_ne_bytes(r.to_ne_bytes()))
}}

export! { asm "GLIBC_2.34";
/// `dlsym(handle, name)`: the symbol is `csys/src/machine/dl.S`'s (it captures the caller); this is
/// its Rust twin for tests, with no caller (the program is the caller).
pub extern "C" fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void {
    liella_dl_sym(handle, name, core::ptr::null(), 0)
}}

export! { asm "GLIBC_2.34";
/// `dlvsym(handle, name, version)`: the symbol is `csys/src/machine/dl.S`'s; this is its Rust twin
/// for tests, with no caller.
pub extern "C" fn dlvsym(handle: *mut c_void, name: *const c_char, version: *const c_char) -> *mut c_void {
    liella_dl_sym(handle, name, version, 0)
}}

/// The loader's `dlclose` entry (`StartInfo::dlclose`, stored by `runtime::early_init`): 0 in a
/// program `ld.so` did not start (DL-17; a static program, ruling #5005).
pub static LOADER_DLCLOSE: AtomicUsize = AtomicUsize::new(0);

export! { "GLIBC_2.34";
/// `dlclose(handle)` (§27.1, s27_1_c_4, dlclose(3); phase 14 DL-17): in a program `ld.so` started,
/// the loader drops one open of `handle` and unloads what is no longer used
/// (`crates/boundary/src/ld/dlopen.rs`): 0. NULL, a pointer that is not a handle, or a handle with no
/// open left: -1 with the text `dlclose: invalid handle` (D-27.32, ruling #5006), never
/// a crash. In a static program no handle exists (ruling #5005): the same failure.
pub extern "C" fn dlclose(handle: *mut c_void) -> c_int {
    let f = LOADER_DLCLOSE.load(Ordering::Acquire);
    if f == 0 {
        crate::dlerr::set(&[b"dlclose: invalid handle"]);
        return -1;
    }
    let w = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
    let r = crate::ffi::liella_machine_call3(f, w(handle.expose_provenance()), 0, 0);
    if r == 0 { 0 } else { -1 }
}}

export! { "GLIBC_2.34";
/// `dlerror()` (§27.1, dlerror(3)): the calling thread's pending text from its last failed `dl*`
/// call, once (the next call returns NULL until another failure); NULL when none is pending. A
/// successful call clears a pending text (#5127). The string stays valid until the thread's next
/// failure or its exit (`crate::dlerr`).
pub extern "C" fn dlerror() -> *mut c_char {
    core::ptr::with_exposed_provenance_mut(crate::dlerr::take())
}}
