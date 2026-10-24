//! The call-frame unwinder behind `backtrace` and the `mtrace` caller (IP-27; spec §2.7; D-2.7).
//! Written for COLD-REWRITE2-W-far-03 from the x86-64 psABI §3.7 (DWARF register numbers,
//! figure 3.36; the CFA is the caller's stack pointer), DWARF 5 §6.4 (call frame instructions)
//! and §2.5 (expressions), and the LSB's `.eh_frame` / `.eh_frame_hdr` layout (records,
//! augmentation strings, pointer encodings). The design decisions are
//! D-2.11.
//!
//! Three layers, each a plain function of its inputs:
//! * **locate**: which object holds an address, and the FDE covering it ([`Object`]: binary
//!   search of the `.eh_frame_hdr` table, or a scan of `.eh_frame` when there is no usable
//!   table);
//! * **interpret**: the FDE's row at that address (the CIE's initial instructions, then the
//!   FDE's, stopping at the first advance past the address);
//! * **apply**: the caller's registers from the row, the current registers and checked memory
//!   reads ([`Memory`]; an unreadable word is a failed read, never a fault).
//!
//! The walker on top reports each caller's return address. A frame is looked up one byte
//! before its address (inside the call), except the frame a signal interrupted: after a frame
//! whose CIE carries `S`, the next address is the interrupted instruction itself.
//!
//! The walk ends at: an undefined or zero return address (the outermost frame), an address no
//! object describes (still reported), a row that cannot be evaluated (unreadable memory, a
//! damaged record, an unsupported operation), a step that makes no progress (same address and
//! stack pointer), or the caller's limit. The stack pointer need not grow from frame to frame:
//! a handler on an alternate signal stack sits apart from the frames it interrupted.
//!
//! Nothing allocates, nothing takes a lock and nothing is kept between calls, so a trace is
//! async-signal-safe. Working storage is on the stack and bounded (a row is 17 rules of 16
//! bytes; at most [`REMEMBER_DEPTH`] remembered rows; an expression stack of [`EXPR_STACK`]).

use core::cell::Cell;

use liella_kernel::mem::{IoVecMut, RemoteIoVec};
use liella_kernel::sys::Sys;

/// The DWARF registers the unwinder tracks: the sixteen general registers (0 to 15) and the
/// return-address column (16), psABI figure 3.36.
pub const REGS: usize = 17;
/// `%rbx`.
pub const RBX: usize = 3;
/// `%rbp`.
pub const RBP: usize = 6;
/// `%rsp`.
pub const RSP: usize = 7;
/// `%r12`.
pub const R12: usize = 12;
/// `%r13`.
pub const R13: usize = 13;
/// `%r14`.
pub const R14: usize = 14;
/// `%r15`.
pub const R15: usize = 15;
/// The return-address column.
pub const RA: usize = 16;

/// Read-only segments kept per object (an executable has two or three).
const SEGMENTS: usize = 8;
/// Nesting of `DW_CFA_remember_state` (compilers emit one level around each epilogue).
const REMEMBER_DEPTH: usize = 4;
/// Entries on an expression's stack (call-frame expressions use a handful).
const EXPR_STACK: usize = 32;
/// Operations one expression may execute (branches can loop).
const EXPR_BUDGET: usize = 1024;
/// Bytes `OwnMemory` reads at a time: an aligned line never crosses a page.
const LINE: usize = 64;

/// `DW_EH_PE_omit`.
const PE_OMIT: u8 = 0xff;

// ------------------------------------------------------------------------------ numbers

/// The bits of `v` as a signed value.
fn signed(v: u64) -> i64 {
    i64::from_ne_bytes(v.to_ne_bytes())
}

/// The bits of `v` as an unsigned value.
fn unsigned(v: i64) -> u64 {
    u64::from_ne_bytes(v.to_ne_bytes())
}

/// An address as a word.
fn word_of(v: usize) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

/// A word as an address (`None` if it does not fit, which cannot happen on x86-64).
fn addr_of(v: u64) -> Option<usize> {
    usize::try_from(v).ok()
}

// ------------------------------------------------------------------------------ inputs

/// Memory read one word at a time, where a read of an unmapped address fails instead of
/// faulting.
pub trait Memory {
    /// The 8 bytes at `addr` (little-endian), or `None` if they cannot be read.
    fn word(&self, addr: u64) -> Option<u64>;
}

/// The process's own memory through `process_vm_readv` (the kernel checks every read). It
/// reads an aligned 64-byte line at a time and keeps the last one: a trace reads neighbouring
/// words of the frames above it, which do not change while it runs. Make one per trace.
pub struct OwnMemory<'s, S: Sys> {
    sys: &'s S,
    pid: i32,
    line: Cell<Option<(u64, [u8; LINE])>>,
}

impl<'s, S: Sys> OwnMemory<'s, S> {
    /// Reads of the calling process.
    pub fn new(sys: &'s S) -> Self {
        OwnMemory { sys, pid: sys.getpid(), line: Cell::new(None) }
    }

    /// Read `buf.len()` bytes at `addr`; `true` only if all of them were read.
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        let (Some(base), len) = (addr_of(addr), buf.len()) else { return false };
        let remote = [RemoteIoVec { base, len }];
        matches!(self.sys.process_vm_readv(self.pid, &mut [IoVecMut::new(buf)], &remote, 0), Ok(n) if n == len)
    }
}

impl<S: Sys> Memory for OwnMemory<'_, S> {
    fn word(&self, addr: u64) -> Option<u64> {
        let base = addr & !word_of(LINE.wrapping_sub(1));
        let off = addr_of(addr.wrapping_sub(base))?;
        let take = |line: &[u8; LINE]| line.get(off..off.checked_add(8)?)?.try_into().ok().map(u64::from_le_bytes);
        if let Some(v) = self.line.get().filter(|(at, _)| *at == base).and_then(|(_, line)| take(&line)) {
            return Some(v);
        }
        let mut line = [0u8; LINE];
        if self.read(base, &mut line) {
            self.line.set(Some((base, line)));
            if let Some(v) = take(&line) {
                return Some(v);
            }
        }
        // A word that straddles two lines, or a line that could not be read as a whole.
        let mut w = [0u8; 8];
        self.read(addr, &mut w).then(|| u64::from_le_bytes(w))
    }
}

/// A loaded, read-only range of memory and its address.
#[derive(Clone, Copy, Debug, Default)]
pub struct Image<'a> {
    /// The address of `bytes[0]`.
    pub addr: usize,
    /// The contents.
    pub bytes: &'a [u8],
}

impl<'a> Image<'a> {
    /// The bytes from `addr` to the end of the range, if `addr` lies in it.
    fn tail(&self, addr: usize) -> Option<&'a [u8]> {
        self.bytes.get(addr.checked_sub(self.addr)?..).filter(|rest| !rest.is_empty())
    }
}

