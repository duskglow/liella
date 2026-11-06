//! ELF64 reader, part 1 (IP-24, plan/ph14/design.md §2): the ELF header, the program headers, the
//! dynamic section and notes. Part 2 (DL-3) adds hash tables, symbol versions and relocation iterators.
//!
//! The reader works on a byte slice (a file prefix, a whole file or a mapped image) and does no I/O.
//! Every offset, count and size from the object is checked with checked arithmetic against the slice
//! before use (threat-model rule 4): a malformed object gives an [`ElfError`], never a panic and never a
//! read outside the slice. Nothing here allocates. The error rules are decision D-NEW (plan/07-decisions/s26-29.md).
//!
//! The ELF identity (class, byte order, machine) is checked only through [`Arch`] (design §0a).

use crate::arch::{Arch, ELFCLASS64, ELFDATA2MSB};

/// Size of the ELF64 file header.
pub const EHDR_SIZE: usize = 64;
/// Size of one ELF64 program header.
pub const PHDR_SIZE: usize = 56;
/// Size of one ELF64 dynamic entry.
pub const DYN_SIZE: usize = 16;
/// `e_phnum` value that means "the count is in section header 0" (`PN_XNUM`); refused.
pub const PN_XNUM: u16 = 0xffff;

/// Object file types (`e_type`) the reader accepts.
pub mod et {
    /// Executable file.
    pub const EXEC: u16 = 2;
    /// Shared object (or position-independent executable).
    pub const DYN: u16 = 3;
}

/// `EI_OSABI` values the reader accepts.
pub mod osabi {
    /// `ELFOSABI_NONE` / System V.
    pub const SYSV: u8 = 0;
    /// `ELFOSABI_GNU` (objects using GNU extensions such as `STT_GNU_IFUNC`).
    pub const GNU: u8 = 3;
}

/// Program header types (`p_type`).
pub mod pt {
    /// Loadable segment.
    pub const LOAD: u32 = 1;
    /// Dynamic section.
    pub const DYNAMIC: u32 = 2;
    /// Program interpreter path.
    pub const INTERP: u32 = 3;
    /// Notes.
    pub const NOTE: u32 = 4;
    /// The program header table itself.
    pub const PHDR: u32 = 6;
    /// Thread-local storage template.
    pub const TLS: u32 = 7;
    /// `.eh_frame_hdr`.
    pub const GNU_EH_FRAME: u32 = 0x6474_e550;
    /// Stack executability.
    pub const GNU_STACK: u32 = 0x6474_e551;
    /// Read-only after relocation.
    pub const GNU_RELRO: u32 = 0x6474_e552;
    /// `.note.gnu.property`.
    pub const GNU_PROPERTY: u32 = 0x6474_e553;
}

/// Dynamic tags (`d_tag`) the reader itself interprets.
pub mod dt {
    /// End of the dynamic array.
    pub const NULL: u64 = 0;
    /// Name of a needed library (string table offset).
    pub const NEEDED: u64 = 1;
    /// Address of the dynamic string table.
    pub const STRTAB: u64 = 5;
    /// Size of the dynamic string table in bytes.
    pub const STRSZ: u64 = 10;
    /// The object's soname (string table offset).
    pub const SONAME: u64 = 14;
    /// Library search path (string table offset).
    pub const RPATH: u64 = 15;
    /// Library search path (string table offset).
    pub const RUNPATH: u64 = 29;
}

/// Note types of the `GNU` owner.
pub mod nt {
    /// `NT_GNU_ABI_TAG`: OS and minimum kernel version.
    pub const GNU_ABI_TAG: u32 = 1;
    /// `NT_GNU_BUILD_ID`.
    pub const GNU_BUILD_ID: u32 = 3;
    /// `NT_GNU_PROPERTY_TYPE_0`: an array of program properties.
    pub const GNU_PROPERTY_TYPE_0: u32 = 5;
}

