//! Static TLS of the initially loaded objects (phase 14 DL-12, plan/ph14/design.md §7, R14-4,
//! D-27.20): module IDs, the static layout with its surplus, the initial thread area (thread block,
//! static blocks, dynamic thread vector) and the lookup `__tls_get_addr` makes.
//!
//! Pure logic over the [`Arch`] table: the variant, the TCB size and alignment come from `A`, never
//! from a formula written for one architecture (design §0a checklist item 3). The block offsets follow
//! Drepper, "ELF Handling For Thread-Local Storage" (§3.4.1 variant I, §3.4.2 variant II), extended so
//! that a block whose `p_vaddr` is not a multiple of its alignment keeps that remainder at run time.
//!
//! **The dtv** (Liella's own layout, read only by Liella's `ld.so`): word 0 the generation the vector
//! was built for, word 1 the number of module slots, word `1 + m` the address of module `m`'s block
//! (0: not allocated in this thread). Module IDs start at 1, in load order over the objects that have
//! a `PT_TLS` segment.

use crate::arch::{Arch, TlsVariant};
use crate::elf::ProgramHeader;

/// The static TLS surplus (R14-4): 64 KiB of address space after the initial modules' blocks, for
/// objects loaded later that need static TLS. Anonymous memory: no page is touched until used.
pub const SURPLUS: u64 = 64 * 1024;

/// The dtv words before the first module slot (generation, slot count).
pub const DTV_HEADER: u64 = 2;

/// One module's `PT_TLS` segment (link-time values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Module {
    /// `p_vaddr`: the initialisation image's link-time address.
    pub vaddr: u64,
    /// `p_filesz`: initialised bytes (`.tdata`).
    pub filesz: u64,
    /// `p_memsz`: all bytes (`.tdata` + `.tbss`).
    pub memsz: u64,
    /// `p_align` (0 and 1 mean none).
    pub align: u64,
}

impl Module {
    /// The module of a `PT_TLS` program header.
    pub fn of(ph: &ProgramHeader) -> Self {
        Self { vaddr: ph.vaddr, filesz: ph.filesz, memsz: ph.memsz, align: ph.align }
    }
}

/// Where one module's static block is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    /// The module ID (1 onwards).
    pub module: u64,
    /// The block's address minus the thread pointer.
    pub tp_offset: i64,
}

/// The initial thread area: everything at byte offsets from an area base aligned to [`Area::align`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Area {
    /// The bytes the area needs from an aligned base.
    pub len: u64,
    /// The alignment the base must have (the largest module alignment and `A::TCB_ALIGN`).
    pub align: u64,
    /// The thread pointer's offset in the area.
    pub tp: u64,
    /// The dtv's offset in the area (8-byte aligned).
    pub dtv: u64,
    /// The dtv's length in words ([`DTV_HEADER`] + the number of modules).
    pub dtv_words: u64,
    /// The number of modules (the highest module ID).
    pub modules: u64,
}

/// Why no layout was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsError {
    /// Object `n`'s `PT_TLS` alignment is not a power of two.
    Align(usize),
    /// Object `n`'s `PT_TLS` has more file bytes than memory bytes.
    Size(usize),
    /// The sizes overflow the address space.
    Overflow,
    /// `out` is shorter than the object list, or the thread block is smaller than `A::TCB_SIZE`.
    Args,
}

fn up(v: u64, align: u64) -> Option<u64> {
    v.checked_next_multiple_of(align)
}