/// A loaded object: its read-only segments and where its call-frame information is.
#[derive(Clone, Copy, Debug, Default)]
pub struct Object<'a> {
    segments: [Image<'a>; SEGMENTS],
    count: usize,
    hdr: Option<usize>,
    frames: Option<(usize, usize)>,
}

impl<'a> Object<'a> {
    /// An object with nothing known yet.
    pub fn new() -> Self {
        Object::default()
    }

    /// Add a read-only segment; `false` if the object has as many as it can hold.
    pub fn add_segment(&mut self, segment: Image<'a>) -> bool {
        match self.segments.get_mut(self.count) {
            Some(slot) => {
                *slot = segment;
                self.count = self.count.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// The address of the object's `.eh_frame_hdr`.
    pub fn set_header(&mut self, addr: usize) {
        self.hdr = Some(addr);
    }

    /// The bounds of the object's `.eh_frame` (used when it has no header).
    pub fn set_frames(&mut self, start: usize, end: usize) {
        self.frames = Some((start, end));
    }

    /// Whether `addr` lies in one of the object's read-only segments.
    pub fn contains(&self, addr: usize) -> bool {
        self.bytes(addr).is_some()
    }

    /// Whether the object knows where its call-frame information is.
    pub fn has_cfi(&self) -> bool {
        self.hdr.is_some() || self.frames.is_some()
    }

    /// The bytes from `addr` to the end of the segment holding it.
    fn bytes(&self, addr: usize) -> Option<&'a [u8]> {
        self.segments.get(..self.count)?.iter().find_map(|s| s.tail(addr))
    }

    /// A cursor at `addr`.
    fn at(&self, addr: usize) -> Option<Cursor<'a>> {
        Some(Cursor { bytes: self.bytes(addr)?, pos: 0, base: addr })
    }

    /// The FDE covering `pc`.
    fn fde_for(&self, pc: usize) -> Option<Fde<'a>> {
        match (self.hdr, self.frames) {
            (Some(hdr), _) => self.search(hdr, pc),
            (None, Some((start, end))) => self.scan(start, end, pc),
            (None, None) => None,
        }
    }

    /// Look `pc` up through the `.eh_frame_hdr` at `hdr`: binary search of its table when it
    /// has one in a fixed-size encoding, otherwise a scan from the `.eh_frame` it points to.
    fn search(&self, hdr: usize, pc: usize) -> Option<Fde<'a>> {
        let mut c = self.at(hdr)?;
        let [version, frames_enc, count_enc, table_enc] = c.array::<4>()?;
        if version != 1 {
            return None;
        }
        let frames = c.pointer(frames_enc, Some(hdr))?;
        let entry = fixed_size(table_enc).and_then(|n| n.checked_mul(2));
        let (Some(entry), false) = (entry, count_enc == PE_OMIT) else {
            return self.scan(frames, usize::MAX, pc);
        };
        let count = c.pointer(count_enc, Some(hdr))?;
        let table = c.addr();
        let row = |i: usize| -> Option<(usize, usize)> {
            let mut r = self.at(table.checked_add(i.checked_mul(entry)?)?)?;
            Some((r.pointer(table_enc, Some(hdr))?, r.pointer(table_enc, Some(hdr))?))
        };
        // The last entry whose start is at or below pc.
        let (mut lo, mut hi) = (0usize, count);
        while lo < hi {
            let mid = lo.wrapping_add(hi.wrapping_sub(lo) / 2);
            if row(mid)?.0 <= pc {
                lo = mid.wrapping_add(1);
            } else {
                hi = mid;
            }
        }
        let (_, fde) = row(lo.checked_sub(1)?)?;
        self.fde(fde, None).filter(|f| f.covers(pc))
    }

    /// Scan the `.eh_frame` records from `start` (to `end`, the terminator, or the end of the
    /// segment) for the FDE covering `pc`. The last CIE parsed is kept: FDEs share a few.
    fn scan(&self, start: usize, end: usize, pc: usize) -> Option<Fde<'a>> {
        let mut at = start;
        let mut last: Option<(usize, Cie<'a>)> = None;
        while at < end {
            let r = self.record(at)?;
            if let Some(cie_at) = r.cie_at() {
                let cie = match last {
                    Some((addr, cie)) if addr == cie_at => Some(cie),
                    _ => self.cie(cie_at),
                };
                if let Some(cie) = cie {
                    last = Some((cie_at, cie));
                    if let Some(f) = Fde::parse(r, cie).filter(|f| f.covers(pc)) {
                        return Some(f);
                    }
                }
            }
            at = r.next;
        }
        None
    }

    /// The record at `addr`; `None` at the terminator or a record that does not fit.
    fn record(&self, addr: usize) -> Option<Record<'a>> {
        let mut c = self.at(addr)?;
        let len = match c.u32()? {
            0 => return None,
            0xffff_ffff => addr_of(c.u64()?)?,
            n => addr_of(u64::from(n))?,
        };
        let next = c.addr().checked_add(len)?;
        let mut body = c.sub(len)?;
        let id_at = body.addr();
        let id = body.u32()?;
        Some(Record { id, id_at, body, next })
    }

    /// The CIE at `addr`.
    fn cie(&self, addr: usize) -> Option<Cie<'a>> {
        let r = self.record(addr)?;
        if r.id != 0 {
            return None;
        }
        Cie::parse(r.body)
    }

    /// The FDE at `addr` (`cie`: its CIE when the caller already has it).
    fn fde(&self, addr: usize, cie: Option<Cie<'a>>) -> Option<Fde<'a>> {
        let r = self.record(addr)?;
        let cie = match cie {
            Some(c) => c,
            None => self.cie(r.cie_at()?)?,
        };
        Fde::parse(r, cie)
    }
}

/// Register values (DWARF numbering); `None` where a value is unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Regs([Option<u64>; REGS]);

impl Regs {
    /// No value known.
    pub fn new() -> Self {
        Regs([None; REGS])
    }

    /// The value of register `r`.
    pub fn get(&self, r: usize) -> Option<u64> {
        self.0.get(r).copied().flatten()
    }

    /// Set register `r` (ignored beyond the tracked registers).
    pub fn set(&mut self, r: usize, v: Option<u64>) {
        if let Some(slot) = self.0.get_mut(r) {
            *slot = v;
        }
    }
}

// ------------------------------------------------------------------------------ program headers

/// `PT_LOAD`.
const PT_LOAD: u64 = 1;
/// `PT_GNU_EH_FRAME`.
const PT_GNU_EH_FRAME: u64 = 0x6474_e550;
/// `PF_W`.
const PF_W: u64 = 2;
/// Words per `Elf64_Phdr`.
const PHDR_WORDS: usize = 7;