/// Which file range an out-of-range or overlap error is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The program header table.
    ProgramHeaders,
    /// The file range of the program header with this index.
    Segment(u16),
}

/// Why an object was refused. Every variant is a property of the bytes, so the same input always
/// gives the same error (decision D-NEW).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    /// The slice is shorter than the ELF header.
    TooShort,
    /// The first four bytes are not `\x7fELF`.
    BadMagic,
    /// `EI_CLASS` is not the architecture's class (or the architecture's class is not 64-bit).
    WrongClass(u8),
    /// `EI_DATA` is not the architecture's byte order.
    WrongData(u8),
    /// `EI_VERSION` is not 1 (`EV_CURRENT`).
    BadIdentVersion(u8),
    /// `EI_OSABI` is neither System V nor GNU.
    UnsupportedOsAbi(u8),
    /// `e_type` is neither `ET_EXEC` nor `ET_DYN`.
    UnsupportedType(u16),
    /// `e_machine` is not the architecture's machine.
    WrongMachine(u16),
    /// `e_version` is not 1.
    BadVersion(u32),
    /// `e_ehsize` is not the ELF64 header size.
    BadEhsize(u16),
    /// `e_phentsize` is not the ELF64 program header size (while `e_phnum` is not 0).
    BadPhentsize(u16),
    /// `e_phnum` is `PN_XNUM` (the count would come from the section headers, which the reader does not use).
    PhnumExtended,
    /// A file range lies (partly) outside the slice, or its end overflows.
    OutOfRange(Part),
    /// A file range overlaps the ELF header or the program header table.
    OverlapsHeaders(Part),
    /// `p_offset + p_filesz` or `p_vaddr + p_memsz` overflows.
    SegmentOverflow(u16),
    /// A `PT_LOAD` segment has `p_filesz > p_memsz`.
    FileszExceedsMemsz(u16),
    /// `p_align` is neither 0 nor a power of two.
    BadAlign(u16),
    /// A second program header of a type that may occur once (`PT_DYNAMIC`, `PT_INTERP`).
    DuplicateSegment(u32),
    /// The `PT_INTERP` bytes are empty, do not end in NUL, or hold a NUL before the end.
    BadInterp,
    /// The `PT_DYNAMIC` size is not a multiple of the entry size.
    DynamicSize,
    /// No `DT_NULL` entry within the `PT_DYNAMIC` range.
    DynamicUnterminated,
    /// A second `DT_STRTAB` or `DT_STRSZ` entry.
    DuplicateDynamic(u64),
    /// A string-valued entry (or `DT_STRSZ`) without `DT_STRTAB`.
    MissingStrtab,
    /// `DT_STRTAB` without `DT_STRSZ`.
    MissingStrsz,
    /// An address range is not wholly inside the file-backed part of one `PT_LOAD` segment.
    AddressNotMapped(u64),
    /// A string offset is at or past the end of the string table.
    StringOutOfRange(u64),
    /// No NUL between a string offset and the end of the string table.
    StringUnterminated(u64),
    /// A `PT_NOTE` segment's `p_align` is not 0, 1, 2, 4 or 8.
    BadNoteAlign(u16),
    /// A note header, name or descriptor runs past the end of its segment (offset of the note in the segment).
    NoteTruncated(usize),
    /// A note name of nonzero size that does not end in NUL (offset of the note in the segment).
    BadNoteName(usize),
    /// A program property runs past the end of its note descriptor (offset of the property in the descriptor).
    PropertyTruncated(usize),
}

/// Reads fixed-size integers from a slice in the object's byte order. `None` means out of range.
#[derive(Clone, Copy, Debug)]
struct Rd<'a> {
    b: &'a [u8],
    be: bool,
}

