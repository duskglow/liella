//! The self-relocator's logic (phase 14 DL-4, design §1, ruling R14-1, D-27.10): what an image
//! that relocates itself (a static PIE today, `ld.so` from DL-5) must do before any of its
//! relocated data is used. The memory side (the slices over the image) is
//! `liella_boundary::bootstrap`; everything here is pure and testable on the host.
//!
//! Rules (D-27.10):
//! * Inputs are the image's program headers and its dynamic array, read in place: one `PT_DYNAMIC`
//!   and at least one writable `PT_LOAD` (any number of them: D-27.25, owner ruling #4655); every
//!   relocation target lies inside one of them (all 8 bytes, never across two) and is written
//!   through that segment's own view ([`Writer`]), and the relocation tables lie outside every one
//!   of them (the relocator never reads what it writes).
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
    /// No `PT_DYNAMIC`, no writable `PT_LOAD`, or a writable `PT_LOAD` whose end overflows.
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
    /// A relocation target outside every writable segment (or across two); its link-time address.
    Outside(u64),
    /// A RELR bitmap word with no address word before it.
    RelrNoBase,
}

/// The parts of the image the relocator works on, from its program headers (link-time addresses).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Image<'h> {
    /// The link-time address of the ELF header: `p_vaddr` of the `PT_LOAD` that maps file offset 0.
    /// The caller's run-time address of the header minus this is the bias.
    pub header: u64,
    /// `PT_DYNAMIC`: address and size in bytes.
    pub dynamic: (u64, u64),
    /// The program headers (as words, `phnum` of them): [`Image::writable`] reads the writable
    /// `PT_LOAD`s from them, so any number is held without a copy or an allocation.
    headers: &'h [u64],
}

impl<'h> Image<'h> {
    /// Every writable `PT_LOAD` (link-time address, size in memory), in header order; [`image`]
    /// checked there is at least one and that no end overflows.
    pub fn writable(&self) -> impl Iterator<Item = (u64, u64)> + 'h {
        self.headers.chunks_exact(PHDR_WORDS).filter_map(writable_load)
    }
}

/// A program header's (link-time address, size in memory) when it is a writable `PT_LOAD`.
fn writable_load(h: &[u64]) -> Option<(u64, u64)> {
    let w0 = h.first().copied().unwrap_or(0);
    (w0 & 0xffff_ffff == u64::from(pt::LOAD) && (w0 >> 32) & PF_W != 0).then(|| (h.get(2).copied().unwrap_or(0), h.get(5).copied().unwrap_or(0)))
}

