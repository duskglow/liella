//! Mapping objects: the reservation, the segments, the `.bss` tail, the RELRO hook (design §3).
//!
//! DL-6 (D-27.14) planned a position-independent program for `ld.so PROGRAM`. DL-7 (`DL-MAP`,
//! D-27.15) makes this the general mapper of every object: position-independent objects (`ET_DYN`:
//! programs and shared objects) anywhere, fixed-address programs (`ET_EXEC`) at their link-time
//! addresses. The steps:
//!
//! 1. [`plan`] validates the headers before anything is mapped: at most [`LOADS_MAX`] `PT_LOAD`s,
//!    each non-empty, with `p_filesz <= p_memsz`, `p_offset` congruent to `p_vaddr` modulo the page,
//!    `p_align` 0, 1 or a power of two, its file bytes inside the file, never writable and
//!    executable at once; the segments in address order without sharing a page; the span and the
//!    alignment bounded; a `PT_GNU_RELRO` range inside the span.
//! 2. [`load`] makes one `PROT_NONE` reservation of the span, aligned so the load bias is a
//!    multiple of the largest `p_align` (honoured above the page size, L14-5), or at the fixed
//!    address without replacing anything there; then maps each segment inside it with exactly the
//!    protection its `p_flags` ask for, zeroes the `.bss` bytes that share the last file page and
//!    maps zero pages for the rest. Gaps between segments stay reserved and inaccessible.
//! 3. On any failure everything mapped so far is unmapped (the whole reservation) before the error
//!    returns.
//! 4. The RELRO range is recorded in [`Loaded`]; [`Loaded::protect_relro`] applies it after
//!    relocation (DL-10).
//!
//! Pure logic: the system calls are the [`Mapper`]'s (the boundary implements it with `mmap`,
//! `munmap` and `mprotect`; the tests with a model of an address space). The page size is a
//! parameter, taken from `AT_PAGESZ` through [`Arch::page_size`](crate::arch::Arch::page_size).

use crate::arch::Arch;
use crate::elf::{pt, Elf, ElfError, ProgramHeader};

/// `ET_EXEC`, `ET_DYN` (ELF gABI).
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
/// `PF_X`, `PF_W`, `PF_R` (ELF gABI).
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
/// `PROT_READ`, `PROT_WRITE`, `PROT_EXEC` (kernel UAPI `asm-generic/mman-common.h`).
pub const PROT_READ: i32 = 1;
/// See [`PROT_READ`].
pub const PROT_WRITE: i32 = 2;
/// See [`PROT_READ`].
pub const PROT_EXEC: i32 = 4;
/// The largest span or alignment accepted (2^46 bytes: half of x86-64's 47-bit user space; a
/// larger one is a malformed object, never a real one).
const SPAN_MAX: u64 = 1 << 46;
/// The most `PT_LOAD`s an object may have (D-27.15): linkers emit two to five; the bound keeps the
/// work per object fixed and lets the loader keep no list of segments.
pub const LOADS_MAX: usize = 64;
/// Bytes per program header (ELF64).
const PHENT: u64 = 56;

/// Why an object cannot be mapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    /// The ELF reader refused the file.
    Elf(ElfError),
    /// Neither `ET_EXEC` nor `ET_DYN`.
    NotLoadable,
    /// No `PT_LOAD`, or the program headers are not inside a loaded segment.
    NoSegments,
    /// More than [`LOADS_MAX`] `PT_LOAD`s.
    TooManySegments,
    /// A `PT_LOAD` (or the `PT_GNU_RELRO` range) is malformed: empty, misaligned, outside the file,
    /// unsorted or overlapping, or the span or the alignment too large.
    BadSegments,
    /// A segment asks to be writable and executable at once (design §3).
    WriteExec,
    /// A fixed-address program's range is not free (or not in user space).
    Placement,
    /// A mapping system call failed (out of memory or address space).
    System,
}

impl MapError {
    /// The diagnostic text (after `ld.so: <path>: `).
    pub fn message(self) -> &'static [u8] {
        match self {
            MapError::Elf(_) => b"not a valid ELF file for this machine\n",
            MapError::NotLoadable => b"not a loadable object (neither ET_EXEC nor ET_DYN)\n",
            MapError::NoSegments => b"no loadable segments holding the program headers\n",
            MapError::TooManySegments => b"too many loadable segments\n",
            MapError::BadSegments => b"malformed loadable segments\n",
            MapError::WriteExec => b"a loadable segment asks to be writable and executable\n",
            MapError::Placement => b"the fixed load address is not available\n",
            MapError::System => b"cannot map the object (out of memory or address space)\n",
        }
    }
}