impl<'a> Rd<'a> {
    fn bytes<const N: usize>(&self, off: usize) -> Option<[u8; N]> {
        let end = off.checked_add(N)?;
        self.b.get(off..end)?.try_into().ok()
    }
    fn u16(&self, off: usize) -> Option<u16> {
        let a = self.bytes::<2>(off)?;
        Some(if self.be { u16::from_be_bytes(a) } else { u16::from_le_bytes(a) })
    }
    fn u32(&self, off: usize) -> Option<u32> {
        let a = self.bytes::<4>(off)?;
        Some(if self.be { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) })
    }
    fn u64(&self, off: usize) -> Option<u64> {
        let a = self.bytes::<8>(off)?;
        Some(if self.be { u64::from_be_bytes(a) } else { u64::from_le_bytes(a) })
    }
}

/// `b[off..off+len]` with every step checked (`None`: outside the slice or overflow).
fn sub(b: &[u8], off: u64, len: u64) -> Option<&[u8]> {
    let off = usize::try_from(off).ok()?;
    let len = usize::try_from(len).ok()?;
    b.get(off..off.checked_add(len)?)
}

/// Whether `[a, a+alen)` and `[b, b+blen)` overlap (empty ranges overlap nothing; ends saturate).
fn overlaps(a: u64, alen: u64, b: u64, blen: u64) -> bool {
    alen != 0 && blen != 0 && a < b.saturating_add(blen) && b < a.saturating_add(alen)
}

/// The ELF64 file header, decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// `EI_CLASS`.
    pub class: u8,
    /// `EI_DATA`.
    pub data: u8,
    /// `EI_VERSION`.
    pub ident_version: u8,
    /// `EI_OSABI`.
    pub osabi: u8,
    /// `EI_ABIVERSION`.
    pub abi_version: u8,
    /// `e_type`.
    pub e_type: u16,
    /// `e_machine`.
    pub machine: u16,
    /// `e_version`.
    pub version: u32,
    /// `e_entry`.
    pub entry: u64,
    /// `e_phoff`.
    pub phoff: u64,
    /// `e_shoff` (reported, not used or checked).
    pub shoff: u64,
    /// `e_flags`.
    pub flags: u32,
    /// `e_ehsize`.
    pub ehsize: u16,
    /// `e_phentsize`.
    pub phentsize: u16,
    /// `e_phnum`.
    pub phnum: u16,
    /// `e_shentsize` (reported, not used or checked).
    pub shentsize: u16,
    /// `e_shnum` (reported, not used or checked).
    pub shnum: u16,
    /// `e_shstrndx` (reported, not used or checked).
    pub shstrndx: u16,
}

/// One ELF64 program header, decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramHeader {
    /// Index in the program header table.
    pub index: u16,
    /// `p_type`.
    pub p_type: u32,
    /// `p_flags` (`PF_X` 1, `PF_W` 2, `PF_R` 4).
    pub flags: u32,
    /// `p_offset`.
    pub offset: u64,
    /// `p_vaddr`.
    pub vaddr: u64,
    /// `p_paddr`.
    pub paddr: u64,
    /// `p_filesz`.
    pub filesz: u64,
    /// `p_memsz`.
    pub memsz: u64,
    /// `p_align`.
    pub align: u64,
}

/// A parsed object: the header and the program header table are checked; the contents of segments
/// are checked when they are read (the slice may be only a prefix of the file).
#[derive(Clone, Copy, Debug)]
pub struct Elf<'a> {
    rd: Rd<'a>,
    header: Header,
}