/// Whether the 8 bytes at `at` lie inside the segment `(vaddr, memsz)`.
fn holds((vaddr, memsz): (u64, u64), at: u64) -> bool {
    at >= vaddr && at.checked_add(8).is_some_and(|end| end <= vaddr.wrapping_add(memsz))
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
/// ELF header is linked, its dynamic array and its writable segments (at least one; any number).
pub fn image(words: &[u64], phnum: usize) -> Result<Image<'_>, BootError> {
    let headers = words.get(..phnum.saturating_mul(PHDR_WORDS)).unwrap_or(words);
    let (mut header, mut dynamic, mut writable) = (None, None, false);
    for h in headers.chunks_exact(PHDR_WORDS) {
        let (w0, vaddr, memsz) = (h.first().copied().unwrap_or(0), h.get(2).copied().unwrap_or(0), h.get(5).copied().unwrap_or(0));
        let ty = u32::try_from(w0 & 0xffff_ffff).unwrap_or(0);
        if ty == pt::LOAD && h.get(1) == Some(&0) && header.is_none() {
            header = Some(vaddr);
        }
        if ty == pt::DYNAMIC && dynamic.is_none() {
            dynamic = Some((vaddr, memsz));
        } else if let Some((v, m)) = writable_load(h) {
            // An end past the address space would let a target wrap around ([`holds`]).
            v.checked_add(m).ok_or(BootError::Segments)?;
            writable = true;
        }
    }
    match (header, dynamic) {
        (Some(header), Some(dynamic)) if writable => Ok(Image { header, dynamic, headers }),
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
/// checked against the writable segments of `img`: each table holds whole entries, lies in the
/// address space and overlaps none of the segments.
pub fn tables(dynamic: &[u64], img: &Image<'_>) -> Result<Tables, BootError> {
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
    for (tag, (addr, len), ent) in [(dt::RELA, t.rela, RELA_ENT), (dt::JMPREL, t.jmprel, RELA_ENT), (dt::RELR, t.relr, RELR_ENT)] {
        if len == 0 {
            continue;
        }
        let end = addr.checked_add(len).ok_or(BootError::Table(tag))?;
        // `image` checked every segment's end fits the address space.
        if len.checked_rem(ent) != Some(0) || img.writable().any(|(v, m)| addr < v.wrapping_add(m) && end > v) {
            return Err(BootError::Table(tag));
        }
    }
    Ok(t)
}

/// The memory of the image's writable segments, one view per segment (the caller's: the boundary
/// makes it from the mapped image, tests from a buffer).
pub trait Memory {
    /// The bytes of the writable segment linked at `vaddr` with memory size `memsz`, from its
    /// first byte. Asked for only after [`Writer`] found the target inside that segment.
    fn segment(&mut self, vaddr: u64, memsz: u64) -> &mut [u8];
}

/// The writable segments as the relocator sees them: each target is found in the segment that
/// holds it (the last one used is tried first: targets come in runs) and written through that
/// segment's view; a target in no writable segment is refused.
#[derive(Debug)]
pub struct Writer<'h, M> {
    img: Image<'h>,
    mem: M,
    /// The segment of the last target (link-time address, memory size); (0, 0) holds nothing.
    last: (u64, u64),
}

impl<'h, M: Memory> Writer<'h, M> {
    /// A writer over the writable segments of `img`, whose memory is `mem`.
    pub fn new(img: Image<'h>, mem: M) -> Self {
        Writer { img, mem, last: (0, 0) }
    }

    /// The 8 bytes at link-time address `at`, if they lie inside one writable segment.
    fn word(&mut self, at: u64) -> Result<&mut [u8; 8], BootError> {
        if !holds(self.last, at) {
            self.last = self.img.writable().find(|&s| holds(s, at)).ok_or(BootError::Outside(at))?;
        }
        let (vaddr, memsz) = self.last;
        let off = usize::try_from(at.wrapping_sub(vaddr)).map_err(|_| BootError::Outside(at))?;
        let view = self.mem.segment(vaddr, memsz);
        view.get_mut(off..off.wrapping_add(8)).and_then(|b| <&mut [u8; 8]>::try_from(b).ok()).ok_or(BootError::Outside(at))
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
fn relative<A: Arch, M: Memory>(table: &[u8], seg: &mut Writer<'_, M>, bias: u64) -> Result<(), BootError> {
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
fn relr<M: Memory>(table: &[u8], seg: &mut Writer<'_, M>, bias: u64) -> Result<(), BootError> {
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
/// `seg` (the writable segment holding it) adjusted by `bias` (load address minus link address).
pub fn relocate<A: Arch, M: Memory>(rela: &[u8], jmprel: &[u8], relr_table: &[u8], seg: &mut Writer<'_, M>, bias: u64) -> Result<(), BootError> {
    relative::<A, M>(rela, seg, bias)?;
    relative::<A, M>(jmprel, seg, bias)?;
    relr(relr_table, seg, bias)
}

/// Pass 2's work list: the (`r_offset`, `r_addend`) of each `Irelative` entry of `table`, in order.
pub fn irelative<A: Arch>(table: &[u8]) -> impl Iterator<Item = (u64, u64)> + '_ {
    table.chunks_exact(RELA_SIZE).map(rela_entry).filter(|&(_, t, _)| A::classify(t) == RelocKind::Irelative).map(|(o, _, a)| (o, a))
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    //! Design §1 / D-27.10, D-27.25 (no spec clause: the relocator is internal to start-up). Types
    //! are x86_64 numbers (8 RELATIVE, 37 IRELATIVE, 6 GLOB_DAT), so these run with `X86_64`.
    use super::*;
    use crate::arch::X86_64;

    fn rela(entries: &[(u64, u64, u64)]) -> alloc::vec::Vec<u8> {
        entries.iter().flat_map(|&(o, t, a)| [o, t, a]).flat_map(u64::to_ne_bytes).collect()
    }
    fn words(ws: &[u64]) -> alloc::vec::Vec<u8> {
        ws.iter().copied().flat_map(u64::to_ne_bytes).collect()
    }
    fn at(mem: &[u8], addr: u64) -> u64 {
        let i = usize::try_from(addr).unwrap();
        u64::from_ne_bytes(mem[i..i + 8].try_into().unwrap())
    }
    /// The image's link-time address space from 0 as one buffer. A segment's view runs from its
    /// first byte to the end of the buffer, past the segment: only the relocator's own check
    /// (`holds`) can refuse a target outside every writable segment, and a word written through
    /// another segment's view lands at the wrong address.
    struct Flat<'a>(&'a mut [u8]);
    impl Memory for Flat<'_> {
        fn segment(&mut self, vaddr: u64, _memsz: u64) -> &mut [u8] {
            &mut self.0[usize::try_from(vaddr).unwrap()..]
        }
    }
    /// A program header as words; file offset = vaddr - `base` (the image is linked at `base`).
    fn ph(base: u64, ty: u64, flags: u64, vaddr: u64, memsz: u64) -> [u64; 7] {
        [ty | flags << 32, vaddr - base, vaddr, vaddr, memsz, memsz, 8]
    }
    /// Headers for an image linked at 0: the ELF header's segment, `PT_DYNAMIC`, then one writable
    /// `PT_LOAD` per `(vaddr, memsz)`.
    fn headers(writable: &[(u64, u64)]) -> alloc::vec::Vec<u64> {
        let mut hs = [ph(0, 1, 5, 0, 0x800), ph(0, 2, 6, 0x40, 0x10)].concat();
        for &(v, m) in writable {
            hs.extend_from_slice(&ph(0, 1, 6, v, m));
        }
        hs
    }

    #[test]
    fn relative_relr_and_irelative_split() {
        let hs = headers(&[(0x1000, 640)]);
        let img = image(&hs, 3).unwrap();
        let mut mem = alloc::vec![0u8; 0x2000];
        mem[0x1010..0x1018].copy_from_slice(&0x55u64.to_ne_bytes());
        let r = rela(&[(0x1000, 8, 0x40), (0x1008, 37, 0x99), (0x1018, 0, 0)]);
        let j = rela(&[(0x1020, 8, 0x50)]);
        // 0x1010 (+bias), then bitmap bits 0 and 2 -> 0x1018, 0x1028; then 0x1030 run and bit 62
        // of the next bitmap -> 0x1038 + 62*8; bit 1 of the bitmap after it -> 0x1038 + 63*8 (a
        // bitmap covers 63 words: the second one's base tells a 63-word span from 64).
        let rr = words(&[0x1010, 0b1011, 0x1030, 1 | (1 << 63), 0b11]);
        relocate::<X86_64, _>(&r, &j, &rr, &mut Writer::new(img, Flat(&mut mem)), 0x7000_0000).unwrap();
        assert_eq!(at(&mem, 0x1000), 0x7000_0040);
        assert_eq!(at(&mem, 0x1008), 0, "IRELATIVE waits for pass 2");
        assert_eq!(at(&mem, 0x1010), 0x7000_0055);
        assert_eq!(at(&mem, 0x1018), 0x7000_0000);
        assert_eq!(at(&mem, 0x1020), 0x7000_0050);
        assert_eq!(at(&mem, 0x1028), 0x7000_0000);
        assert_eq!(at(&mem, 0x1030), 0x7000_0000);
        assert_eq!(at(&mem, 0x1038 + 62 * 8), 0x7000_0000);
        assert_eq!(at(&mem, 0x1038 + 63 * 8), 0x7000_0000, "the second bitmap starts 63 words on");
        assert_eq!(mem.chunks(8).filter(|w| w != &[0; 8]).count(), 8, "no other word written");
        assert_eq!(irelative::<X86_64>(&r).collect::<alloc::vec::Vec<_>>(), [(0x1008, 0x99)]);
    }

    /// D-27.25: three writable segments with gaps between them (as a 16 KiB-aligned `.data1` and
    /// a second orphan data section give); RELA and RELR targets in each land in their own
    /// segment, in any order, and a run of RELR bits may cross from one segment into the next only
    /// where the two are adjacent.
    #[test]
    fn writable_segments_each_relocated() {
        let segs = [(0x1000, 0x20), (0x3000, 0x18), (0x5000, 0x10), (0x5010, 0x10)];
        let hs = headers(&segs);
        let img = image(&hs, 6).unwrap();
        assert_eq!(img.writable().collect::<alloc::vec::Vec<_>>(), segs);
        let mut mem = alloc::vec![0u8; 0x6000];
        let r = rela(&[(0x3010, 8, 1), (0x1000, 8, 2), (0x5008, 8, 3), (0x3000, 8, 4), (0x1018, 8, 5)]);
        // 0x3008, then 0x5000 and bits 0 and 1: 0x5008 (+= bias over the RELA value) and 0x5010,
        // the first word of the adjacent fourth segment.
        let rr = words(&[0x3008, 0x5000, 0b111]);
        relocate::<X86_64, _>(&r, &[], &rr, &mut Writer::new(img, Flat(&mut mem)), 0x100).unwrap();
        let want = [(0x1000, 0x102), (0x1018, 0x105), (0x3000, 0x104), (0x3008, 0x100), (0x3010, 0x101), (0x5000, 0x100), (0x5008, 0x203), (0x5010, 0x100)];
        for (a, v) in want {
            assert_eq!(at(&mem, a), v, "word at {a:#x}");
        }
        assert_eq!(mem.chunks(8).filter(|w| w != &[0; 8]).count(), want.len(), "no other word written");
    }

    #[test]
    fn refusals() {
        // The segments are 0x1000..0x1020 and 0x3000..0x3010; the buffer is mapped well past both.
        let hs = headers(&[(0x1000, 0x20), (0x3000, 0x10)]);
        let img = image(&hs, 4).unwrap();
        let mut mem = alloc::vec![0u8; 0x4000];
        let mut run = |r: &[u8], rr: &[u8]| relocate::<X86_64, _>(r, &[], rr, &mut Writer::new(img, Flat(&mut mem)), 1);
        assert_eq!(run(&rela(&[(0x1000, 6, 0)]), &[]), Err(BootError::Unsupported(6)));
        assert_eq!(run(&rela(&[(0x1020, 8, 0)]), &[]), Err(BootError::Outside(0x1020)));
        assert_eq!(run(&rela(&[(0x1019, 8, 0)]), &[]), Err(BootError::Outside(0x1019)));
        assert_eq!(run(&rela(&[(0xff8, 8, 0)]), &[]), Err(BootError::Outside(0xff8)));
        assert_eq!(run(&rela(&[(0x2000, 8, 0)]), &[]), Err(BootError::Outside(0x2000)), "between the segments");
        assert_eq!(run(&rela(&[(0x3000, 8, 0), (0x3010, 8, 0)]), &[]), Err(BootError::Outside(0x3010)), "past the second");
        assert_eq!(run(&rela(&[(0x300c, 8, 0)]), &[]), Err(BootError::Outside(0x300c)));
        assert_eq!(run(&[], &words(&[3])), Err(BootError::RelrNoBase));
        assert_eq!(run(&[], &words(&[0x1018, 3])), Err(BootError::Outside(0x1020)));
        assert_eq!(run(&[], &words(&[0x1020])), Err(BootError::Outside(0x1020)));
        assert_eq!(run(&[], &words(&[0x2ff8])), Err(BootError::Outside(0x2ff8)));
        // Only the in-segment words before a refused one were written (0x3000, then 0x1018 of the
        // RELR run that 0x1020 ends): nothing in the gap or past either segment.
        let written: alloc::vec::Vec<usize> = (0..mem.len()).step_by(8).filter(|&i| at(&mem, u64::try_from(i).unwrap()) != 0).collect();
        assert_eq!(written, [0x1018, 0x3000], "words written");
        // Two adjacent segments: a word may not straddle them.
        let hs = headers(&[(0x1000, 0x10), (0x1010, 0x10)]);
        let mut mem = alloc::vec![0u8; 0x2000];
        let r = relocate::<X86_64, _>(&rela(&[(0x100c, 8, 0)]), &[], &[], &mut Writer::new(image(&hs, 4).unwrap(), Flat(&mut mem)), 1);
        assert_eq!(r, Err(BootError::Outside(0x100c)));
    }

    #[test]
    fn dynamic_and_headers() {
        let hs = headers(&[(0x5000, 0x1000)]);
        let img = image(&hs, 3).unwrap();
        let d = [dt::RELA, 0x100, dt::RELASZ, 48, dt::RELAENT, 24, dt::RELR, 0x200, dt::RELRSZ, 16, dt::RELRENT, 8, dt::JMPREL, 0x300, dt::PLTRELSZ, 24, dt::PLTREL, dt::RELA, dt::NULL, 0];
        assert_eq!(tables(&d, &img), Ok(Tables { rela: (0x100, 48), jmprel: (0x300, 24), relr: (0x200, 16) }));
        assert_eq!(tables(&d[..d.len() - 2], &img), Err(BootError::NoNull));
        let bad = |i: usize, v: u64, img: &Image<'_>| {
            let mut x = d;
            x[i] = v;
            tables(&x, img)
        };
        assert_eq!(bad(5, 16, &img), Err(BootError::BadDynamic(dt::RELAENT)));
        assert_eq!(bad(11, 4, &img), Err(BootError::BadDynamic(dt::RELRENT)));
        assert_eq!(bad(17, 17, &img), Err(BootError::BadDynamic(dt::PLTREL)));
        assert_eq!(bad(0, DT_TEXTREL, &img), Err(BootError::BadDynamic(DT_TEXTREL)));
        assert_eq!(bad(3, 40, &img), Err(BootError::Table(dt::RELA)));
        assert_eq!(bad(9, 12, &img), Err(BootError::Table(dt::RELR)));
        assert_eq!(bad(1, 0x4ff0, &img), Err(BootError::Table(dt::RELA)), "overlaps the writable segment");
        assert_eq!(bad(7, 0x5ff8, &img), Err(BootError::Table(dt::RELR)));
        assert_eq!(bad(13, u64::MAX - 8, &img), Err(BootError::Table(dt::JMPREL)));
        // A table overlapping the second of two writable segments is refused too.
        let two = headers(&[(0x5000, 0x1000), (0x9000, 0x100)]);
        let img2 = image(&two, 4).unwrap();
        assert_eq!(tables(&d, &img2), Ok(Tables { rela: (0x100, 48), jmprel: (0x300, 24), relr: (0x200, 16) }));
        assert_eq!(bad(13, 0x90f8, &img2), Err(BootError::Table(dt::JMPREL)), "overlaps the second writable segment");
        assert_eq!(img2.writable().collect::<alloc::vec::Vec<_>>(), [(0x5000, 0x1000), (0x9000, 0x100)]);
        assert_eq!(image(&two, 2), Err(BootError::Segments), "no writable segment among the first two");
        // A writable segment whose end overflows the address space is refused.
        assert_eq!(image(&headers(&[(0x5000, 0x10), (u64::MAX - 8, 0x10)]), 4), Err(BootError::Segments));
        assert_eq!(image(&hs, 3).map(|i| (i.header, i.dynamic)), Ok((0, (0x40, 0x10))));
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