/// One program header's fields.
#[derive(Clone, Copy)]
struct Phdr {
    kind: u64,
    flags: u64,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
}

/// The first `phnum` program headers in `words` (`Elf64_Phdr`: type and flags share the
/// first word, then offset, vaddr, paddr, filesz, memsz, align).
fn phdrs(words: &[u64], phnum: usize) -> impl Iterator<Item = Phdr> + '_ {
    words.chunks_exact(PHDR_WORDS).take(phnum).map(|h| {
        let w = |i: usize| h.get(i).copied().unwrap_or(0);
        Phdr { kind: w(0) & 0xffff_ffff, flags: w(0).checked_shr(32).unwrap_or(0), offset: w(1), vaddr: w(2), filesz: w(4), memsz: w(5) }
    })
}

/// The object `phnum` program headers (`words`, loaded with bias `bias`) describe: each
/// read-only `PT_LOAD` segment, viewed through `view(address, length)`, and its
/// `PT_GNU_EH_FRAME` header if it has one.
pub fn object<'a>(words: &[u64], phnum: usize, bias: usize, view: impl Fn(usize, usize) -> &'a [u8]) -> Object<'a> {
    let mut o = Object::new();
    for p in phdrs(words, phnum) {
        let Some(addr) = addr_of(p.vaddr).map(|v| v.wrapping_add(bias)) else { continue };
        if p.kind == PT_LOAD && p.flags & PF_W == 0 {
            if let Some(len) = addr_of(p.filesz) {
                o.add_segment(Image { addr, bytes: view(addr, len) });
            }
        } else if p.kind == PT_GNU_EH_FRAME {
            o.set_header(addr);
        }
    }
    o
}

/// Whether `addr` lies in a loadable segment (of any permission) of the object `phnum` program
/// headers describe, loaded with bias `bias`.
pub fn in_object(words: &[u64], phnum: usize, bias: usize, addr: usize) -> bool {
    phdrs(words, phnum).any(|p| {
        let start = addr_of(p.vaddr).map(|v| v.wrapping_add(bias));
        let off = start.and_then(|s| addr.checked_sub(s)).map(word_of);
        p.kind == PT_LOAD && off.is_some_and(|o| o < p.memsz)
    })
}

/// The program header table of an object from its ELF header (the 64 bytes at the object's
/// start): its offset from the header and its entry count. `None` unless the header is a
/// 64-bit little-endian ELF header with 56-byte program headers.
pub fn program_headers(ehdr: &[u8]) -> Option<(usize, usize)> {
    let mut c = Cursor { bytes: ehdr, pos: 0, base: 0 };
    let ident = c.array::<16>()?;
    if ident.get(..6) != Some(b"\x7fELF\x02\x01".as_slice()) {
        return None;
    }
    c.take(16)?; // e_type, e_machine, e_version, e_entry
    let phoff = addr_of(c.u64()?)?;
    c.take(14)?; // e_shoff, e_flags, e_ehsize
    let (entsize, phnum) = (c.u16()?, c.u16()?);
    // 0xffff (PN_XNUM) keeps the real count in section header 0, which is not mapped here.
    (entsize == 56 && phnum != 0xffff).then_some((phoff, usize::from(phnum)))
}

/// The load bias of an object whose ELF header is at `ehdr`: the header lies at the start of
/// the loadable segment with file offset 0.
pub fn bias_of(words: &[u64], phnum: usize, ehdr: usize) -> Option<usize> {
    let first = phdrs(words, phnum).find(|p| p.kind == PT_LOAD && p.offset == 0)?;
    ehdr.checked_sub(addr_of(first.vaddr)?)
}

// ------------------------------------------------------------------------------ bytes

/// A cursor over bytes whose first byte is at address `base`.
#[derive(Clone, Copy, Debug)]
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Cursor<'a> {
    /// The address of the next byte.
    fn addr(&self) -> usize {
        self.base.wrapping_add(self.pos)
    }

    fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    /// The next `n` bytes as a cursor of their own.
    fn sub(&mut self, n: usize) -> Option<Cursor<'a>> {
        let base = self.addr();
        Some(Cursor { bytes: self.take(n)?, pos: 0, base })
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        Some(u8::from_le_bytes(self.array()?))
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.array()?))
    }

    /// A LEB128 number: its value bits and the shift past its last group. `None` if bits fall
    /// beyond 64.
    fn leb(&mut self) -> Option<(u64, u32, u8)> {
        let (mut v, mut shift) = (0u64, 0u32);
        loop {
            let b = self.u8()?;
            let part = u64::from(b & 0x7f);
            if shift < 64 {
                let moved = part.checked_shl(shift)?;
                if moved.checked_shr(shift)? != part {
                    return None;
                }
                v |= moved;
            } else if part != 0 {
                return None;
            }
            shift = shift.saturating_add(7);
            if b & 0x80 == 0 {
                return Some((v, shift, b));
            }
        }
    }

    fn uleb(&mut self) -> Option<u64> {
        self.leb().map(|(v, _, _)| v)
    }

    fn sleb(&mut self) -> Option<i64> {
        let (v, shift, last) = self.leb()?;
        let fill = if last & 0x40 != 0 && shift < 64 { u64::MAX.checked_shl(shift).unwrap_or(0) } else { 0 };
        Some(signed(v | fill))
    }

    /// A ULEB128 number as a `usize`.
    fn len(&mut self) -> Option<usize> {
        addr_of(self.uleb()?)
    }

    /// A NUL-terminated string (without its NUL).
    fn cstr(&mut self) -> Option<&'a [u8]> {
        let rest = self.bytes.get(self.pos..)?;
        let n = rest.iter().position(|&b| b == 0)?;
        let s = self.take(n)?;
        self.take(1)?;
        Some(s)
    }

    /// A value in the format `enc & 0x0f` of a pointer encoding (sign-extended when signed).
    fn value(&mut self, enc: u8) -> Option<u64> {
        Some(match enc & 0x0f {
            0x00 | 0x04 | 0x0c => self.u64()?,
            0x01 => self.uleb()?,
            0x02 => u64::from(self.u16()?),
            0x03 => u64::from(self.u32()?),
            0x09 => unsigned(self.sleb()?),
            0x0a => unsigned(i64::from(i16::from_le_bytes(self.array()?))),
            0x0b => unsigned(i64::from(i32::from_le_bytes(self.array()?))),
            _ => return None,
        })
    }

    /// A pointer in encoding `enc`: absolute, relative to its own address (pcrel), or to
    /// `datarel` (the `.eh_frame_hdr`). Indirect, text-, function-relative and aligned forms
    /// are not used for what the unwinder reads, so they fail.
    fn pointer(&mut self, enc: u8, datarel: Option<usize>) -> Option<usize> {
        if enc == PE_OMIT || enc & 0x80 != 0 {
            return None;
        }
        let field = self.addr();
        let v = self.value(enc)?;
        let base = match enc & 0x70 {
            0x00 => 0,
            0x10 => field,
            0x30 => datarel?,
            _ => return None,
        };
        addr_of(word_of(base).wrapping_add(v))
    }
}

