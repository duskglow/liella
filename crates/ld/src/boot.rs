//! The self-relocator's logic (phase 14 DL-4, design §1, ruling R14-1, D-27.10): what an image
//! that relocates itself (a static PIE today, `ld.so` from DL-5) must do before any of its
//! relocated data is used. The memory side (the slices over the image) is
//! `liella_boundary::bootstrap`; everything here is pure and testable on the host.
//!
//! Rules (D-27.10):
//! * Inputs are the image's program headers and its dynamic array, read in place: one `PT_DYNAMIC`
//!   and exactly one writable `PT_LOAD`; every relocation target lies inside that segment, and the
//!   relocation tables lie outside it (the relocator never reads what it writes).
//! * The image relocates itself only when its ELF header says `ET_DYN` ([`position_independent`]).
//!   The bias is the run-time address of that header (taken PC-relatively by the caller, never
//!   through a GOT slot, which is unrelocated) minus the `p_vaddr` of the `PT_LOAD` at file
//!   offset 0 ([`Image::header`]); every other address follows from the headers and the bias.
//! * Pass 1 ([`relocate`]): the RELA entries of `DT_RELA` then `DT_JMPREL` whose kind
//!   ([`Arch::classify`]) is [`RelocKind::Relative`] (word = bias + addend), then every RELR address
//!   (word += bias). [`RelocKind::Irelative`] and [`RelocKind::None`] are left for pass 2; any other
//!   kind is an error (the image needs a loader, not a self-relocator).
//! * Pass 2 ([`irelative`]) is the caller's, after pass 1 and after the CPU features are known: each
//!   `Irelative` entry's resolver (bias + addend) is called and its result stored at the target.
//! * `DT_TEXTREL`, an entry size other than 24 (RELA) or 8 (RELR), `DT_PLTREL` other than
//!   `DT_RELA`, a table size that is not a whole number of entries, a RELR bitmap before any address,
//!   or an address overflow is an error. Nothing panics: the caller runs before its own panic
//!   machinery (whose `Location` data is relocated) can be used.

use crate::arch::{Arch, RelocKind};
use crate::elf::{dt, et, pt, RELA_SIZE, RELR_SIZE};

/// `DT_TEXTREL` (ELF gABI): relocations against non-writable segments.
const DT_TEXTREL: u64 = 22;
/// Words per ELF64 program header.
const PHDR_WORDS: usize = 7;
/// `PF_W` in `p_flags`.
const PF_W: u64 = 2;
/// `DT_RELAENT` and `DT_RELRENT` as the dynamic array states them.
const RELA_ENT: u64 = 24;
const RELR_ENT: u64 = 8;
/// Addresses one RELR bitmap word covers (63 words).
const RELR_SPAN: u64 = 63 * 8;

/// Why an image cannot relocate itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootError {
    /// No `PT_DYNAMIC`, or none or more than one writable `PT_LOAD`.
    Segments,
    /// The dynamic array has no `DT_NULL`.
    NoNull,
    /// A dynamic entry with a value the relocator does not accept; the tag.
    BadDynamic(u64),
    /// A relocation table outside the image's address space or overlapping the writable segment;
    /// the table's dynamic tag.
    Table(u64),
    /// A relocation of a kind a self-relocator cannot apply; its raw type (for the message only).
    Unsupported(u32),
    /// A relocation target outside the writable segment; its link-time address.
    Outside(u64),
    /// A RELR bitmap word with no address word before it.
    RelrNoBase,
}

/// The parts of the image the relocator works on, from its program headers (link-time addresses).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Image {
    /// The link-time address of the ELF header: `p_vaddr` of the `PT_LOAD` that maps file offset 0.
    /// The caller's run-time address of the header minus this is the bias.
    pub header: u64,
    /// `PT_DYNAMIC`: address and size in bytes.
    pub dynamic: (u64, u64),
    /// The writable `PT_LOAD`: address and size in memory.
    pub writable: (u64, u64),
}

/// Whether the ELF header `ehdr` (its first bytes, as mapped) describes a position-independent
/// image (`e_type` `ET_DYN`): only such a static program relocates itself (D-27.10).
pub fn position_independent(ehdr: &[u8]) -> bool {
    matches!(ehdr.get(16..18), Some(&[a, b]) if u16::from_ne_bytes([a, b]) == et::DYN)
}