impl<'a> Elf<'a> {
    /// Checks the ELF header against architecture `A` and every program header, without reading
    /// any segment's contents.
    ///
    /// # Errors
    ///
    /// The first rule the bytes break, in the order of decision D-NEW: identity, header fields, the
    /// program header table's range, then each program header in index order.
    pub fn parse<A: Arch>(bytes: &'a [u8]) -> Result<Self, ElfError> {
        let ident = bytes.get(..16).ok_or(ElfError::TooShort)?;
        if bytes.len() < EHDR_SIZE {
            return Err(ElfError::TooShort);
        }
        let (magic, class, data, ident_version, osabi, abi_version) = match ident {
            [m0, m1, m2, m3, c, d, v, o, a, ..] => ([*m0, *m1, *m2, *m3], *c, *d, *v, *o, *a),
            _ => return Err(ElfError::TooShort),
        };
        if magic != *b"\x7fELF" {
            return Err(ElfError::BadMagic);
        }
        if class != A::EI_CLASS || class != ELFCLASS64 {
            return Err(ElfError::WrongClass(class));
        }
        if data != A::EI_DATA {
            return Err(ElfError::WrongData(data));
        }
        if ident_version != 1 {
            return Err(ElfError::BadIdentVersion(ident_version));
        }
        if osabi != osabi::SYSV && osabi != osabi::GNU {
            return Err(ElfError::UnsupportedOsAbi(osabi));
        }
        let rd = Rd { b: bytes, be: data == ELFDATA2MSB };
        let h = read_header(&rd, [class, data, ident_version, osabi, abi_version]).ok_or(ElfError::TooShort)?;
        if h.e_type != et::EXEC && h.e_type != et::DYN {
            return Err(ElfError::UnsupportedType(h.e_type));
        }
        if h.machine != A::E_MACHINE {
            return Err(ElfError::WrongMachine(h.machine));
        }
        if h.version != 1 {
            return Err(ElfError::BadVersion(h.version));
        }
        if usize::from(h.ehsize) != EHDR_SIZE {
            return Err(ElfError::BadEhsize(h.ehsize));
        }
        if h.phnum == PN_XNUM {
            return Err(ElfError::PhnumExtended);
        }
        if h.phnum != 0 && usize::from(h.phentsize) != PHDR_SIZE {
            return Err(ElfError::BadPhentsize(h.phentsize));
        }
        let elf = Elf { rd, header: h };
        let table = elf.table_len();
        if sub(bytes, h.phoff, table).is_none() {
            return Err(ElfError::OutOfRange(Part::ProgramHeaders));
        }
        if overlaps(h.phoff, table, 0, ehdr_len()) {
            return Err(ElfError::OverlapsHeaders(Part::ProgramHeaders));
        }
        let mut seen_dynamic = false;
        let mut seen_interp = false;
        for i in 0..h.phnum {
            let ph = elf.read_phdr(i).ok_or(ElfError::OutOfRange(Part::ProgramHeaders))?;
            elf.check_phdr(&ph)?;
            let seen = match ph.p_type {
                pt::DYNAMIC => Some(&mut seen_dynamic),
                pt::INTERP => Some(&mut seen_interp),
                _ => None,
            };
            if let Some(seen) = seen {
                if *seen {
                    return Err(ElfError::DuplicateSegment(ph.p_type));
                }
                *seen = true;
            }
        }
        Ok(elf)
    }

    /// The decoded ELF header.
    #[must_use]
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Length in bytes of the program header table (`e_phnum` entries).
    fn table_len(&self) -> u64 {
        // PHDR_SIZE * u16::MAX fits in u64; saturating anyway so no arithmetic can wrap.
        u64::from(self.header.phnum).saturating_mul(phdr_len())
    }

    fn read_phdr(&self, i: u16) -> Option<ProgramHeader> {
        let rel = usize::from(i).checked_mul(PHDR_SIZE)?;
        let base = usize::try_from(self.header.phoff).ok()?.checked_add(rel)?;
        let at = |o: usize| base.checked_add(o);
        Some(ProgramHeader {
            index: i,
            p_type: self.rd.u32(base)?,
            flags: self.rd.u32(at(4)?)?,
            offset: self.rd.u64(at(8)?)?,
            vaddr: self.rd.u64(at(16)?)?,
            paddr: self.rd.u64(at(24)?)?,
            filesz: self.rd.u64(at(32)?)?,
            memsz: self.rd.u64(at(40)?)?,
            align: self.rd.u64(at(48)?)?,
        })
    }