/// The size of a fixed-size pointer encoding.
fn fixed_size(enc: u8) -> Option<usize> {
    match enc & 0x0f {
        _ if enc == PE_OMIT || enc & 0x80 != 0 => None,
        0x00 | 0x04 | 0x0c => Some(8),
        0x02 | 0x0a => Some(2),
        0x03 | 0x0b => Some(4),
        _ => None,
    }
}

// ------------------------------------------------------------------------------ records

/// One `.eh_frame` record: its CIE id or pointer, where that field is, its body after the
/// field, and the address of the next record.
#[derive(Clone, Copy, Debug)]
struct Record<'a> {
    id: u32,
    id_at: usize,
    body: Cursor<'a>,
    next: usize,
}

impl Record<'_> {
    /// For an FDE, its CIE's address (the pointer counts back from its own field).
    fn cie_at(&self) -> Option<usize> {
        if self.id == 0 {
            return None;
        }
        self.id_at.checked_sub(addr_of(u64::from(self.id))?)
    }
}

/// What the unwinder needs from a CIE.
#[derive(Clone, Copy, Debug)]
struct Cie<'a> {
    code_align: u64,
    data_align: i64,
    ra: usize,
    ptr_enc: u8,
    augmented: bool,
    signal: bool,
    insns: Cursor<'a>,
}

impl<'a> Cie<'a> {
    /// Parse a CIE body (after its id): versions 1 and 3 (the LSB's), 4 with 8-byte
    /// addresses; augmentation "" or "z" followed by any of `R`, `P`, `L`, `S`, `B`, `G`.
    fn parse(mut c: Cursor<'a>) -> Option<Cie<'a>> {
        let version = c.u8()?;
        let aug = c.cstr()?;
        match version {
            1 | 3 => {}
            4 => {
                if c.array::<2>()? != [8, 0] {
                    return None;
                }
            }
            _ => return None,
        }
        let code_align = c.uleb()?;
        let data_align = c.sleb()?;
        let ra = if version == 1 { u64::from(c.u8()?) } else { c.uleb()? };
        let ra = addr_of(ra).filter(|&r| r < REGS)?;
        let mut cie = Cie { code_align, data_align, ra, ptr_enc: 0, augmented: false, signal: false, insns: c };
        if let Some((&b'z', letters)) = aug.split_first() {
            cie.augmented = true;
            let n = c.len()?;
            let mut data = c.sub(n)?;
            for &l in letters {
                match l {
                    b'R' => cie.ptr_enc = data.u8()?,
                    b'L' => {
                        data.u8()?;
                    }
                    b'P' => {
                        let enc = data.u8()?;
                        data.value(enc)?;
                    }
                    b'S' => cie.signal = true,
                    b'B' | b'G' => {}
                    _ => return None,
                }
            }
        } else if !aug.is_empty() {
            return None;
        }
        cie.insns = c;
        Some(cie)
    }
}

/// An FDE: the code it covers, its CIE and its instructions.
#[derive(Clone, Copy, Debug)]
struct Fde<'a> {
    begin: usize,
    end: usize,
    cie: Cie<'a>,
    insns: Cursor<'a>,
}

impl<'a> Fde<'a> {
    fn parse(r: Record<'a>, cie: Cie<'a>) -> Option<Fde<'a>> {
        let mut c = r.body;
        let begin = c.pointer(cie.ptr_enc, None)?;
        let len = addr_of(c.value(cie.ptr_enc)?)?;
        let end = begin.checked_add(len)?;
        if cie.augmented {
            let n = c.len()?;
            c.take(n)?;
        }
        Some(Fde { begin, end, cie, insns: c })
    }

    fn covers(&self, pc: usize) -> bool {
        self.begin <= pc && pc < self.end
    }
}

/// The object holding `pc` and the FDE covering it.
fn locate<'o, 'a>(objects: &'o [Object<'a>], pc: usize) -> Option<(&'o Object<'a>, Fde<'a>)> {
    objects.iter().filter(|o| o.contains(pc)).find_map(|o| Some((o, o.fde_for(pc)?)))
}

// ------------------------------------------------------------------------------ rows

/// Bytes of an expression in an object: its address and length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    at: usize,
    len: u32,
}

impl Block {
    /// A ULEB128 length and that many bytes, from the instructions.
    fn read(c: &mut Cursor<'_>) -> Option<Block> {
        let n = c.len()?;
        let at = c.addr();
        c.take(n)?;
        Some(Block { at, len: u32::try_from(n).ok()? })
    }
}

/// How the caller's value of a register is found (DWARF 5 §6.4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// Unchanged from this frame.
    Same,
    /// Unknown.
    Undefined,
    /// Saved at CFA + n.
    At(i64),
    /// The value CFA + n.
    Val(i64),
    /// In another register of this frame.
    Reg(u8),
    /// Saved at the address the expression computes (the CFA pushed first). The block's fields
    /// are held inline so the tag shares the length's word and a rule stays 16 bytes.
    AtExpr { at: usize, len: u32 },
    /// The value the expression computes (the CFA pushed first).
    ValExpr { at: usize, len: u32 },
}

/// How the CFA is found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cfa {
    Unset,
    Reg(u8, i64),
    Expr(Block),
}

/// A row of the call-frame table: the CFA rule and a rule per tracked register.
#[derive(Clone, Copy, Debug)]
struct Row {
    cfa: Cfa,
    rules: [Rule; REGS],
}

impl Row {
    /// The rules before any instruction: registers keep their value, the stack pointer is the
    /// CFA, and the return-address column `ra` is undefined until a rule names it.
    fn new(ra: usize) -> Row {
        let mut row = Row { cfa: Cfa::Unset, rules: [Rule::Same; REGS] };
        row.set(word_of(RSP), Rule::Val(0));
        row.set(word_of(ra), Rule::Undefined);
        row
    }

    /// Set register `reg`'s rule (registers beyond the tracked ones are not kept).
    fn set(&mut self, reg: u64, rule: Rule) {
        if let Some(slot) = addr_of(reg).and_then(|r| self.rules.get_mut(r)) {
            *slot = rule;
        }
    }

    fn rule(&self, reg: u64) -> Rule {
        addr_of(reg).and_then(|r| self.rules.get(r)).copied().unwrap_or(Rule::Same)
    }
}