/// Where an image's own program headers are, from its ELF header (`ehdr`, its first bytes as
/// mapped): `(e_phoff, e_phnum)`. `ld.so` finds its headers this way (`AT_PHDR` names the
/// program's when the kernel starts it as an interpreter). `None` unless `e_phentsize` is the
/// 64-bit header size.
pub fn own_headers(ehdr: &[u8]) -> Option<(u64, usize)> {
    let phoff = u64::from_ne_bytes(ehdr.get(32..40)?.try_into().ok()?);
    let entsize = u16::from_ne_bytes(ehdr.get(54..56)?.try_into().ok()?);
    let phnum = u16::from_ne_bytes(ehdr.get(56..58)?.try_into().ok()?);
    (usize::from(entsize) == crate::elf::PHDR_SIZE).then_some((phoff, usize::from(phnum)))
}

/// The image's program headers (`phnum` headers as words, as the kernel mapped them): where its
/// ELF header is linked, its dynamic array and its one writable segment.
pub fn image(words: &[u64], phnum: usize) -> Result<Image, BootError> {
    let (mut header, mut dynamic, mut writable, mut n_writable) = (None, None, None, 0usize);
    for h in words.chunks_exact(PHDR_WORDS).take(phnum) {
        let (w0, vaddr, memsz) = (h.first().copied().unwrap_or(0), h.get(2).copied().unwrap_or(0), h.get(5).copied().unwrap_or(0));
        let ty = u32::try_from(w0 & 0xffff_ffff).unwrap_or(0);
        if ty == pt::LOAD && h.get(1) == Some(&0) && header.is_none() {
            header = Some(vaddr);
        }
        if ty == pt::DYNAMIC && dynamic.is_none() {
            dynamic = Some((vaddr, memsz));
        } else if ty == pt::LOAD && (w0 >> 32) & PF_W != 0 {
            n_writable = n_writable.saturating_add(1);
            writable = Some((vaddr, memsz));
        }
    }
    match (header, dynamic, writable) {
        (Some(header), Some(dynamic), Some(writable)) if n_writable == 1 => Ok(Image { header, dynamic, writable }),
        _ => Err(BootError::Segments),
    }
}

/// The relocation tables the dynamic array names: link-time address and size in bytes, (0, 0) when
/// absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tables {
    /// `DT_RELA` / `DT_RELASZ`.
    pub rela: (u64, u64),
    /// `DT_JMPREL` / `DT_PLTRELSZ` (RELA entries).
    pub jmprel: (u64, u64),
    /// `DT_RELR` / `DT_RELRSZ`.
    pub relr: (u64, u64),
}

/// The relocation tables of the dynamic array `dynamic` (tag/value word pairs up to `DT_NULL`),
/// checked against the writable segment `writable` (link-time address and size): each table holds
/// whole entries, lies in the address space and does not overlap the segment.
pub fn tables(dynamic: &[u64], writable: (u64, u64)) -> Result<Tables, BootError> {
    let mut t = Tables::default();
    let mut ended = false;
    for e in dynamic.chunks_exact(2) {
        let (tag, val) = (e.first().copied().unwrap_or(0), e.get(1).copied().unwrap_or(0));
        match tag {
            dt::NULL => {
                ended = true;
                break;
            }
            dt::RELA => t.rela.0 = val,
            dt::RELASZ => t.rela.1 = val,
            dt::JMPREL => t.jmprel.0 = val,
            dt::PLTRELSZ => t.jmprel.1 = val,
            dt::RELR => t.relr.0 = val,
            dt::RELRSZ => t.relr.1 = val,
            dt::RELAENT if val != RELA_ENT => return Err(BootError::BadDynamic(tag)),
            dt::RELRENT if val != RELR_ENT => return Err(BootError::BadDynamic(tag)),
            dt::PLTREL if val != dt::RELA => return Err(BootError::BadDynamic(tag)),
            DT_TEXTREL => return Err(BootError::BadDynamic(tag)),
            _ => {}
        }
    }
    if !ended {
        return Err(BootError::NoNull);
    }
    let w_end = writable.0.checked_add(writable.1).ok_or(BootError::Segments)?;
    for (tag, (addr, len), ent) in [(dt::RELA, t.rela, RELA_ENT), (dt::JMPREL, t.jmprel, RELA_ENT), (dt::RELR, t.relr, RELR_ENT)] {
        if len == 0 {
            continue;
        }
        let end = addr.checked_add(len).ok_or(BootError::Table(tag))?;
        if len.checked_rem(ent) != Some(0) || (addr < w_end && end > writable.0) {
            return Err(BootError::Table(tag));
        }
    }
    Ok(t)
}