    fn check_phdr(&self, ph: &ProgramHeader) -> Result<(), ElfError> {
        let i = ph.index;
        if ph.offset.checked_add(ph.filesz).is_none() || ph.vaddr.checked_add(ph.memsz).is_none() {
            return Err(ElfError::SegmentOverflow(i));
        }
        if ph.align > 1 && !ph.align.is_power_of_two() {
            return Err(ElfError::BadAlign(i));
        }
        if ph.p_type == pt::LOAD && ph.filesz > ph.memsz {
            return Err(ElfError::FileszExceedsMemsz(i));
        }
        if matches!(ph.p_type, pt::DYNAMIC | pt::INTERP | pt::NOTE)
            && (overlaps(ph.offset, ph.filesz, 0, ehdr_len())
                || overlaps(ph.offset, ph.filesz, self.header.phoff, self.table_len()))
        {
            return Err(ElfError::OverlapsHeaders(Part::Segment(i)));
        }
        Ok(())
    }

    /// The program header with index `i` (`None` past `e_phnum`).
    #[must_use]
    pub fn program_header(&self, i: u16) -> Option<ProgramHeader> {
        if i < self.header.phnum { self.read_phdr(i) } else { None }
    }

    /// The program headers in table order.
    pub fn program_headers(&self) -> impl Iterator<Item = ProgramHeader> + 'a {
        let elf = *self;
        (0..self.header.phnum).filter_map(move |i| elf.read_phdr(i))
    }

    /// The first program header of type `p_type`.
    #[must_use]
    pub fn find(&self, p_type: u32) -> Option<ProgramHeader> {
        self.program_headers().find(|p| p.p_type == p_type)
    }

    /// The file bytes of a segment (`p_offset .. p_offset + p_filesz`).
    ///
    /// # Errors
    ///
    /// [`ElfError::OutOfRange`] when the range is not wholly inside the slice.
    pub fn segment_bytes(&self, ph: &ProgramHeader) -> Result<&'a [u8], ElfError> {
        sub(self.rd.b, ph.offset, ph.filesz).ok_or(ElfError::OutOfRange(Part::Segment(ph.index)))
    }

    /// The file bytes behind `len` bytes at virtual address `vaddr`: the range must lie wholly in the
    /// file-backed part (`p_filesz`) of one `PT_LOAD` segment.
    ///
    /// # Errors
    ///
    /// [`ElfError::AddressNotMapped`] when no segment holds the whole range, [`ElfError::OutOfRange`]
    /// when the segment holding it extends past the slice.
    pub fn bytes_at(&self, vaddr: u64, len: u64) -> Result<&'a [u8], ElfError> {
        let end = vaddr.checked_add(len).ok_or(ElfError::AddressNotMapped(vaddr))?;
        let ph = self
            .program_headers()
            .find(|p| p.p_type == pt::LOAD && p.vaddr <= vaddr && p.vaddr.checked_add(p.filesz).is_some_and(|e| end <= e))
            .ok_or(ElfError::AddressNotMapped(vaddr))?;
        let rel = vaddr.checked_sub(ph.vaddr).ok_or(ElfError::AddressNotMapped(vaddr))?;
        let seg = self.segment_bytes(&ph)?;
        sub(seg, rel, len).ok_or(ElfError::AddressNotMapped(vaddr))
    }

    /// The program interpreter path (without its NUL), if the object has `PT_INTERP`.
    ///
    /// # Errors
    ///
    /// [`ElfError::OutOfRange`] for bytes outside the slice, [`ElfError::BadInterp`] unless the bytes
    /// are a non-empty path followed by exactly one NUL at the end.
    pub fn interp(&self) -> Result<Option<&'a [u8]>, ElfError> {
        let Some(ph) = self.find(pt::INTERP) else { return Ok(None) };
        let b = self.segment_bytes(&ph)?;
        match b.split_last() {
            Some((0, path)) if !path.is_empty() && !path.contains(&0) => Ok(Some(path)),
            _ => Err(ElfError::BadInterp),
        }
    }

    /// The dynamic section, if the object has `PT_DYNAMIC`.
    ///
    /// # Errors
    ///
    /// [`ElfError::OutOfRange`], [`ElfError::DynamicSize`], [`ElfError::DynamicUnterminated`],
    /// [`ElfError::DuplicateDynamic`], [`ElfError::MissingStrtab`], [`ElfError::MissingStrsz`], or
    /// the [`Elf::bytes_at`] errors for the string table.
    pub fn dynamic(&self) -> Result<Option<Dynamic<'a>>, ElfError> {
        let Some(ph) = self.find(pt::DYNAMIC) else { return Ok(None) };
        let all = self.segment_bytes(&ph)?;
        if all.len().checked_rem(DYN_SIZE) != Some(0) {
            return Err(ElfError::DynamicSize);
        }
        let rd = Rd { b: all, be: self.rd.be };
        let (mut strtab, mut strsz, mut wants_strings) = (None, None, false);
        let mut count = 0usize;
        loop {
            let off = count.checked_mul(DYN_SIZE).ok_or(ElfError::DynamicUnterminated)?;
            let (Some(tag), Some(val)) = (rd.u64(off), off.checked_add(8).and_then(|o| rd.u64(o))) else {
                return Err(ElfError::DynamicUnterminated);
            };
            count = count.checked_add(1).ok_or(ElfError::DynamicUnterminated)?;
            match tag {
                dt::NULL => break,
                dt::STRTAB | dt::STRSZ => {
                    let slot = if tag == dt::STRTAB { &mut strtab } else { &mut strsz };
                    if slot.replace(val).is_some() {
                        return Err(ElfError::DuplicateDynamic(tag));
                    }
                }
                dt::NEEDED | dt::SONAME | dt::RPATH | dt::RUNPATH => wants_strings = true,
                _ => {}
            }
        }
        let strtab = match (strtab, strsz) {
            (Some(addr), Some(size)) => Some(self.bytes_at(addr, size)?),
            (Some(_), None) => return Err(ElfError::MissingStrsz),
            (None, Some(_)) => return Err(ElfError::MissingStrtab),
            (None, None) if wants_strings => return Err(ElfError::MissingStrtab),
            (None, None) => None,
        };
        let used = all.get(..count.checked_mul(DYN_SIZE).ok_or(ElfError::DynamicUnterminated)?).ok_or(ElfError::DynamicUnterminated)?;
        Ok(Some(Dynamic { rd: Rd { b: used, be: self.rd.be }, strtab }))
    }

    /// The notes of a `PT_NOTE` segment (or any segment laid out as notes, such as `PT_GNU_PROPERTY`).
    ///
    /// # Errors
    ///
    /// [`ElfError::OutOfRange`] for bytes outside the slice, [`ElfError::BadNoteAlign`] for an
    /// alignment other than 0, 1, 2, 4 or 8. Each note's own errors come from the iterator.
    pub fn notes(&self, ph: &ProgramHeader) -> Result<Notes<'a>, ElfError> {
        let pad = match ph.align {
            0..=4 => 4,
            8 => 8,
            _ => return Err(ElfError::BadNoteAlign(ph.index)),
        };
        Ok(Notes { rd: Rd { b: self.segment_bytes(ph)?, be: self.rd.be }, pad, off: 0, done: false })
    }
}