/// Run call-frame instructions `c` over `row`, from location `loc`, until an advance passes
/// `target` or the instructions end. `initial` is the CIE's row (for `DW_CFA_restore`), `None`
/// while running the CIE's own instructions.
fn run(row: &mut Row, initial: Option<&Row>, mut c: Cursor<'_>, cie: &Cie<'_>, mut loc: usize, target: usize) -> Option<()> {
    let mut saved = [*row; REMEMBER_DEPTH];
    let mut depth = 0usize;
    let factored = |n: u64| i64::try_from(n).ok()?.checked_mul(cie.data_align);
    let factored_sf = |n: i64| n.checked_mul(cie.data_align);
    let restore = |reg: u64| initial.map_or(Row::new(cie.ra).rule(reg), |r| r.rule(reg));
    while !c.done() {
        let op = c.u8()?;
        let low = u64::from(op & 0x3f);
        let advance = match op & 0xc0 {
            0x40 => Some(low),
            0x80 => {
                let off = factored(c.uleb()?)?;
                row.set(low, Rule::At(off));
                None
            }
            0xc0 => {
                row.set(low, restore(low));
                None
            }
            _ => match op {
                0x00 => None,
                0x01 => {
                    loc = c.pointer(cie.ptr_enc, None)?;
                    if loc > target {
                        return Some(());
                    }
                    None
                }
                0x02 => Some(u64::from(c.u8()?)),
                0x03 => Some(u64::from(c.u16()?)),
                0x04 => Some(u64::from(c.u32()?)),
                0x05 => {
                    let (reg, off) = (c.uleb()?, factored(c.uleb()?)?);
                    row.set(reg, Rule::At(off));
                    None
                }
                0x06 => {
                    let reg = c.uleb()?;
                    row.set(reg, restore(reg));
                    None
                }
                0x07 => {
                    row.set(c.uleb()?, Rule::Undefined);
                    None
                }
                0x08 => {
                    row.set(c.uleb()?, Rule::Same);
                    None
                }
                0x09 => {
                    let (reg, from) = (c.uleb()?, u8::try_from(c.uleb()?).ok()?);
                    row.set(reg, Rule::Reg(from));
                    None
                }
                0x0a => {
                    *saved.get_mut(depth)? = *row;
                    depth = depth.saturating_add(1);
                    None
                }
                0x0b => {
                    depth = depth.checked_sub(1)?;
                    *row = *saved.get(depth)?;
                    None
                }
                0x0c => {
                    let (reg, off) = (u8::try_from(c.uleb()?).ok()?, i64::try_from(c.uleb()?).ok()?);
                    row.cfa = Cfa::Reg(reg, off);
                    None
                }
                0x0d => {
                    let reg = u8::try_from(c.uleb()?).ok()?;
                    row.cfa = match row.cfa {
                        Cfa::Reg(_, off) => Cfa::Reg(reg, off),
                        Cfa::Unset => Cfa::Reg(reg, 0),
                        Cfa::Expr(_) => return None,
                    };
                    None
                }
                0x0e | 0x13 => {
                    let off = if op == 0x0e { i64::try_from(c.uleb()?).ok()? } else { factored_sf(c.sleb()?)? };
                    let Cfa::Reg(reg, _) = row.cfa else { return None };
                    row.cfa = Cfa::Reg(reg, off);
                    None
                }
                0x0f => {
                    row.cfa = Cfa::Expr(Block::read(&mut c)?);
                    None
                }
                0x10 | 0x16 => {
                    let reg = c.uleb()?;
                    let Block { at, len } = Block::read(&mut c)?;
                    row.set(reg, if op == 0x10 { Rule::AtExpr { at, len } } else { Rule::ValExpr { at, len } });
                    None
                }
                0x11 => {
                    let (reg, off) = (c.uleb()?, factored_sf(c.sleb()?)?);
                    row.set(reg, Rule::At(off));
                    None
                }
                0x12 => {
                    let (reg, off) = (u8::try_from(c.uleb()?).ok()?, factored_sf(c.sleb()?)?);
                    row.cfa = Cfa::Reg(reg, off);
                    None
                }
                0x14 => {
                    let (reg, off) = (c.uleb()?, factored(c.uleb()?)?);
                    row.set(reg, Rule::Val(off));
                    None
                }
                0x15 => {
                    let (reg, off) = (c.uleb()?, factored_sf(c.sleb()?)?);
                    row.set(reg, Rule::Val(off));
                    None
                }
                0x2e => {
                    c.uleb()?; // DW_CFA_GNU_args_size: nothing to unwind
                    None
                }
                0x2f => {
                    let (reg, off) = (c.uleb()?, factored(c.uleb()?)?);
                    row.set(reg, Rule::At(off.checked_neg()?));
                    None
                }
                _ => return None,
            },
        };
        if let Some(delta) = advance {
            loc = loc.checked_add(addr_of(delta.checked_mul(cie.code_align)?)?)?;
            if loc > target {
                return Some(());
            }
        }
    }
    Some(())
}

/// The row of `fde` at `target`.
fn row_at(fde: &Fde<'_>, target: usize) -> Option<Row> {
    let mut row = Row::new(fde.cie.ra);
    run(&mut row, None, fde.cie.insns, &fde.cie, fde.begin, usize::MAX)?;
    let initial = row;
    run(&mut row, Some(&initial), fde.insns, &fde.cie, fde.begin, target)?;
    Some(row)
}

// ------------------------------------------------------------------------------ expressions

/// A DWARF expression's stack.
struct Stack {
    v: [u64; EXPR_STACK],
    n: usize,
}

impl Stack {
    fn push(&mut self, x: u64) -> Option<()> {
        *self.v.get_mut(self.n)? = x;
        self.n = self.n.saturating_add(1);
        Some(())
    }

    fn pop(&mut self) -> Option<u64> {
        self.n = self.n.checked_sub(1)?;
        self.v.get(self.n).copied()
    }

    /// The entry `i` below the top (0 is the top).
    fn pick(&self, i: usize) -> Option<u64> {
        self.v.get(self.n.checked_sub(i)?.checked_sub(1)?).copied()
    }
}

