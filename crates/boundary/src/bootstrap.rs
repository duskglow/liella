//! The self-relocator's memory side (phase 14 DL-4, design §1, ruling R14-1, D-27.10): the
//! program headers, dynamic array, relocation tables and writable segment of the running image
//! become slices (category *slice*), and `liella_ld::boot` decides everything. First user: the
//! static-PIE start of `libc.a` (`runtime::start`); `ld.so` reuses it from DL-5.
//!
//! Runs before the image is relocated: nothing here may read a pointer stored in the image's data
//! (no `&'static` with addresses, no trait objects, no panics), and every call is a direct one.

use liella_ld::arch::Native;
use liella_ld::boot::{self, BootError, Segment};

use crate::conv;
use crate::ffi;

/// What pass 2 needs: the bias, the writable segment and the two RELA tables (run-time addresses).
#[derive(Clone, Copy)]
pub(crate) struct Relocated {
    /// Load address minus link address.
    pub bias: usize,
    writable: (u64, u64),
    rela: (usize, usize),
    jmprel: (usize, usize),
}

/// A table's run-time address and length, from its link-time `(addr, len)`.
fn at(bias: usize, (addr, len): (u64, u64)) -> (usize, usize) {
    (usize::try_from(addr).unwrap_or(0).wrapping_add(bias), usize::try_from(len).unwrap_or(0))
}

/// The writable segment as a mutable byte view, for one relocation step.
fn segment(bias: usize, (vaddr, memsz): (u64, u64)) -> Segment<'static> {
    let (addr, len) = at(bias, (vaddr, memsz));
    // UNSAFE(slice): the image's own writable PT_LOAD segment.
    #[allow(unsafe_code)]
    // SAFETY: the kernel mapped the segment readable and writable for `memsz` bytes at
    // `bias + vaddr` (⚑ KERN, execve), and during relocation the thread holds no reference into
    // it: the view lives for one relocation step and nothing else runs meanwhile.
    let bytes = unsafe { conv::bytes_at_mut(addr, len) };
    Segment { bytes, vaddr }
}

/// The run-time address of the executable's ELF header when the executable is a static PIE
/// (`ET_DYN`), else `None`. Taken PC-relatively (`adapter/image_anchor.c`), so it is right before
/// the image is relocated.
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
    let t = boot::tables(words, img.writable)?;
    let (rela, jmprel, relr) = (at(bias, t.rela), at(bias, t.jmprel), at(bias, t.relr));
    // UNSAFE(slice): the three relocation tables of the mapped image.
    #[allow(unsafe_code)]
    // SAFETY: `boot::tables` checked each table holds whole entries, lies in the address space
    // and does not overlap the writable segment, so these reads never alias the view below; the
    // linker places them in a read-only segment of the image, mapped by the kernel.
    let (ra, ja, rr) = unsafe { (conv::bytes_at(rela.0, rela.1), conv::bytes_at(jmprel.0, jmprel.1), conv::bytes_at(relr.0, relr.1)) };
    let mut seg = segment(bias, img.writable);
    boot::relocate::<Native>(ra, ja, rr, &mut seg, u64::try_from(bias).unwrap_or(0))?;
    Ok(Relocated { bias, writable: img.writable, rela, jmprel })
}

/// Pass 2: each IRELATIVE entry's resolver (bias + addend) is called and its result stored. Runs
/// after pass 1 and after the CPU paths are chosen, before the RELRO segment is protected.
pub(crate) fn irelative(r: &Relocated) -> Result<(), BootError> {
    for (addr, len) in [r.rela, r.jmprel] {
        // UNSAFE(slice): a relocation table of the image (as in `relocate_self`).
        #[allow(unsafe_code)]
        // SAFETY: checked by `boot::tables` in pass 1; outside the writable segment.
        let table = unsafe { conv::bytes_at(addr, len) };
        for (offset, addend) in boot::irelative::<Native>(table) {
            let resolver = usize::try_from(addend).unwrap_or(0).wrapping_add(r.bias);
            let value = ffi::liella_machine_call0(resolver);
            // The view is made after the resolver returns: the resolver may use the segment.
            segment(r.bias, r.writable).store(offset, u64::from_ne_bytes(value.to_ne_bytes()))?;
        }
    }
    Ok(())
}