/// One `PT_LOAD`, in page terms, at link-time addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// First page (`p_vaddr` rounded down).
    pub start: u64,
    /// File offset of that page (`p_offset` rounded down by the same amount).
    pub offset: u64,
    /// Bytes from `start` that come from the file (0 when the segment has no file bytes).
    pub file_len: u64,
    /// End of the file bytes (`p_vaddr + p_filesz`); from here to `zero_end` is zero.
    pub file_end: u64,
    /// End of the whole segment, rounded up to the page.
    pub zero_end: u64,
    /// `PROT_*` bits, exactly as `p_flags` asks.
    pub prot: i32,
}

/// The mapping plan of an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Bytes to reserve (from the first segment's page to the last one's end).
    pub span: u64,
    /// The load bias's alignment: the largest `p_align` (at least the page).
    pub align: u64,
    /// The link-time address the reservation's start corresponds to (the first page).
    pub first: u64,
    /// `ET_EXEC`: mapped at its link-time addresses (bias 0).
    pub fixed: bool,
    /// The program headers' link-time address.
    pub phdr: u64,
    /// The number of program headers.
    pub phnum: u16,
    /// The entry point's link-time address.
    pub entry: u64,
    /// The pages `PT_GNU_RELRO` covers (link-time start, length), whole pages only; `None` without
    /// the header or when it covers no whole page.
    pub relro: Option<(u64, u64)>,
    /// The page size the plan was made for.
    pub page: u64,
}

/// The system calls of the mapper. Addresses and lengths are page multiples except where noted.
/// An `Err(())` makes [`load`] unmap everything and fail with [`MapError::System`] (or
/// [`MapError::Placement`] for a fixed reservation).
pub trait Mapper {
    /// Reserve `len` inaccessible bytes: anywhere when `at` is `None` (the start), or exactly at `at`
    /// without replacing an existing mapping.
    #[allow(clippy::result_unit_err)]
    fn reserve(&mut self, at: Option<u64>, len: u64) -> Result<u64, ()>;
    /// Map `len` bytes of the object's file from `offset` at `at`, private, replacing the
    /// reservation there, with protection `prot`.
    #[allow(clippy::result_unit_err)]
    fn map_file(&mut self, at: u64, len: u64, prot: i32, offset: u64) -> Result<(), ()>;
    /// Map `len` zero bytes at `at`, private, replacing the reservation there, with `prot`.
    #[allow(clippy::result_unit_err)]
    fn map_zero(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()>;
    /// Store `bytes` at `at` (any address and length; the range is mapped writable), or zeroes when
    /// `bytes` is `None` (`len` of them).
    fn store(&mut self, at: u64, len: u64, bytes: Option<&[u8]>);
    /// Change the protection of `len` bytes at `at` to `prot`.
    #[allow(clippy::result_unit_err)]
    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()>;
    /// Unmap `len` bytes at `at` (never fails on a range this mapper mapped).
    fn unmap(&mut self, at: u64, len: u64);
}

/// A mapped object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// The reservation's start (the first segment's page).
    pub base: u64,
    /// The reservation's length.
    pub span: u64,
    /// Run-time address minus link-time address (0 for `ET_EXEC`).
    pub bias: u64,
    /// The program headers' run-time address.
    pub phdr: u64,
    /// The number of program headers.
    pub phnum: u16,
    /// The entry point's run-time address.
    pub entry: u64,
    /// The RELRO pages at run time (start, length).
    pub relro: Option<(u64, u64)>,
}

impl Loaded {
    /// Make the RELRO pages read-only (DL-10 calls this after the object's relocation; L14-8).
    ///
    /// # Errors
    ///
    /// [`MapError::System`] when the protection change fails.
    pub fn protect_relro<M: Mapper>(&self, m: &mut M) -> Result<(), MapError> {
        match self.relro {
            Some((at, len)) => m.protect(at, len, PROT_READ).map_err(|()| MapError::System),
            None => Ok(()),
        }
    }

    /// Unmap the whole object.
    pub fn unmap<M: Mapper>(&self, m: &mut M) {
        m.unmap(self.base, self.span);
    }
}

fn page_down(v: u64, page: u64) -> u64 {
    v & !page.wrapping_sub(1)
}

fn page_up(v: u64, page: u64) -> Option<u64> {
    v.checked_add(page.wrapping_sub(1)).map(|e| page_down(e, page))
}

