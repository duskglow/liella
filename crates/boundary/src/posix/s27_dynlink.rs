//! Translation for spec §27: dynamic linking.
//!
//! Implementation phase: 14 (implementation-plan.md §6). Not yet implemented.
//!
//! `dl_iterate_phdr` (D-27.4): a static program has two objects, as glibc reports them: the
//! main program (name "", `AT_PHDR`/`AT_PHNUM`, bias from `PT_PHDR`) and the vDSO
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

use crate::{conv, export, process, tb};
use crate::sys::CSys;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use liella_abi::ctypes::{c_int, c_uint, c_void};
use liella_rt::cpu::{feature_leaves, FEATURE_LEAVES};
use liella_abi::layout::{struct_dl_find_object, struct_dl_phdr_info};
use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::{AT_PHDR, AT_PHNUM, AT_SYSINFO_EHDR};

type Callback = Option<extern "C" fn(&struct_dl_phdr_info, usize, *mut c_void) -> c_int>;

const PT_LOAD: u32 = 1;
const PT_TLS: u32 = 7;
const PT_PHDR: u32 = 6;
const PT_GNU_EH_FRAME: u32 = 0x6474_e550;

fn word(w: &[u64], i: usize) -> usize {
    w.get(i).map_or(0, |&v| usize::try_from(v).unwrap_or(0))
}

/// Bias and TLS segment (memsz, align) of the headers at `phdr`.
fn scan(phdr: usize, phnum: usize, first_load_bias_from: Option<usize>) -> (usize, Option<(usize, usize)>) {
    // UNSAFE(slice): program headers the kernel mapped (⚑ KERN) and that stay mapped.
    #[allow(unsafe_code)]
    // SAFETY: `AT_PHDR`/`AT_PHNUM` (or the vDSO image's e_phoff/e_phnum) describe mapped headers.
    let w = unsafe { conv::words_at(phdr, phnum.saturating_mul(7)) };
    let hs = || w.chunks_exact(7).take(phnum);
    let ty = |h: &[u64]| u32::try_from(h.first().copied().unwrap_or(0) & 0xffff_ffff).unwrap_or(0);
    let bias = match first_load_bias_from {
        Some(base) => hs().find(|h| ty(h) == PT_LOAD).map_or(0, |h| base.wrapping_sub(word(h, 2))),
        None => hs().find(|h| ty(h) == PT_PHDR).map_or(0, |h| phdr.wrapping_sub(word(h, 2))),
    };
    let tls = hs().find(|h| ty(h) == PT_TLS).map(|h| (word(h, 5), word(h, 6)));
    (bias, tls)
}

export! { "GLIBC_2.2.5";
pub extern "C" fn dl_iterate_phdr(cb: Callback, data: *mut c_void) -> c_int {
    let Some(cb) = cb else { return 0 };
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
fn find_in(phdr: usize, phnum: usize, bias: usize, pc: usize, whole: bool) -> Option<(usize, usize, usize)> {
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
        // UNSAFE(slice): the vDSO's ELF64 header (64 bytes), mapped by the kernel.
        #[allow(unsafe_code)]
        // SAFETY: `AT_SYSINFO_EHDR` is the kernel's own vDSO image (KERN).
        let h = unsafe { conv::words_at(vdso, 8) };
        let vphdr = vdso.wrapping_add(word(h, 4));
        let vphnum = usize::try_from(h.get(7).copied().unwrap_or(0) & 0xffff).unwrap_or(0);
        found = find_in(vphdr, vphnum, scan(vphdr, vphnum, Some(vdso)).0, pc, true);
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
