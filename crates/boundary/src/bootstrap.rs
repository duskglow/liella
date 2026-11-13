//! The self-relocator's memory side (phase 14 DL-4, design §1, ruling R14-1, D-27.10): the
//! program headers, dynamic array, relocation tables and writable segments of the running image
//! become slices (category *slice*), and `liella_ld::boot` decides everything. First user: the
//! static-PIE start of `libc.a` (`runtime::start`); `ld.so` reuses it from DL-5.
//!
//! Runs before the image is relocated: nothing here may read a pointer stored in the image's data
//! (no `&'static` with addresses, no trait objects, no panics), and every call is a direct one.

use liella_ld::arch::Native;
use liella_ld::boot::{self, BootError, Memory, Writer};

use crate::conv;
use crate::ffi;

/// What pass 2 needs: the bias, the program headers (for the writable segments) and the two RELA
/// tables (run-time addresses).
#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "runtime"), allow(dead_code))]
pub(crate) struct Relocated {
    /// Load address minus link address.
    pub bias: usize,
    headers: &'static [u64],
    phnum: usize,
    rela: (usize, usize),
    jmprel: (usize, usize),
}

/// A table's run-time address and length, from its link-time `(addr, len)`.
fn at(bias: usize, (addr, len): (u64, u64)) -> (usize, usize) {
    (usize::try_from(addr).unwrap_or(0).wrapping_add(bias), usize::try_from(len).unwrap_or(0))
}

/// A relocation table of the mapped image (run-time address and length), as bytes.
fn table((addr, len): (usize, usize)) -> &'static [u8] {
    // UNSAFE(slice): one relocation table of the mapped image.
    #[allow(unsafe_code)]
    // SAFETY: `boot::tables` checked the table holds whole entries, lies in the address space
    // and does not overlap the writable segment, so the read never aliases a segment view; the
    // linker places it in a read-only segment of the image, mapped by the kernel.
    unsafe { conv::bytes_at(addr, len) }
}

/// The mapped image's writable segments, loaded `.0` bytes from their link-time addresses: each
/// view is made for one store, for the segment `boot::Writer` found the target in (D-27.25).
struct Mapped(usize);

impl Memory for Mapped {
    fn segment(&mut self, vaddr: u64, memsz: u64) -> &mut [u8] {
        let (addr, len) = at(self.0, (vaddr, memsz));
        // UNSAFE(slice): one of the image's own writable PT_LOAD segments.
        #[allow(unsafe_code)]
        // SAFETY: `boot::Writer` asks only for a writable `PT_LOAD` of the image's headers, which
        // the kernel (⚑ KERN, execve) or `ld.so`'s mapper mapped readable and writable for `memsz`
        // bytes at `bias + vaddr`; the view lives for one store, the thread holds no other
        // reference into the segment meanwhile, and nothing else runs.
        unsafe { conv::bytes_at_mut(addr, len) }
    }
}

/// The run-time address of the executable's ELF header when the executable is a static PIE
/// (`ET_DYN`), else `None`. Taken PC-relatively (`adapter/image_anchor.c`), so it is right before
/// the image is relocated.
#[cfg_attr(not(feature = "runtime"), allow(dead_code))]
pub(crate) fn static_pie_header() -> Option<usize> {
    let ehdr = ffi::liella_link_image_header();
    // UNSAFE(slice): the executable's ELF header, mapped with its first segment.
    #[allow(unsafe_code)]
    // SAFETY: the linker defines `__ehdr_start` only where the ELF header lies in a loaded
    // segment, so `EHDR_SIZE` bytes from it are mapped readable for the process's lifetime.
    let bytes = unsafe { conv::bytes_at(ehdr, liella_ld::elf::EHDR_SIZE) };
    boot::position_independent(bytes).then_some(ehdr)
}