fn read_header(rd: &Rd<'_>, ident: [u8; 5]) -> Option<Header> {
    let [class, data, ident_version, osabi, abi_version] = ident;
    Some(Header {
        class,
        data,
        ident_version,
        osabi,
        abi_version,
        e_type: rd.u16(16)?,
        machine: rd.u16(18)?,
        version: rd.u32(20)?,
        entry: rd.u64(24)?,
        phoff: rd.u64(32)?,
        shoff: rd.u64(40)?,
        flags: rd.u32(48)?,
        ehsize: rd.u16(52)?,
        phentsize: rd.u16(54)?,
        phnum: rd.u16(56)?,
        shentsize: rd.u16(58)?,
        shnum: rd.u16(60)?,
        shstrndx: rd.u16(62)?,
    })
}

fn ehdr_len() -> u64 {
    u64::try_from(EHDR_SIZE).unwrap_or(u64::MAX)
}

fn phdr_len() -> u64 {
    u64::try_from(PHDR_SIZE).unwrap_or(u64::MAX)
}

/// One dynamic entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynEntry {
    /// `d_tag`.
    pub tag: u64,
    /// `d_val` / `d_ptr`.
    pub val: u64,
}

/// The dynamic section: its entries up to and including the first `DT_NULL`, and the string table
/// (`DT_STRTAB`, `DT_STRSZ`) resolved through the `PT_LOAD` segments.
#[derive(Clone, Copy, Debug)]
pub struct Dynamic<'a> {
    rd: Rd<'a>,
    strtab: Option<&'a [u8]>,
}