/// The writable segment as the relocator sees it: its bytes in memory and the link-time address of
/// the first one.
#[derive(Debug)]
pub struct Segment<'a> {
    /// The segment's bytes (its whole memory size).
    pub bytes: &'a mut [u8],
    /// The link-time address of `bytes[0]`.
    pub vaddr: u64,
}

impl Segment<'_> {
    /// The 8 bytes at link-time address `at`, if they lie inside the segment.
    fn word(&mut self, at: u64) -> Result<&mut [u8; 8], BootError> {
        let off = at.checked_sub(self.vaddr).and_then(|o| usize::try_from(o).ok()).ok_or(BootError::Outside(at))?;
        let end = off.checked_add(8).ok_or(BootError::Outside(at))?;
        self.bytes.get_mut(off..end).and_then(|b| <&mut [u8; 8]>::try_from(b).ok()).ok_or(BootError::Outside(at))
    }

    /// Store `value` at link-time address `at` (in the image's byte order: native, as it runs).
    pub fn store(&mut self, at: u64, value: u64) -> Result<(), BootError> {
        *self.word(at)? = value.to_ne_bytes();
        Ok(())
    }

    /// Add `bias` to the word at link-time address `at` (a RELR address: the addend is in place).
    fn add(&mut self, at: u64, bias: u64) -> Result<(), BootError> {
        let w = self.word(at)?;
        *w = u64::from_ne_bytes(*w).wrapping_add(bias).to_ne_bytes();
        Ok(())
    }
}

/// One RELA entry: (`r_offset`, `r_type`, `r_addend`).
fn rela_entry(e: &[u8]) -> (u64, u32, u64) {
    let word = |i: usize| e.get(i..i.saturating_add(8)).and_then(|b| <[u8; 8]>::try_from(b).ok()).map_or(0, u64::from_ne_bytes);
    (word(0), u32::try_from(word(8) & 0xffff_ffff).unwrap_or(0), word(16))
}

/// Pass 1 over one RELA table: apply its `Relative` entries, leave `Irelative`/`None`, refuse others.
fn relative<A: Arch>(table: &[u8], seg: &mut Segment<'_>, bias: u64) -> Result<(), BootError> {
    for e in table.chunks_exact(RELA_SIZE) {
        let (offset, r_type, addend) = rela_entry(e);
        match A::classify(r_type) {
            RelocKind::Relative => seg.store(offset, bias.wrapping_add(addend))?,
            RelocKind::Irelative | RelocKind::None => {}
            _ => return Err(BootError::Unsupported(r_type)),
        }
    }
    Ok(())
}

/// Pass 1 over the RELR table `table` (8-byte words): an even word is an address to relocate and
/// starts a run; an odd word is a bitmap of the 63 words after the run's last address.
fn relr(table: &[u8], seg: &mut Segment<'_>, bias: u64) -> Result<(), BootError> {
    let mut base: Option<u64> = None;
    for w in table.chunks_exact(RELR_SIZE) {
        let w = <[u8; 8]>::try_from(w).map_or(0, u64::from_ne_bytes);
        if w & 1 == 0 {
            seg.add(w, bias)?;
            base = Some(w.checked_add(8).ok_or(BootError::Outside(w))?);
        } else {
            let start = base.ok_or(BootError::RelrNoBase)?;
            let (mut bits, mut at) = (w >> 1, start);
            while bits != 0 {
                if bits & 1 != 0 {
                    seg.add(at, bias)?;
                }
                bits >>= 1;
                at = at.checked_add(8).ok_or(BootError::Outside(at))?;
            }
            base = Some(start.checked_add(RELR_SPAN).ok_or(BootError::Outside(start))?);
        }
    }
    Ok(())
}

/// Pass 1: the `Relative` entries of `rela` then `jmprel`, then every RELR address, each word in
/// `seg` adjusted by `bias` (load address minus link address).
pub fn relocate<A: Arch>(rela: &[u8], jmprel: &[u8], relr_table: &[u8], seg: &mut Segment<'_>, bias: u64) -> Result<(), BootError> {
    relative::<A>(rela, seg, bias)?;
    relative::<A>(jmprel, seg, bias)?;
    relr(relr_table, seg, bias)
}