/// Evaluate the expression `b` of `obj` (DWARF 5 §2.5) over `regs` and `mem`; `cfa`, when
/// given, is pushed first and is what `DW_OP_call_frame_cfa` pushes. Arithmetic wraps;
/// division is signed and fails on zero or overflow; comparisons are signed.
fn eval<M: Memory>(obj: &Object<'_>, b: Block, regs: &Regs, mem: &M, cfa: Option<u64>) -> Option<u64> {
    let code = obj.bytes(b.at)?.get(..addr_of(u64::from(b.len))?)?;
    let mut s = Stack { v: [0; EXPR_STACK], n: 0 };
    if let Some(cfa) = cfa {
        s.push(cfa)?;
    }
    let mut c = Cursor { bytes: code, pos: 0, base: b.at };
    for _ in 0..EXPR_BUDGET {
        if c.done() {
            return s.pop();
        }
        let op = c.u8()?;
        let two = |s: &mut Stack, f: &dyn Fn(u64, u64) -> Option<u64>| -> Option<()> {
            let (top, below) = (s.pop()?, s.pop()?);
            s.push(f(below, top)?)
        };
        match op {
            0x03 => s.push(c.u64()?)?,
            0x06 => {
                let a = s.pop()?;
                s.push(mem.word(a)?)?;
            }
            0x08 => s.push(u64::from(c.u8()?))?,
            0x09 => s.push(unsigned(i64::from(i8::from_le_bytes(c.array()?))))?,
            0x0a => s.push(c.value(0x02)?)?,
            0x0b => s.push(c.value(0x0a)?)?,
            0x0c => s.push(c.value(0x03)?)?,
            0x0d => s.push(c.value(0x0b)?)?,
            0x0e | 0x0f => s.push(c.u64()?)?,
            0x10 => s.push(c.uleb()?)?,
            0x11 => s.push(unsigned(c.sleb()?))?,
            0x12 => s.push(s.pick(0)?)?,
            0x13 => {
                s.pop()?;
            }
            0x14 => s.push(s.pick(1)?)?,
            0x15 => {
                let i = usize::from(c.u8()?);
                s.push(s.pick(i)?)?;
            }
            0x16 => {
                let (a, b) = (s.pop()?, s.pop()?);
                s.push(a)?;
                s.push(b)?;
            }
            0x17 => {
                // The top becomes the third entry; the second and third move up.
                let (a, b, c3) = (s.pop()?, s.pop()?, s.pop()?);
                s.push(a)?;
                s.push(c3)?;
                s.push(b)?;
            }
            0x19 => {
                let a = s.pop()?;
                s.push(signed(a).unsigned_abs())?;
            }
            0x1a => two(&mut s, &|x, y| Some(x & y))?,
            0x1b => two(&mut s, &|x, y| signed(x).checked_div(signed(y)).map(unsigned))?,
            0x1c => two(&mut s, &|x, y| Some(x.wrapping_sub(y)))?,
            0x1d => two(&mut s, &|x, y| x.checked_rem(y))?,
            0x1e => two(&mut s, &|x, y| Some(x.wrapping_mul(y)))?,
            0x1f => {
                let a = s.pop()?;
                s.push(a.wrapping_neg())?;
            }
            0x20 => {
                let a = s.pop()?;
                s.push(!a)?;
            }
            0x21 => two(&mut s, &|x, y| Some(x | y))?,
            0x22 => two(&mut s, &|x, y| Some(x.wrapping_add(y)))?,
            0x23 => {
                let (a, n) = (s.pop()?, c.uleb()?);
                s.push(a.wrapping_add(n))?;
            }
            0x24 => two(&mut s, &|x, y| Some(u32::try_from(y).ok().and_then(|y| x.checked_shl(y)).unwrap_or(0)))?,
            0x25 => two(&mut s, &|x, y| Some(u32::try_from(y).ok().and_then(|y| x.checked_shr(y)).unwrap_or(0)))?,
            0x26 => two(&mut s, &|x, y| {
                let fill = if signed(x) < 0 { u64::MAX } else { 0 };
                Some(u32::try_from(y).ok().and_then(|y| signed(x).checked_shr(y)).map_or(fill, unsigned))
            })?,
            0x27 => two(&mut s, &|x, y| Some(x ^ y))?,
            0x29..=0x2e => {
                let (y, x) = (signed(s.pop()?), signed(s.pop()?));
                let r = match op {
                    0x29 => x == y,
                    0x2a => x >= y,
                    0x2b => x > y,
                    0x2c => x <= y,
                    0x2d => x < y,
                    _ => x != y,
                };
                s.push(u64::from(r))?;
            }
            0x28 | 0x2f => {
                let off = i16::from_le_bytes(c.array()?);
                if op == 0x2f || s.pop()? != 0 {
                    let to = isize::try_from(c.pos).ok()?.checked_add(isize::from(off))?;
                    c.pos = usize::try_from(to).ok().filter(|&p| p <= code.len())?;
                }
            }
            0x30..=0x4f => s.push(u64::from(op.wrapping_sub(0x30)))?,
            0x70..=0x8f => {
                let r = regs.get(usize::from(op.wrapping_sub(0x70)))?;
                s.push(r.wrapping_add(unsigned(c.sleb()?)))?;
            }
            0x92 => {
                let r = regs.get(addr_of(c.uleb()?)?)?;
                s.push(r.wrapping_add(unsigned(c.sleb()?)))?;
            }
            0x94 => {
                let n = u32::from(c.u8()?);
                if !(1..=8).contains(&n) {
                    return None;
                }
                let a = s.pop()?;
                let mask = u64::MAX.checked_shr(64u32.wrapping_sub(n.wrapping_mul(8))).unwrap_or(u64::MAX);
                s.push(mem.word(a)? & mask)?;
            }
            0x96 => {}
            0x9c => s.push(cfa?)?,
            _ => return None,
        }
    }
    None
}

// ------------------------------------------------------------------------------ frames

/// The caller's registers from the frame whose registers are `regs`, described by `fde` at
/// `look` (in `obj`): the caller's return address in `RA`.
fn step<M: Memory>(obj: &Object<'_>, fde: &Fde<'_>, regs: &Regs, mem: &M, look: usize) -> Option<Regs> {
    let row = row_at(fde, look)?;
    let cfa = match row.cfa {
        Cfa::Unset => return None,
        Cfa::Reg(reg, off) => regs.get(usize::from(reg))?.checked_add_signed(off)?,
        Cfa::Expr(b) => eval(obj, b, regs, mem, None)?,
    };
    let mut next = Regs::new();
    for (r, rule) in row.rules.iter().enumerate() {
        let v = match *rule {
            Rule::Same => regs.get(r),
            Rule::Undefined => None,
            Rule::At(off) => cfa.checked_add_signed(off).and_then(|a| mem.word(a)),
            Rule::Val(off) => cfa.checked_add_signed(off),
            Rule::Reg(from) => regs.get(usize::from(from)),
            Rule::AtExpr { at, len } => eval(obj, Block { at, len }, regs, mem, Some(cfa)).and_then(|a| mem.word(a)),
            Rule::ValExpr { at, len } => eval(obj, Block { at, len }, regs, mem, Some(cfa)),
        };
        next.set(r, v);
    }
    if fde.cie.ra != RA {
        next.set(RA, next.get(fde.cie.ra));
    }
    Some(next)
}

