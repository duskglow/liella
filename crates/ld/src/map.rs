//! Mapping objects: reservations, segments, RELRO, CET marking.
//!
//! DL-6 (D-27.14) adds the plan for `ld.so PROGRAM`: a position-independent executable with no
//! dependencies, mapped as one reservation with each `PT_LOAD` inside it. DL-7 (`DL-MAP`) extends
//! this into the general mapper (shared objects, `/proc/self/maps` checks, the malformed corpus).
//! Pure logic: the boundary does the system calls the plan describes.

use crate::arch::Arch;
use crate::elf::{Elf, ElfError, ProgramHeader};

/// `ET_DYN` (ELF gABI).
const ET_DYN: u16 = 3;
/// `PT_LOAD`, `PT_PHDR` (ELF gABI).
const PT_LOAD: u32 = 1;
const PT_PHDR: u32 = 6;
/// `PF_X`, `PF_W`, `PF_R` (ELF gABI).
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
/// `PROT_READ`, `PROT_WRITE`, `PROT_EXEC` (kernel UAPI `asm-generic/mman-common.h`).
const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const PROT_EXEC: i32 = 4;
/// The largest span accepted (2^46 bytes: half of x86-64's 47-bit user space; a larger span is a
/// malformed object, never a real program).
const SPAN_MAX: u64 = 1 << 46;

/// Why a program cannot be mapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    /// The ELF reader refused the file.
    Elf(ElfError),
    /// Not `ET_DYN` (a fixed-address executable is DL-7's).
    NotPie,
    /// No `PT_LOAD`, or the program headers are not inside a loaded segment.
    NoSegments,
    /// `PT_LOAD`s unsorted, overlapping, misaligned against the page, or the span too large.
    BadSegments,
    /// A segment asks to be writable and executable at once (design §3).
    WriteExec,
}

impl MapError {
    /// The diagnostic line (standard error).
    pub fn message(self) -> &'static [u8] {
        match self {
            MapError::Elf(_) => b"ld.so: the program is not a valid ELF file for this machine\n",
            MapError::NotPie => b"ld.so: the program is not position-independent (only PIE programs load in this build)\n",
            MapError::NoSegments => b"ld.so: the program has no loadable segments holding its headers\n",
            MapError::BadSegments => b"ld.so: the program's loadable segments are malformed\n",
            MapError::WriteExec => b"ld.so: the program asks for a writable and executable segment\n",
        }
    }
}

/// One `PT_LOAD`, in page terms, relative to the load base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// First page (`p_vaddr` rounded down).
    pub start: u64,
    /// File offset of that page (`p_offset` rounded down by the same amount).
    pub offset: u64,
    /// Bytes mapped from the file from `start` (0 when the segment has no file bytes).
    pub file_len: u64,
    /// End of the file bytes (`p_vaddr + p_filesz`); from here to `zero_end` is zero.
    pub file_end: u64,
    /// End of the whole segment, rounded up to the page.
    pub zero_end: u64,
    /// `PROT_*` bits, exactly as `p_flags` asks.
    pub prot: i32,
}

/// The mapping plan of a position-independent program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Bytes to reserve (from the first segment's page to the last one's end).
    pub span: u64,
    /// The reservation's alignment: the largest `p_align` (at least the page).
    pub align: u64,
    /// The link-time address the reservation's start corresponds to (the first page).
    pub first: u64,
    /// The program headers' link-time address.
    pub phdr: u64,
    /// The number of program headers.
    pub phnum: u16,
    /// The entry point's link-time address.
    pub entry: u64,
}