/// Pass 2's work list: the (`r_offset`, `r_addend`) of each `Irelative` entry of `table`, in order.
pub fn irelative<A: Arch>(table: &[u8]) -> impl Iterator<Item = (u64, u64)> + '_ {
    table.chunks_exact(RELA_SIZE).map(rela_entry).filter(|&(_, t, _)| A::classify(t) == RelocKind::Irelative).map(|(o, _, a)| (o, a))
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    //! Design §1 / D-27.10 (no spec clause: the relocator is internal to start-up). Types are
    //! x86_64 numbers (8 RELATIVE, 37 IRELATIVE, 6 GLOB_DAT), so these run with `X86_64`.
    use super::*;
    use crate::arch::X86_64;

    fn rela(entries: &[(u64, u64, u64)]) -> alloc::vec::Vec<u8> {
        entries.iter().flat_map(|&(o, t, a)| [o, t, a]).flat_map(u64::to_ne_bytes).collect()
    }
    fn words(ws: &[u64]) -> alloc::vec::Vec<u8> {
        ws.iter().copied().flat_map(u64::to_ne_bytes).collect()
    }
    fn at(seg: &[u8], i: usize) -> u64 {
        u64::from_ne_bytes(seg[i * 8..i * 8 + 8].try_into().unwrap())
    }

    #[test]
    fn relative_relr_and_irelative_split() {
        let mut mem = words(&[0; 80]);
        mem[16..24].copy_from_slice(&0x55u64.to_ne_bytes());
        let mut seg = Segment { bytes: &mut mem, vaddr: 0x1000 };
        let r = rela(&[(0x1000, 8, 0x40), (0x1008, 37, 0x99), (0x1018, 0, 0)]);
        let j = rela(&[(0x1020, 8, 0x50)]);
        // 0x1010 (+bias), then bitmap bits 0 and 2 -> 0x1018, 0x1028; then 0x1030 run and bit 62
        // of the next bitmap -> 0x1038 + 62*8; bit 1 of the bitmap after it -> 0x1038 + 63*8 (a
        // bitmap covers 63 words: the second one's base tells a 63-word span from 64).
        let rr = words(&[0x1010, 0b1011, 0x1030, 1 | (1 << 63), 0b11]);
        relocate::<X86_64>(&r, &j, &rr, &mut seg, 0x7000_0000).unwrap();
        assert_eq!(at(&mem, 0), 0x7000_0040);
        assert_eq!(at(&mem, 1), 0, "IRELATIVE waits for pass 2");
        assert_eq!(at(&mem, 2), 0x7000_0055);
        assert_eq!(at(&mem, 3), 0x7000_0000);
        assert_eq!(at(&mem, 4), 0x7000_0050);
        assert_eq!(at(&mem, 5), 0x7000_0000);
        assert_eq!(at(&mem, 6), 0x7000_0000);
        assert_eq!(at(&mem, 7 + 62), 0x7000_0000);
        assert_eq!(at(&mem, 7 + 63), 0x7000_0000, "the second bitmap starts 63 words on");
        assert_eq!(mem.chunks(8).filter(|w| w != &[0; 8]).count(), 8, "no other word written");
        assert_eq!(irelative::<X86_64>(&r).collect::<alloc::vec::Vec<_>>(), [(0x1008, 0x99)]);
    }

    #[test]
    fn refusals() {
        let mut mem = words(&[0; 4]);
        let mut seg = Segment { bytes: &mut mem, vaddr: 0x1000 };
        assert_eq!(relocate::<X86_64>(&rela(&[(0x1000, 6, 0)]), &[], &[], &mut seg, 1), Err(BootError::Unsupported(6)));
        assert_eq!(relocate::<X86_64>(&rela(&[(0x1020, 8, 0)]), &[], &[], &mut seg, 1), Err(BootError::Outside(0x1020)));
        assert_eq!(relocate::<X86_64>(&rela(&[(0x1019, 8, 0)]), &[], &[], &mut seg, 1), Err(BootError::Outside(0x1019)));
        assert_eq!(relocate::<X86_64>(&rela(&[(0xff8, 8, 0)]), &[], &[], &mut seg, 1), Err(BootError::Outside(0xff8)));
        assert_eq!(relocate::<X86_64>(&[], &[], &words(&[3]), &mut seg, 1), Err(BootError::RelrNoBase));
        assert_eq!(relocate::<X86_64>(&[], &[], &words(&[0x1018, 3]), &mut seg, 1), Err(BootError::Outside(0x1020)));
        assert_eq!(relocate::<X86_64>(&[], &[], &words(&[0x1020]), &mut seg, 1), Err(BootError::Outside(0x1020)));
    }

    #[test]
    fn dynamic_and_headers() {
        let w = (0x5000, 0x1000);
        let d = [dt::RELA, 0x100, dt::RELASZ, 48, dt::RELAENT, 24, dt::RELR, 0x200, dt::RELRSZ, 16, dt::RELRENT, 8, dt::JMPREL, 0x300, dt::PLTRELSZ, 24, dt::PLTREL, dt::RELA, dt::NULL, 0];
        assert_eq!(tables(&d, w), Ok(Tables { rela: (0x100, 48), jmprel: (0x300, 24), relr: (0x200, 16) }));
        assert_eq!(tables(&d[..d.len() - 2], w), Err(BootError::NoNull));
        let bad = |i: usize, v: u64| {
            let mut x = d;
            x[i] = v;
            tables(&x, w)
        };
        assert_eq!(bad(5, 16), Err(BootError::BadDynamic(dt::RELAENT)));
        assert_eq!(bad(11, 4), Err(BootError::BadDynamic(dt::RELRENT)));
        assert_eq!(bad(17, 17), Err(BootError::BadDynamic(dt::PLTREL)));
        assert_eq!(bad(0, DT_TEXTREL), Err(BootError::BadDynamic(DT_TEXTREL)));
        assert_eq!(bad(3, 40), Err(BootError::Table(dt::RELA)));
        assert_eq!(bad(9, 12), Err(BootError::Table(dt::RELR)));
        assert_eq!(bad(1, 0x4ff0), Err(BootError::Table(dt::RELA)), "overlaps the writable segment");
        assert_eq!(bad(7, 0x5ff8), Err(BootError::Table(dt::RELR)));
        assert_eq!(bad(13, u64::MAX - 8), Err(BootError::Table(dt::JMPREL)));
        // Headers as words; file offset = vaddr - `base` (the image is linked at `base`).
        let ph = |base: u64, ty: u64, flags: u64, vaddr: u64, memsz: u64| [ty | flags << 32, vaddr - base, vaddr, vaddr, memsz, memsz, 8];
        let hs: alloc::vec::Vec<u64> = [ph(0, 1, 5, 0, 0x1000), ph(0, 1, 6, 0x5000, 0x800), ph(0, 2, 6, 0x5100, 0x100)].concat();
        assert_eq!(image(&hs, 3), Ok(Image { header: 0, dynamic: (0x5100, 0x100), writable: (0x5000, 0x800) }));
        assert_eq!(image(&hs, 2), Err(BootError::Segments));
        let two: alloc::vec::Vec<u64> = [hs.clone(), ph(0, 1, 6, 0x9000, 8).to_vec()].concat();
        assert_eq!(image(&two, 4), Err(BootError::Segments));
        // The header's link address is the vaddr of the segment at file offset 0, wherever that is.
        let b = 0x40_0000;
        let at_b: alloc::vec::Vec<u64> = [ph(b, 2, 6, b + 0x5100, 0x100), ph(b, 1, 6, b + 0x5000, 0x800), ph(b, 1, 5, b, 0x1000)].concat();
        assert_eq!(image(&at_b, 3).map(|i| i.header), Ok(b));
        // No segment maps the ELF header: no bias can be found.
        let no_header: alloc::vec::Vec<u64> = [ph(0, 1, 6, 0x5000, 0x800), ph(0, 2, 6, 0x5100, 0x100)].concat();
        assert_eq!(image(&no_header, 2), Err(BootError::Segments));
    }

    #[test]
    fn position_independent_header() {
        let mut e = [0u8; 64];
        e[16..18].copy_from_slice(&et::DYN.to_ne_bytes());
        assert!(position_independent(&e));
        e[16..18].copy_from_slice(&et::EXEC.to_ne_bytes());
        assert!(!position_independent(&e), "a non-PIE executable is not relocated");
        e[16..18].copy_from_slice(&1u16.to_ne_bytes());
        assert!(!position_independent(&e), "only ET_DYN is position-independent (ET_REL is not)");
        assert!(!position_independent(&e[..17]), "a short header is not position-independent");
    }
}