impl<'a> Dynamic<'a> {
    /// The number of entries, the terminating `DT_NULL` included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rd.b.len().checked_div(DYN_SIZE).unwrap_or(0)
    }

    /// Never true: a checked dynamic section holds at least its `DT_NULL`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries in order, the terminating `DT_NULL` included.
    pub fn entries(&self) -> impl Iterator<Item = DynEntry> + 'a {
        let rd = self.rd;
        (0..self.len()).filter_map(move |i| {
            let off = i.checked_mul(DYN_SIZE)?;
            Some(DynEntry { tag: rd.u64(off)?, val: rd.u64(off.checked_add(8)?)? })
        })
    }

    /// The value of the first entry with tag `tag`.
    #[must_use]
    pub fn get(&self, tag: u64) -> Option<u64> {
        self.entries().find(|e| e.tag == tag).map(|e| e.val)
    }

    /// The string table (`None` when the object has no `DT_STRTAB`).
    #[must_use]
    pub fn strtab(&self) -> Option<&'a [u8]> {
        self.strtab
    }

    /// The NUL-terminated string at offset `off` of the string table, without its NUL.
    ///
    /// # Errors
    ///
    /// [`ElfError::MissingStrtab`], [`ElfError::StringOutOfRange`] (offset at or past `DT_STRSZ`),
    /// [`ElfError::StringUnterminated`] (no NUL before the end of the table).
    pub fn string(&self, off: u64) -> Result<&'a [u8], ElfError> {
        let tab = self.strtab.ok_or(ElfError::MissingStrtab)?;
        let start = usize::try_from(off).ok().filter(|&o| o < tab.len()).ok_or(ElfError::StringOutOfRange(off))?;
        let rest = tab.get(start..).ok_or(ElfError::StringOutOfRange(off))?;
        let n = rest.iter().position(|&c| c == 0).ok_or(ElfError::StringUnterminated(off))?;
        rest.get(..n).ok_or(ElfError::StringUnterminated(off))
    }

    /// The `DT_NEEDED` names in order.
    pub fn needed(&self) -> impl Iterator<Item = Result<&'a [u8], ElfError>> + 'a {
        let d = *self;
        self.entries().filter(|e| e.tag == dt::NEEDED).map(move |e| d.string(e.val))
    }
}

/// One note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note<'a> {
    /// `n_type`.
    pub n_type: u32,
    /// The owner name without its NUL (`GNU`).
    pub name: &'a [u8],
    /// The descriptor (`n_descsz` bytes, padding excluded).
    pub desc: &'a [u8],
    be: bool,
}