/// Lay out the static TLS of `mods` (one entry per object, in load order; `None`: no `PT_TLS`) with
/// `surplus` bytes left for later objects. `tail` is the thread block's size from the thread pointer
/// (at least `A::TCB_SIZE`). Writes each object's placement to `out` (same indices) and returns the
/// area.
///
/// # Errors
///
/// [`TlsError`] as documented there.
pub fn layout<A: Arch>(mods: &[Option<Module>], out: &mut [Option<Placed>], tail: u64, surplus: u64) -> Result<Area, TlsError> {
    if out.len() < mods.len() || tail < A::TCB_SIZE {
        return Err(TlsError::Args);
    }
    let mut max = A::TCB_ALIGN.max(1);
    let mut id = 0u64;
    // Variant II: `off` is the distance below TP reached so far. Variant I: the distance above TP.
    let mut off = match A::TLS_VARIANT {
        TlsVariant::II => 0,
        TlsVariant::I => tail,
    };
    for (n, (m, slot)) in mods.iter().zip(out.iter_mut()).enumerate() {
        *slot = None;
        let Some(m) = m else { continue };
        let align = m.align.max(1);
        if !align.is_power_of_two() {
            return Err(TlsError::Align(n));
        }
        if m.filesz > m.memsz {
            return Err(TlsError::Size(n));
        }
        max = max.max(align);
        let r = m.vaddr % align;
        id = id.checked_add(1).ok_or(TlsError::Overflow)?;
        let at = match A::TLS_VARIANT {
            TlsVariant::II => {
                // The block starts at TP - off with (TP - off) % align == r.
                off = up(off.checked_add(m.memsz).and_then(|v| v.checked_add(r)).ok_or(TlsError::Overflow)?, align).ok_or(TlsError::Overflow)? - r;
                0i64.checked_sub_unsigned(off).ok_or(TlsError::Overflow)?
            }
            TlsVariant::I => {
                let start = up(off.checked_sub(r).ok_or(TlsError::Overflow)?, align).ok_or(TlsError::Overflow)?.checked_add(r).ok_or(TlsError::Overflow)?;
                let start = if start < off { start.checked_add(align).ok_or(TlsError::Overflow)? } else { start };
                off = start.checked_add(m.memsz).ok_or(TlsError::Overflow)?;
                i64::try_from(start).map_err(|_| TlsError::Overflow)?
            }
        };
        *slot = Some(Placed { module: id, tp_offset: at });
    }
    let off = off.checked_add(surplus).ok_or(TlsError::Overflow)?;
    let dtv_words = DTV_HEADER.checked_add(id).ok_or(TlsError::Overflow)?;
    let (tp, end) = match A::TLS_VARIANT {
        TlsVariant::II => {
            let tp = up(off, max).ok_or(TlsError::Overflow)?;
            (tp, tp.checked_add(tail).ok_or(TlsError::Overflow)?)
        }
        TlsVariant::I => (0, off),
    };
    let dtv = up(end, 8).ok_or(TlsError::Overflow)?;
    let len = dtv.checked_add(dtv_words.checked_mul(8).ok_or(TlsError::Overflow)?).ok_or(TlsError::Overflow)?;
    Ok(Area { len, align: max, tp, dtv, dtv_words, modules: id })
}

/// The dtv of a thread whose thread pointer is `tp`: generation `generation`, a slot per module of
/// `placed` (indices as given to [`layout`]) pointing at its static block. `dtv` holds
/// `area.dtv_words` words.
pub fn fill_dtv(dtv: &mut [u64], generation: u64, tp: u64, placed: &[Option<Placed>]) {
    let mut count = 0u64;
    for p in placed.iter().flatten() {
        let i = usize::try_from(DTV_HEADER.saturating_add(p.module).saturating_sub(1)).unwrap_or(usize::MAX);
        if let Some(w) = dtv.get_mut(i) {
            *w = tp.wrapping_add_signed(p.tp_offset);
        }
        count = count.max(p.module);
    }
    if let [g, n, ..] = dtv {
        *g = generation;
        *n = count;
    }
}

/// `__tls_get_addr`'s lookup: the address of `offset` in module `module`'s block, through the dtv
/// whose word `i` is `word(i)`, current only when it was built for `generation`. `None`: the slow path
/// (a stale vector, an unknown module, or a block this thread has not allocated).
pub fn lookup<A: Arch>(word: impl Fn(u64) -> u64, generation: u64, module: u64, offset: u64) -> Option<u64> {
    if word(0) != generation || module == 0 || module > word(1) {
        return None;
    }
    let block = word(DTV_HEADER.wrapping_add(module).wrapping_sub(1));
    (block != 0).then(|| block.wrapping_add(offset).wrapping_add(A::DTP_BIAS))
}

/// Whether module `m`'s initialisation image (`p_filesz` bytes at `p_vaddr`) lies in the file part of
/// one of `elf`'s `PT_LOAD` segments: the loader copies it from the loaded object, so an image
/// anywhere else is refused rather than read.
pub fn image_in_load(elf: &crate::elf::Elf<'_>, m: &Module) -> bool {
    let Some(end) = m.vaddr.checked_add(m.filesz) else { return false };
    elf.program_headers().any(|p| p.p_type == crate::elf::pt::LOAD && p.vaddr <= m.vaddr && end <= p.vaddr.saturating_add(p.filesz))
}