/// The `PT_LOAD` segment `ph` in page terms (`page` a power of two).
///
/// # Errors
///
/// [`MapError::BadSegments`] for an empty, misaligned or overflowing segment, [`MapError::WriteExec`].
pub fn segment(ph: &ProgramHeader, page: u64) -> Result<Segment, MapError> {
    let mask = page.wrapping_sub(1);
    let align_ok = ph.align <= 1 || (ph.align.is_power_of_two() && ph.align <= SPAN_MAX);
    if ph.vaddr & mask != ph.offset & mask || ph.filesz > ph.memsz || ph.memsz == 0 || !align_ok {
        return Err(MapError::BadSegments);
    }
    if ph.flags & PF_W != 0 && ph.flags & PF_X != 0 {
        return Err(MapError::WriteExec);
    }
    let start = page_down(ph.vaddr, page);
    let lead = ph.vaddr.wrapping_sub(start);
    let file_end = ph.vaddr.checked_add(ph.filesz).ok_or(MapError::BadSegments)?;
    let end = ph.vaddr.checked_add(ph.memsz).and_then(|e| page_up(e, page)).ok_or(MapError::BadSegments)?;
    let file_len = if ph.filesz == 0 { 0 } else { lead.checked_add(ph.filesz).ok_or(MapError::BadSegments)? };
    let mut prot = 0;
    for (f, p) in [(PF_R, PROT_READ), (PF_W, PROT_WRITE), (PF_X, PROT_EXEC)] {
        if ph.flags & f != 0 {
            prot |= p;
        }
    }
    Ok(Segment { start, offset: ph.offset & !mask, file_len, file_end, zero_end: end, prot })
}

/// The plan for the object in `elf` (the whole file), with pages of `page` bytes (a power of two;
/// [`crate::arch::Arch::page_size`]). Nothing is mapped.
///
/// # Errors
///
/// [`MapError`] for anything but a well-formed loadable object.
pub fn plan<A: Arch>(elf: &Elf<'_>, page: u64) -> Result<Plan, MapError> {
    let h = elf.header();
    if h.e_type != ET_DYN && h.e_type != ET_EXEC {
        return Err(MapError::NotLoadable);
    }
    if !page.is_power_of_two() {
        return Err(MapError::BadSegments);
    }
    if elf.program_headers().filter(|p| p.p_type == pt::LOAD).count() > LOADS_MAX {
        return Err(MapError::TooManySegments);
    }
    let (mut first, mut last, mut align, mut count) = (0u64, 0u64, page, 0usize);
    let mut phdr = elf.find(pt::PHDR).map(|p| p.vaddr);
    let phend = h.phoff.saturating_add(u64::from(h.phnum).saturating_mul(PHENT));
    for ph in elf.program_headers().filter(|p| p.p_type == pt::LOAD) {
        let s = segment(&ph, page)?;
        elf.segment_bytes(&ph).map_err(MapError::Elf)?;
        if count == 0 {
            first = s.start;
        } else if s.start < last {
            return Err(MapError::BadSegments);
        }
        last = s.zero_end;
        align = align.max(ph.align);
        // Without PT_PHDR, the headers' address is where the segment holding them maps them.
        if phdr.is_none() && ph.offset <= h.phoff && phend <= ph.offset.saturating_add(ph.filesz) {
            phdr = Some(ph.vaddr.wrapping_add(h.phoff.wrapping_sub(ph.offset)));
        }
        count = count.wrapping_add(1);
    }
    if count == 0 {
        return Err(MapError::NoSegments);
    }
    let span = last.wrapping_sub(first);
    if span > SPAN_MAX || (h.e_type == ET_EXEC && last > A::USER_END) {
        return Err(MapError::BadSegments);
    }
    let relro = match elf.find(pt::GNU_RELRO) {
        Some(r) => {
            let from = page_down(r.vaddr, page);
            let to = page_down(r.vaddr.checked_add(r.memsz).ok_or(MapError::BadSegments)?, page);
            if r.memsz != 0 && (r.vaddr < first || to > last) {
                return Err(MapError::BadSegments);
            }
            (to > from).then(|| (from, to.wrapping_sub(from)))
        }
        None => None,
    };
    let phdr = phdr.ok_or(MapError::NoSegments)?;
    Ok(Plan { span, align, first, fixed: h.e_type == ET_EXEC, phdr, phnum: h.phnum, entry: h.entry, relro, page })
}

/// Parse `file` (a whole object file) and plan it.
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
    elf.into_iter().flat_map(|e| (0..e.header().phnum).filter_map(move |i| e.program_header(i))).filter(|p| p.p_type == pt::LOAD).map(move |p| segment(&p, page))
}