impl<'a> Note<'a> {
    /// The program properties of an `NT_GNU_PROPERTY_TYPE_0` note's descriptor (8-byte aligned
    /// entries of `pr_type`, `pr_datasz`, data). The iterator yields nothing for other notes.
    #[must_use]
    pub fn properties(&self) -> Properties<'a> {
        let ours = self.name == b"GNU" && self.n_type == nt::GNU_PROPERTY_TYPE_0;
        Properties { rd: Rd { b: if ours { self.desc } else { &[] }, be: self.be }, off: 0, done: false }
    }
}

/// Iterator over the notes of a segment; it yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Notes<'a> {
    rd: Rd<'a>,
    pad: usize,
    off: usize,
    done: bool,
}

/// `n` rounded up to a multiple of `pad` (a power of two), checked.
fn round_up(n: usize, pad: usize) -> Option<usize> {
    let mask = pad.checked_sub(1)?;
    Some(n.checked_add(mask)? & !mask)
}

impl<'a> Notes<'a> {
    fn step(&mut self) -> Result<Note<'a>, ElfError> {
        let at = self.off;
        let bad = ElfError::NoteTruncated(at);
        let namesz = usize::try_from(self.rd.u32(at).ok_or(bad)?).map_err(|_| bad)?;
        let descsz = usize::try_from(self.rd.u32(at.checked_add(4).ok_or(bad)?).ok_or(bad)?).map_err(|_| bad)?;
        let n_type = self.rd.u32(at.checked_add(8).ok_or(bad)?).ok_or(bad)?;
        let name_at = at.checked_add(12).ok_or(bad)?;
        let name_full = self.rd.b.get(name_at..name_at.checked_add(namesz).ok_or(bad)?).ok_or(bad)?;
        let desc_at = round_up(name_at.checked_add(namesz).ok_or(bad)?, self.pad).ok_or(bad)?;
        let desc = self.rd.b.get(desc_at..desc_at.checked_add(descsz).ok_or(bad)?).ok_or(bad)?;
        let name = match name_full.split_last() {
            None => name_full,
            Some((0, n)) => n,
            Some(_) => return Err(ElfError::BadNoteName(at)),
        };
        let next = round_up(desc_at.checked_add(descsz).ok_or(bad)?, self.pad).ok_or(bad)?;
        // The padding after the last descriptor may be cut by the segment's end.
        self.off = next.min(self.rd.b.len());
        Ok(Note { n_type, name, desc, be: self.rd.be })
    }
}

impl<'a> Iterator for Notes<'a> {
    type Item = Result<Note<'a>, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.off >= self.rd.b.len() {
            return None;
        }
        let r = self.step();
        self.done = r.is_err();
        Some(r)
    }
}

/// One program property of an `NT_GNU_PROPERTY_TYPE_0` note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Property<'a> {
    /// `pr_type`.
    pub pr_type: u32,
    /// `pr_datasz` bytes of data.
    pub data: &'a [u8],
}

/// Iterator over program properties; it yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Properties<'a> {
    rd: Rd<'a>,
    off: usize,
    done: bool,
}

impl<'a> Properties<'a> {
    fn step(&mut self) -> Result<Property<'a>, ElfError> {
        let at = self.off;
        let bad = ElfError::PropertyTruncated(at);
        let pr_type = self.rd.u32(at).ok_or(bad)?;
        let size = usize::try_from(self.rd.u32(at.checked_add(4).ok_or(bad)?).ok_or(bad)?).map_err(|_| bad)?;
        let data_at = at.checked_add(8).ok_or(bad)?;
        let data = self.rd.b.get(data_at..data_at.checked_add(size).ok_or(bad)?).ok_or(bad)?;
        let next = round_up(data_at.checked_add(size).ok_or(bad)?, 8).ok_or(bad)?;
        self.off = next.min(self.rd.b.len());
        Ok(Property { pr_type, data })
    }
}

impl<'a> Iterator for Properties<'a> {
    type Item = Result<Property<'a>, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.off >= self.rd.b.len() {
            return None;
        }
        let r = self.step();
        self.done = r.is_err();
        Some(r)
    }
}