/// Pass 1 of a static PIE (`ehdr` = run-time address of its ELF header, from
/// [`static_pie_header`]): RELATIVE (RELA) then RELR. `phdr`/`phnum` are `AT_PHDR`/`AT_PHNUM`.
pub(crate) fn relocate_self(phdr: usize, phnum: usize, ehdr: usize) -> Result<Relocated, BootError> {
    // UNSAFE(slice): the program headers the kernel mapped with the executable.
    #[allow(unsafe_code)]
    // SAFETY: `AT_PHDR`/`AT_PHNUM` describe the loaded headers (⚑ KERN), which stay mapped.
    let headers = unsafe { conv::words_at(phdr, phnum.saturating_mul(7)) };
    let img = boot::image(headers, phnum)?;
    let bias = ehdr.wrapping_sub(usize::try_from(img.header).unwrap_or(0));
    let dynamic = bias.wrapping_add(usize::try_from(img.dynamic.0).unwrap_or(0));
    // UNSAFE(slice): the image's dynamic array (`PT_DYNAMIC`), read before any write.
    #[allow(unsafe_code)]
    // SAFETY: `dynamic` is the run-time address (bias + `p_vaddr`) of `PT_DYNAMIC`'s `p_memsz`
    // bytes in the mapped image; the array is only read, and dropped before the segment view
    // below is made.
    let words = unsafe { conv::words_at(dynamic, usize::try_from(img.dynamic.1).unwrap_or(0) / 8) };
    let t = boot::tables(words, &img)?;
    let (rela, jmprel, relr) = (at(bias, t.rela), at(bias, t.jmprel), at(bias, t.relr));
    let (ra, ja, rr) = (table(rela), table(jmprel), table(relr));
    boot::relocate::<Native, _>(ra, ja, rr, &mut Writer::new(img, Mapped(bias)), u64::try_from(bias).unwrap_or(0))?;
    Ok(Relocated { bias, headers, phnum, rela, jmprel })
}

/// `ld.so`'s pass 1 (DL-5): its own ELF header (run-time address `ehdr` of `__ehdr_start`, taken
/// PC-relatively by `liella_machine_ld_entry`) names its program headers. Its link check (L14-1) leaves it no
/// IRELATIVE entry, so there is no pass 2.
#[cfg(feature = "ld")]
#[inline(never)] // `cargo xtask so` finds the end of the unrelocated code at its call (D-27.13).
pub(crate) fn relocate_image(ehdr: usize) -> Result<Relocated, BootError> {
    // UNSAFE(slice): the image's ELF header, mapped with its first segment.
    #[allow(unsafe_code)]
    // SAFETY: as in `static_pie_header`: the linker defines `__ehdr_start` only where the ELF
    // header lies in a loaded segment, mapped readable for the process's lifetime.
    let bytes = unsafe { conv::bytes_at(ehdr, liella_ld::elf::EHDR_SIZE) };
    let (phoff, phnum) = boot::own_headers(bytes).ok_or(BootError::Segments)?;
    // The headers lie in the first loaded segment, which maps file offset 0 (the shared-object
    // layout of GNU ld and lld), so `ehdr + e_phoff` is their run-time address.
    relocate_self(ehdr.wrapping_add(usize::try_from(phoff).unwrap_or(0)), phnum, ehdr)
}

/// Pass 2: each IRELATIVE entry's resolver (bias + addend) is called and its result stored. Runs
/// after pass 1 and after the CPU paths are chosen, before the RELRO segment is protected.
#[cfg_attr(not(feature = "runtime"), allow(dead_code))]
pub(crate) fn irelative(r: &Relocated) -> Result<(), BootError> {
    // The writer holds no view between stores: each is made after its resolver returns.
    let mut writer = Writer::new(boot::image(r.headers, r.phnum)?, Mapped(r.bias));
    for (addr, len) in [r.rela, r.jmprel] {
        // UNSAFE(slice): a relocation table of the image (as in `relocate_self`).
        #[allow(unsafe_code)]
        // SAFETY: checked by `boot::tables` in pass 1; outside every writable segment.
        let table = unsafe { conv::bytes_at(addr, len) };
        for (offset, addend) in boot::irelative::<Native>(table) {
            let resolver = usize::try_from(addend).unwrap_or(0).wrapping_add(r.bias);
            let value = ffi::liella_machine_call0(resolver);
            // The view is made after the resolver returns: the resolver may use the segment.
            writer.store(offset, u64::from_ne_bytes(value.to_ne_bytes()))?;
        }
    }
    Ok(())
}