/// Map the object `file` (its whole contents; the [`Mapper`] maps from the same file) with pages of
/// `page` bytes: plan, reserve, map each segment, zero the `.bss`. Nothing stays mapped on failure.
///
/// # Errors
///
/// As [`plan_file`] (nothing mapped yet), [`MapError::Placement`] when a fixed-address program's
/// range is taken, [`MapError::System`] when a mapping call fails.
pub fn load<A: Arch, M: Mapper>(m: &mut M, file: &[u8], page: u64) -> Result<Loaded, MapError> {
    let plan = plan_file::<A>(file, page)?;
    let (base, bias) = reserve(m, &plan)?;
    let at = |v: u64| v.wrapping_add(bias);
    for seg in segments::<A>(file, page) {
        let mapped = seg.and_then(|s| map_segment(m, file, &s, bias, page));
        if let Err(e) = mapped {
            m.unmap(base, plan.span);
            return Err(e);
        }
    }
    let relro = plan.relro.map(|(s, l)| (at(s), l));
    Ok(Loaded { base, span: plan.span, bias, phdr: at(plan.phdr), phnum: plan.phnum, entry: at(plan.entry), relro })
}

/// One reservation for `plan`: (its start, the load bias). A movable object's bias is a multiple of
/// `plan.align`: the reservation is over-sized by `align - page` and trimmed at both ends.
fn reserve<M: Mapper>(m: &mut M, plan: &Plan) -> Result<(u64, u64), MapError> {
    if plan.fixed {
        let at = m.reserve(Some(plan.first), plan.span).map_err(|()| MapError::Placement)?;
        if at != plan.first {
            m.unmap(at, plan.span);
            return Err(MapError::Placement);
        }
        return Ok((at, 0));
    }
    let extra = plan.align.wrapping_sub(plan.page);
    let len = plan.span.checked_add(extra).ok_or(MapError::BadSegments)?;
    let raw = m.reserve(None, len).map_err(|()| MapError::System)?;
    // The start congruent to `first` modulo `align`, so `base - first` (the bias) is a multiple of it.
    let base = raw.wrapping_add(plan.first.wrapping_sub(raw) & plan.align.wrapping_sub(1));
    if base > raw {
        m.unmap(raw, base.wrapping_sub(raw));
    }
    let end = base.wrapping_add(plan.span);
    let raw_end = raw.wrapping_add(len);
    if raw_end > end {
        m.unmap(end, raw_end.wrapping_sub(end));
    }
    Ok((base, base.wrapping_sub(plan.first)))
}

/// Map one segment inside the reservation (bias `bias`).
fn map_segment<M: Mapper>(m: &mut M, file: &[u8], s: &Segment, bias: u64, page: u64) -> Result<(), MapError> {
    let at = |v: u64| v.wrapping_add(bias);
    let sys = |()| MapError::System;
    let file_end = at(s.file_end);
    let tail_page = page_down(file_end, page);
    let partial = s.file_len != 0 && tail_page != file_end && s.zero_end > s.file_end;
    let mut anon_from = at(s.start);
    if s.file_len != 0 {
        let whole = page_up(s.file_len, page).ok_or(MapError::BadSegments)?;
        if partial && s.prot & PROT_WRITE == 0 {
            // Read-only `.bss` sharing the last file page: the file's whole pages, then that page
            // as zero pages (read-write only while nothing can run or read it) holding a copy of
            // the file bytes, then the segment's protection. Never writable and executable at once.
            let full = tail_page.wrapping_sub(at(s.start));
            if full != 0 {
                m.map_file(at(s.start), full, s.prot, s.offset).map_err(sys)?;
            }
            m.map_zero(tail_page, page, PROT_READ | PROT_WRITE).map_err(sys)?;
            let from = usize::try_from(s.offset.wrapping_add(full)).map_err(|_| MapError::BadSegments)?;
            let n = usize::try_from(file_end.wrapping_sub(tail_page)).map_err(|_| MapError::BadSegments)?;
            let bytes = file.get(from..from.wrapping_add(n)).ok_or(MapError::BadSegments)?;
            m.store(tail_page, page, None);
            m.store(tail_page, file_end.wrapping_sub(tail_page), Some(bytes));
            m.protect(tail_page, page, s.prot).map_err(sys)?;
        } else {
            m.map_file(at(s.start), whole, s.prot, s.offset).map_err(sys)?;
            if partial {
                // The rest of the page holding the last file byte is `.bss`: zero it.
                m.store(file_end, page.wrapping_sub(file_end.wrapping_sub(tail_page)), None);
            }
        }
        anon_from = at(s.start).wrapping_add(whole);
    }
    let anon_to = at(s.zero_end);
    if anon_to > anon_from {
        m.map_zero(anon_from, anon_to.wrapping_sub(anon_from), s.prot).map_err(sys)?;
    }
    Ok(())
}