/// The `PT_LOAD` segment `ph` in page terms (`page` a power of two).
///
/// # Errors
///
/// [`MapError::BadSegments`] for a misaligned or overflowing segment, [`MapError::WriteExec`].
pub fn segment(ph: &ProgramHeader, page: u64) -> Result<Segment, MapError> {
    let mask = page.wrapping_sub(1);
    if ph.vaddr & mask != ph.offset & mask || ph.filesz > ph.memsz {
        return Err(MapError::BadSegments);
    }
    if ph.flags & PF_W != 0 && ph.flags & PF_X != 0 {
        return Err(MapError::WriteExec);
    }
    let start = ph.vaddr & !mask;
    let lead = ph.vaddr.wrapping_sub(start);
    let file_end = ph.vaddr.checked_add(ph.filesz).ok_or(MapError::BadSegments)?;
    let end = ph.vaddr.checked_add(ph.memsz).and_then(|e| e.checked_add(mask)).ok_or(MapError::BadSegments)? & !mask;
    let file_len = if ph.filesz == 0 { 0 } else { lead.checked_add(ph.filesz).ok_or(MapError::BadSegments)? };
    let mut prot = 0;
    for (f, p) in [(PF_R, PROT_READ), (PF_W, PROT_WRITE), (PF_X, PROT_EXEC)] {
        if ph.flags & f != 0 {
            prot |= p;
        }
    }
    Ok(Segment { start, offset: ph.offset & !mask, file_len, file_end, zero_end: end, prot })
}

/// The plan for the program in `elf` (the whole file), with pages of `page` bytes.
///
/// # Errors
///
/// [`MapError`] for anything but a well-formed position-independent program.
pub fn plan<A: Arch>(elf: &Elf<'_>, page: u64) -> Result<Plan, MapError> {
    let h = elf.header();
    if h.e_type != ET_DYN {
        return Err(MapError::NotPie);
    }
    let (mut first, mut last, mut align, mut count) = (0u64, 0u64, page, 0usize);
    let mut phdr = elf.find(PT_PHDR).map(|p| p.vaddr);
    for ph in elf.program_headers().filter(|p| p.p_type == PT_LOAD) {
        let s = segment(&ph, page)?;
        elf.segment_bytes(&ph).map_err(MapError::Elf)?;
        if count == 0 {
            first = s.start;
        } else if s.start < last {
            return Err(MapError::BadSegments);
        }
        last = s.zero_end;
        if ph.align > align && ph.align.is_power_of_two() {
            align = ph.align;
        }
        // Without PT_PHDR, the headers' address is where the segment holding them maps them.
        let phend = h.phoff.saturating_add(u64::from(h.phnum).saturating_mul(56));
        if phdr.is_none() && ph.offset <= h.phoff && phend <= ph.offset.saturating_add(ph.filesz) {
            phdr = Some(ph.vaddr.wrapping_add(h.phoff.wrapping_sub(ph.offset)));
        }
        count = count.wrapping_add(1);
    }
    let span = last.wrapping_sub(first);
    if count == 0 {
        return Err(MapError::NoSegments);
    }
    if span == 0 || span > SPAN_MAX || align > SPAN_MAX {
        return Err(MapError::BadSegments);
    }
    let phdr = phdr.ok_or(MapError::NoSegments)?;
    Ok(Plan { span, align, first, phdr, phnum: h.phnum, entry: h.entry })
}

/// Parse `file` (a whole program file) and plan it.
///
/// # Errors
///
/// As [`plan`], and [`MapError::Elf`] when the reader refuses the file.
pub fn plan_file<A: Arch>(file: &[u8], page: u64) -> Result<Plan, MapError> {
    let elf = Elf::parse::<A>(file).map_err(MapError::Elf)?;
    plan::<A>(&elf, page)
}

/// The `PT_LOAD` segments of `file` in page terms, in header order (after [`plan_file`] accepted it).
pub fn segments<'a, A: Arch>(file: &'a [u8], page: u64) -> impl Iterator<Item = Result<Segment, MapError>> + 'a {
    let elf = Elf::parse::<A>(file).ok();
    elf.into_iter().flat_map(|e| (0..e.header().phnum).filter_map(move |i| e.program_header(i))).filter(|p| p.p_type == PT_LOAD).map(move |p| segment(&p, page))
}