/// Walk the frames above the one `start` describes (its `RA` is its own address, which is not
/// visited). `visit(address, function start)` is called for each caller, innermost first,
/// with the start of the function covering it (`None` where no object describes it, which
/// ends the walk); it returns whether to go on.
fn walk<M: Memory>(objects: &[Object<'_>], mem: &M, start: Regs, mut visit: impl FnMut(usize, Option<usize>) -> bool) {
    let mut regs = start;
    let mut exact = false;
    let mut first = true;
    loop {
        let Some(pc) = regs.get(RA).filter(|&p| p != 0).and_then(addr_of) else { return };
        let look = if exact { pc } else { pc.wrapping_sub(1) };
        let found = locate(objects, look);
        if !first && !visit(pc, found.map(|(_, f)| f.begin)) {
            return;
        }
        first = false;
        let Some((obj, fde)) = found else { return };
        let Some(next) = step(obj, &fde, &regs, mem, look) else { return };
        if next.get(RA) == regs.get(RA) && next.get(RSP) == regs.get(RSP) {
            return; // no progress
        }
        exact = fde.cie.signal;
        regs = next;
    }
}

/// Unwind from the registers `start` (captured inside the caller of `trace`, whose own frame
/// is not reported) and pass each return address to `store`, innermost first, at most `max`.
/// The first frame reported is left out when its function starts at `entry` (the exported
/// entry point's own frame; 0 for none). Returns the number stored.
pub fn trace<M: Memory>(objects: &[Object<'_>], mem: &M, start: Regs, entry: usize, max: usize, mut store: impl FnMut(usize)) -> usize {
    let (mut n, mut first) = (0usize, true);
    if max == 0 {
        return 0;
    }
    walk(objects, mem, start, |pc, begin| {
        let skip = first && entry != 0 && begin == Some(entry);
        first = false;
        if !skip {
            store(pc);
            n = n.saturating_add(1);
        }
        n < max
    });
    n
}

/// The return address into the first frame above `start` whose function does not start at
/// one of `inside`, looking at most `max` frames; `None` if there is none within them.
pub fn caller<M: Memory>(objects: &[Object<'_>], mem: &M, start: Regs, inside: &[usize], max: usize) -> Option<usize> {
    let (mut seen, mut found) = (0usize, None);
    walk(objects, mem, start, |pc, begin| {
        if seen >= max {
            return false;
        }
        seen = seen.saturating_add(1);
        if begin.is_some_and(|b| inside.contains(&b)) {
            return true;
        }
        found = Some(pc);
        false
    });
    found
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    struct NoMemory;

    impl Memory for NoMemory {
        fn word(&self, _: u64) -> Option<u64> {
            None
        }
    }

    fn cursor(bytes: &[u8]) -> Cursor<'_> {
        Cursor { bytes, pos: 0, base: 0x1000 }
    }

    #[test]
    fn leb128() {
        let mut c = cursor(&[0xe5, 0x8e, 0x26, 0x7f, 0x80, 0x7f, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02]);
        assert_eq!(c.uleb(), Some(624_485));
        assert_eq!(c.sleb(), Some(-1));
        assert_eq!(c.sleb(), Some(-128));
        // Ten groups whose last carries bits beyond 64.
        assert_eq!(c.uleb(), None);
        assert_eq!(cursor(&[]).uleb(), None);
        assert_eq!(cursor(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]).uleb(), Some(u64::MAX));
    }

    #[test]
    fn pointers() {
        // pcrel sdata4 -4 at 0x1000 -> 0xffc; datarel udata2; omit; indirect; textrel.
        assert_eq!(cursor(&[0xfc, 0xff, 0xff, 0xff]).pointer(0x1b, None), Some(0xffc));
        assert_eq!(cursor(&[0x10, 0]).pointer(0x32, Some(0x5000)), Some(0x5010));
        assert_eq!(cursor(&[0x10, 0]).pointer(0x32, None), None);
        assert_eq!(cursor(&[0; 8]).pointer(PE_OMIT, None), None);
        assert_eq!(cursor(&[0; 8]).pointer(0x9b, None), None);
        assert_eq!(cursor(&[0; 8]).pointer(0x20, None), None);
        assert_eq!((fixed_size(0x3b), fixed_size(0x01), fixed_size(0x0a)), (Some(4), None, Some(2)));
    }

    /// Evaluate `code` as if it were an object's bytes at 0x1000.
    fn run_expr(code: &[u8], regs: &Regs) -> Option<u64> {
        let mut o = Object::new();
        o.add_segment(Image { addr: 0x1000, bytes: code });
        eval(&o, Block { at: 0x1000, len: u32::try_from(code.len()).unwrap() }, regs, &NoMemory, Some(0x40))
    }

    #[test]
    fn stack_operations() {
        let r = Regs::new();
        // (CFA) lit1 lit2 lit3 rot -> CFA 3 1 2: the top is 2, then 1, then 3.
        assert_eq!(run_expr(&[0x31, 0x32, 0x33, 0x17], &r), Some(2));
        assert_eq!(run_expr(&[0x31, 0x32, 0x33, 0x17, 0x13], &r), Some(1));
        assert_eq!(run_expr(&[0x31, 0x32, 0x33, 0x17, 0x13, 0x13], &r), Some(3));
        // pick 3 reaches the pushed CFA; pick 4 is below the stack.
        assert_eq!(run_expr(&[0x31, 0x32, 0x33, 0x15, 3], &r), Some(0x40));
        assert_eq!(run_expr(&[0x31, 0x32, 0x33, 0x15, 4], &r), None);
        // call_frame_cfa; shifts past the width; deref_size of an unreadable word.
        assert_eq!(run_expr(&[0x9c], &r), Some(0x40));
        assert_eq!(run_expr(&[0x31, 0x08, 64, 0x24], &r), Some(0));
        assert_eq!(run_expr(&[0x09, 0xff, 0x08, 70, 0x26], &r), Some(u64::MAX));
        assert_eq!(run_expr(&[0x94, 4], &r), None);
        assert_eq!(run_expr(&[0x31, 0x94, 9], &r), None);
        // A full stack fails rather than overwriting.
        assert_eq!(run_expr(&[0x30; EXPR_STACK], &r), None);
    }

    #[test]
    fn remember_depth_is_bounded() {
        let cie = Cie { code_align: 1, data_align: -8, ra: RA, ptr_enc: 0x1b, augmented: true, signal: false, insns: cursor(&[]) };
        let mut row = Row::new(RA);
        let deep = [0x0a; REMEMBER_DEPTH + 1];
        assert!(run(&mut row, None, cursor(&deep[..REMEMBER_DEPTH]), &cie, 0, 10).is_some());
        assert!(run(&mut row, None, cursor(&deep), &cie, 0, 10).is_none());
        assert!(run(&mut row, None, cursor(&[0x0b]), &cie, 0, 10).is_none());
        // Rules for registers beyond the tracked ones are read and dropped.
        assert!(run(&mut row, None, cursor(&[0x05, 40, 2, 0x10, 33, 1, 0x96]), &cie, 0, 10).is_some());
        assert_eq!(row.rule(40), Rule::Same);
        assert_eq!(size_of::<Rule>(), 16);
    }

    /// A CIE for row tests: code factor 1, data factor `data`, return address column 16,
    /// absolute 8-byte pointers (for `DW_CFA_set_loc`).
    fn row_cie(data: i64) -> Cie<'static> {
        Cie { code_align: 1, data_align: data, ra: RA, ptr_enc: 0x00, augmented: true, signal: false, insns: cursor(&[]) }
    }

    /// The row at `target` after the CIE instructions `initial` and the FDE instructions `fde`
    /// (both starting at location 0).
    fn row_of(cie: &Cie<'_>, initial: &[u8], fde: &[u8], target: usize) -> Option<Row> {
        let mut row = Row::new(cie.ra);
        run(&mut row, None, cursor(initial), cie, 0, usize::MAX)?;
        let first = row;
        run(&mut row, Some(&first), cursor(fde), cie, 0, target)?;
        Some(row)
    }

    #[test]
    fn data_factor_scales_every_factored_offset() {
        // Factor -4: offset rbx 2 -> -8; offset_extended_sf rbx -2 -> 8; GNU negative offset 2 -> 8.
        let cie = row_cie(-4);
        assert_eq!(row_of(&cie, &[], &[0x83, 2], 0).unwrap().rule(3), Rule::At(-8));
        assert_eq!(row_of(&cie, &[], &[0x11, 3, 0x7e], 0).unwrap().rule(3), Rule::At(8));
        assert_eq!(row_of(&cie, &[], &[0x2f, 3, 2], 0).unwrap().rule(3), Rule::At(8));
        assert_eq!(row_of(&row_cie(-8), &[], &[0x2f, 3, 2], 0).unwrap().rule(3), Rule::At(16));
    }

    #[test]
    fn restore_returns_to_the_cie_rule() {
        // The CIE saves rbx at CFA - 16; the FDE marks it unchanged, then restores it (short
        // and extended forms) to the CIE's rule, not to "unchanged".
        let cie = row_cie(-8);
        assert_eq!(row_of(&cie, &[0x83, 2], &[0x08, 3, 0xc3], 0).unwrap().rule(3), Rule::At(-16));
        assert_eq!(row_of(&cie, &[0x83, 2], &[0x08, 3, 0x06, 3], 0).unwrap().rule(3), Rule::At(-16));
        assert_eq!(row_of(&cie, &[], &[0x83, 2, 0xc3], 0).unwrap().rule(3), Rule::Same);
    }

    #[test]
    fn remembered_rows_nest() {
        // Two levels: the inner restore brings back the rule set between the remembers.
        let cie = row_cie(-8);
        let fde = [0x0a, 0x83, 1, 0x0a, 0x83, 2, 0x0b, 0x0b];
        assert_eq!(row_of(&cie, &[], &fde[..7], 0).unwrap().rule(3), Rule::At(-8));
        assert_eq!(row_of(&cie, &[], &fde, 0).unwrap().rule(3), Rule::Same);
    }

    #[test]
    fn set_loc_applies_at_its_own_address() {
        // set_loc 0x10, then rbx at CFA - 8: in force at 0x10, not before.
        let cie = row_cie(-8);
        let fde = [0x01, 0x10, 0, 0, 0, 0, 0, 0, 0, 0x83, 1];
        assert_eq!(row_of(&cie, &[], &fde, 0x10).unwrap().rule(3), Rule::At(-8));
        assert_eq!(row_of(&cie, &[], &fde, 0x0f).unwrap().rule(3), Rule::Same);
    }

    #[test]
    fn leb128_groups_past_64_bits_must_be_empty() {
        // An eleventh group starts at bit 70: zero bits there are padding, any other bit
        // does not fit.
        let mut long = [0x80u8; 11];
        long[10] = 0x01;
        assert_eq!(cursor(&long).uleb(), None);
        long[10] = 0x00;
        assert_eq!(cursor(&long).uleb(), Some(0));
    }

    #[test]
    fn two_byte_signed_values_are_sign_extended() {
        // pcrel sdata2 -4 at 0x1000 -> 0xffc; sdata2 0x8000 is -32768.
        assert_eq!(cursor(&[0xfc, 0xff]).pointer(0x1a, None), Some(0xffc));
        assert_eq!(cursor(&[0x00, 0x80]).value(0x0a), Some(unsigned(-32_768)));
        assert_eq!(cursor(&[0x00, 0x80]).value(0x02), Some(0x8000));
    }

    #[test]
    fn expression_arithmetic_and_comparisons() {
        let r = Regs::new();
        // mod is unsigned: (2^64 - 7) mod 2 = 1 (signed it would be -1); mod 0 fails.
        assert_eq!(run_expr(&[0x09, 0xf9, 0x32, 0x1d], &r), Some(1));
        assert_eq!(run_expr(&[0x31, 0x30, 0x1d], &r), None);
        // Comparisons pop the right operand first and compare signed: 1 op 2, then -1 op 1.
        let ops = [(0x29, 0, 0), (0x2a, 0, 0), (0x2b, 0, 0), (0x2c, 1, 1), (0x2d, 1, 1), (0x2e, 1, 1)];
        for (op, small_big, minus_one_one) in ops {
            assert_eq!(run_expr(&[0x31, 0x32, op], &r), Some(small_big), "op {op:#x} on 1, 2");
            assert_eq!(run_expr(&[0x09, 0xff, 0x31, op], &r), Some(minus_one_one), "op {op:#x} on -1, 1");
        }
    }

    #[test]
    fn expression_branches() {
        let r = Regs::new();
        // bra pops and branches on non-zero: lit1 bra +1 skips lit3 (the pushed CFA is left on
        // top); lit0 bra +1 falls through to lit3.
        assert_eq!(run_expr(&[0x31, 0x28, 1, 0, 0x33], &r), Some(0x40));
        assert_eq!(run_expr(&[0x30, 0x28, 1, 0, 0x33], &r), Some(3));
        // A branch may land on the end of the block, not past it or before its start.
        assert_eq!(run_expr(&[0x2f, 0, 0], &r), Some(0x40));
        assert_eq!(run_expr(&[0x2f, 1, 0], &r), None);
        assert_eq!(run_expr(&[0x2f, 5, 0], &r), None);
        assert_eq!(run_expr(&[0x2f, 0xfc, 0xff], &r), None);
    }
}
