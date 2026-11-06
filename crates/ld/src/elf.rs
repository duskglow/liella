//! ELF64 reader (IP-24, plan/ph14/design.md §2). Part 1 (DL-2): the ELF header, the program headers, the
//! dynamic section and notes. Part 2 (DL-3): the GNU and SysV hash tables (structure only; symbol lookup
//! is DL-9), the symbol version tables (`versym`, `verdef`, `verneed`) and the RELA and RELR iterators.
//!
//! The reader works on a byte slice (a file prefix, a whole file or a mapped image) and does no I/O.
//! Every offset, count and size from the object is checked with checked arithmetic against the slice
//! before use (threat-model rule 4): a malformed object gives an [`ElfError`], never a panic and never a
//! read outside the slice. Nothing here allocates. The error rules are decisions D-27.7 (part 1) and D-27.8
//! (part 2) in plan/07-decisions/s26-29.md.
//!
//! The ELF identity (class, byte order, machine) is checked only through [`Arch`], and relocation types
//! are seen only as [`RelocKind`] through [`Arch::classify`] (design §0a).

use core::marker::PhantomData;

use crate::arch::{Arch, RelocKind, ELFCLASS64, ELFDATA2MSB};

/// Size of the ELF64 file header.
pub const EHDR_SIZE: usize = 64;
/// Size of one ELF64 program header.
pub const PHDR_SIZE: usize = 56;
/// Size of one ELF64 dynamic entry.
pub const DYN_SIZE: usize = 16;
/// `e_phnum` value that means "the count is in section header 0" (`PN_XNUM`); refused.
pub const PN_XNUM: u16 = 0xffff;
/// Size of one ELF64 RELA entry (`r_offset`, `r_info`, `r_addend`).
pub const RELA_SIZE: usize = 24;
/// Size of one ELF64 RELR entry.
pub const RELR_SIZE: usize = 8;
/// Size of an `Elf64_Verdef`.
pub const VERDEF_SIZE: usize = 20;
/// Size of an `Elf64_Verdaux`.
pub const VERDAUX_SIZE: usize = 8;
/// Size of an `Elf64_Verneed`.
pub const VERNEED_SIZE: usize = 16;
/// Size of an `Elf64_Vernaux`.
pub const VERNAUX_SIZE: usize = 16;
/// The only `vd_version` / `vn_version` value (`VER_DEF_CURRENT`, `VER_NEED_CURRENT`).
pub const VER_CURRENT: u16 = 1;
/// `versym` bit that marks a hidden (non-default) version.
pub const VERSYM_HIDDEN: u16 = 0x8000;

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
    /// Size in bytes of the PLT relocations (`DT_JMPREL`).
    pub const PLTRELSZ: u64 = 2;
    /// Address of the SysV hash table.
    pub const HASH: u64 = 4;
    /// Address of the dynamic string table.
    pub const STRTAB: u64 = 5;
    /// Address of the dynamic symbol table.
    pub const SYMTAB: u64 = 6;
    /// Address of the RELA relocations.
    pub const RELA: u64 = 7;
    /// Size in bytes of the RELA relocations.
    pub const RELASZ: u64 = 8;
    /// Size of one RELA entry.
    pub const RELAENT: u64 = 9;
    /// Size of the dynamic string table in bytes.
    pub const STRSZ: u64 = 10;
    /// The object's soname (string table offset).
    pub const SONAME: u64 = 14;
    /// Library search path (string table offset).
    pub const RPATH: u64 = 15;
    /// Type of the PLT relocations (`DT_RELA` or `DT_REL`).
    pub const PLTREL: u64 = 20;
    /// Address of the PLT relocations.
    pub const JMPREL: u64 = 23;
    /// Library search path (string table offset).
    pub const RUNPATH: u64 = 29;
    /// Size in bytes of the RELR relocations.
    pub const RELRSZ: u64 = 35;
    /// Address of the RELR relocations.
    pub const RELR: u64 = 36;
    /// Size of one RELR entry.
    pub const RELRENT: u64 = 37;
    /// Address of the GNU hash table.
    pub const GNU_HASH: u64 = 0x6fff_fef5;
    /// Address of the symbol version table (`versym`).
    pub const VERSYM: u64 = 0x6fff_fff0;
    /// Address of the version definitions.
    pub const VERDEF: u64 = 0x6fff_fffc;
    /// Number of version definitions.
    pub const VERDEFNUM: u64 = 0x6fff_fffd;
    /// Address of the version requirements.
    pub const VERNEED: u64 = 0x6fff_fffe;
    /// Number of version requirement entries (files).
    pub const VERNEEDNUM: u64 = 0x6fff_ffff;
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

/// Which table a part-2 error is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// The SysV hash table (`DT_HASH`).
    SysvHash,
    /// The GNU hash table (`DT_GNU_HASH`).
    GnuHash,
    /// The symbol version table (`DT_VERSYM`).
    Versym,
    /// The version definitions (`DT_VERDEF`).
    Verdef,
    /// The version requirements (`DT_VERNEED`).
    Verneed,
    /// The RELA relocations (`DT_RELA`).
    Rela,
    /// The PLT relocations (`DT_JMPREL`).
    Jmprel,
    /// The RELR relocations (`DT_RELR`).
    Relr,
}

/// Why an object was refused. Every variant is a property of the bytes, so the same input always
/// gives the same error (decision D-27.7).
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
    /// A dynamic tag that another present tag requires is absent (the missing tag).
    MissingDynamic(u64),
    /// A dynamic entry has a value the reader does not accept (`DT_RELAENT` not 24, `DT_RELRENT` not 8,
    /// `DT_PLTREL` not `DT_RELA`); the tag.
    BadDynamicValue(u64),
    /// A table's size is not a multiple of its entry size.
    TableSize(Table),
    /// A hash table header the reader refuses (GNU: no Bloom words, or a Bloom shift of 64 or more).
    BadHashHeader(Table),
    /// A hash bucket or chain names a symbol index outside the table (the index).
    HashIndex(Table, u32),
    /// A SysV hash chain visits more entries than the table holds (a loop).
    HashChainLoop(u32),
    /// A version entry or auxiliary entry runs past the end of its segment (offset in the table).
    VersionTruncated(Table, usize),
    /// A version entry's revision is not 1.
    BadVersionRevision(Table, u16),
    /// A version chain ends (next offset 0) before its count (`DT_VERDEFNUM`, `DT_VERNEEDNUM`, `vd_cnt`,
    /// `vn_cnt`) is reached; the offset of the entry whose chain ended early.
    VersionCount(Table, usize),
    /// A `versym` entry names a version index that no definition or requirement has (symbol, index).
    VersionIndex(u32, u16),
    /// A symbol index at or past the symbol count.
    SymbolIndex(u32),
    /// A RELR bitmap entry with no address entry before it (entry index).
    RelrNoBase(usize),
    /// A RELR entry's addresses overflow the address space (entry index).
    RelrOverflow(usize),
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
    /// The first rule the bytes break, in the order of decision D-27.7: identity, header fields, the
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

    /// The file bytes from virtual address `vaddr` to the end of the file-backed part of the `PT_LOAD`
    /// segment holding it (possibly empty), for tables whose length is found by walking them.
    ///
    /// # Errors
    ///
    /// As [`Elf::bytes_at`].
    pub fn bytes_from(&self, vaddr: u64) -> Result<&'a [u8], ElfError> {
        let ph = self
            .program_headers()
            .find(|p| p.p_type == pt::LOAD && p.vaddr <= vaddr && p.vaddr.checked_add(p.filesz).is_some_and(|e| vaddr <= e))
            .ok_or(ElfError::AddressNotMapped(vaddr))?;
        let rel = vaddr.checked_sub(ph.vaddr).ok_or(ElfError::AddressNotMapped(vaddr))?;
        let seg = self.segment_bytes(&ph)?;
        usize::try_from(rel).ok().and_then(|r| seg.get(r..)).ok_or(ElfError::AddressNotMapped(vaddr))
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
        Ok(Some(Dynamic { rd: Rd { b: used, be: self.rd.be }, strtab, elf: *self }))
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
    elf: Elf<'a>,
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

// ---------------------------------------------------------------------------------------------------
// Part 2 (DL-3): hash tables, symbol versions, relocation iterators. Error rules: decision D-27.8.

/// `n * size` as a `u64` byte count, checked.
fn bytes_for(n: u64, size: usize) -> Option<u64> {
    n.checked_mul(u64::try_from(size).ok()?)
}

impl<'a> Dynamic<'a> {
    /// The value of tag `tag`, which may occur at most once.
    fn once(&self, tag: u64) -> Result<Option<u64>, ElfError> {
        let mut it = self.entries().filter(|e| e.tag == tag);
        let first = it.next().map(|e| e.val);
        if it.next().is_some() {
            return Err(ElfError::DuplicateDynamic(tag));
        }
        Ok(first)
    }

    /// An (address, size or count) pair of tags: both or neither.
    fn pair(&self, addr: u64, size: u64) -> Result<Option<(u64, u64)>, ElfError> {
        match (self.once(addr)?, self.once(size)?) {
            (Some(a), Some(s)) => Ok(Some((a, s))),
            (None, None) => Ok(None),
            (Some(_), None) => Err(ElfError::MissingDynamic(size)),
            (None, Some(_)) => Err(ElfError::MissingDynamic(addr)),
        }
    }

    /// The SysV hash table (`DT_HASH`): `nbucket`, `nchain`, then the buckets and chains (32-bit words).
    ///
    /// # Errors
    ///
    /// [`ElfError::DuplicateDynamic`], or the [`Elf::bytes_at`] errors when the header or the
    /// `nbucket + nchain` words are not wholly mapped.
    pub fn sysv_hash(&self) -> Result<Option<SysvHash<'a>>, ElfError> {
        let Some(addr) = self.once(dt::HASH)? else { return Ok(None) };
        let be = self.rd.be;
        let head = Rd { b: self.elf.bytes_at(addr, 8)?, be };
        let (Some(nbucket), Some(nchain)) = (head.u32(0), head.u32(4)) else { return Err(ElfError::AddressNotMapped(addr)) };
        let len = bytes_for(u64::from(nbucket).saturating_add(u64::from(nchain)), 4)
            .and_then(|n| n.checked_add(8))
            .ok_or(ElfError::AddressNotMapped(addr))?;
        let all = self.elf.bytes_at(addr, len)?;
        Ok(Some(SysvHash { rd: Rd { b: all, be }, nbucket, nchain }))
    }

    /// The GNU hash table (`DT_GNU_HASH`): `nbuckets`, `symoffset`, `bloom_size`, `bloom_shift`, the
    /// 64-bit Bloom words, the buckets, then the chain, which runs to the end of the mapped bytes.
    ///
    /// # Errors
    ///
    /// [`ElfError::DuplicateDynamic`], [`ElfError::BadHashHeader`] (no Bloom words, or a shift of 64
    /// or more), or the [`Elf::bytes_at`] errors when the header, Bloom words or buckets are not mapped.
    pub fn gnu_hash(&self) -> Result<Option<GnuHash<'a>>, ElfError> {
        let Some(addr) = self.once(dt::GNU_HASH)? else { return Ok(None) };
        let be = self.rd.be;
        let head = Rd { b: self.elf.bytes_at(addr, 16)?, be };
        let field = |o| head.u32(o).ok_or(ElfError::AddressNotMapped(addr));
        let (nbuckets, symoffset, bloom_size, bloom_shift) = (field(0)?, field(4)?, field(8)?, field(12)?);
        if bloom_size == 0 || bloom_shift >= 64 {
            return Err(ElfError::BadHashHeader(Table::GnuHash));
        }
        let unmapped = ElfError::AddressNotMapped(addr);
        let bloom_len = bytes_for(u64::from(bloom_size), 8).ok_or(unmapped)?;
        let bucket_len = bytes_for(u64::from(nbuckets), 4).ok_or(unmapped)?;
        let fixed = bloom_len.checked_add(bucket_len).and_then(|n| n.checked_add(16)).ok_or(unmapped)?;
        let all = self.elf.bytes_at(addr, fixed)?;
        let bloom = sub(all, 16, bloom_len).ok_or(unmapped)?;
        let buckets = sub(all, bloom_len.checked_add(16).ok_or(unmapped)?, bucket_len).ok_or(unmapped)?;
        let chain = self.elf.bytes_from(addr.checked_add(fixed).ok_or(unmapped)?)?;
        Ok(Some(GnuHash { bloom: Rd { b: bloom, be }, buckets: Rd { b: buckets, be }, chain: Rd { b: chain, be }, nbuckets, symoffset, bloom_shift }))
    }

    /// The number of dynamic symbols: the SysV table's `nchain` when there is one (exact), else the
    /// symbols the GNU table covers (exact when the hashed symbols come last, as linkers place them; an
    /// object whose buckets are all empty gives `symoffset`, a lower bound); `None` without either.
    ///
    /// # Errors
    ///
    /// The errors of [`Dynamic::sysv_hash`], [`Dynamic::gnu_hash`] and [`GnuHash::symbol_count`].
    pub fn symbol_count(&self) -> Result<Option<u32>, ElfError> {
        if let Some(h) = self.sysv_hash()? {
            return Ok(Some(h.nchain()));
        }
        self.gnu_hash()?.map(|g| g.symbol_count()).transpose()
    }

    /// The symbol version table (`DT_VERSYM`): one 16-bit entry per dynamic symbol. The table's length is
    /// not recorded, so an entry is read wherever it lies in the mapped bytes; [`Versym::count`] is the
    /// [`Dynamic::symbol_count`] hint. Every entry is checked against the highest version index the
    /// definitions and requirements give (walked here, names included).
    ///
    /// # Errors
    ///
    /// The [`Elf::bytes_from`] errors for the address, the [`Dynamic::symbol_count`] errors, and the
    /// errors of the version walks.
    pub fn versym(&self) -> Result<Option<Versym<'a>>, ElfError> {
        let Some(addr) = self.once(dt::VERSYM)? else { return Ok(None) };
        let count = self.symbol_count()?;
        let tab = self.elf.bytes_from(addr)?;
        let mut max = 1u16;
        if let Some(defs) = self.verdef()? {
            for d in defs {
                let d = d?;
                for a in d.names() {
                    a?;
                }
                max = max.max(d.ndx & !VERSYM_HIDDEN);
            }
        }
        if let Some(needs) = self.verneed()? {
            for n in needs {
                for a in n?.auxes() {
                    max = max.max(a?.other & !VERSYM_HIDDEN);
                }
            }
        }
        Ok(Some(Versym { rd: Rd { b: tab, be: self.rd.be }, count, max }))
    }

    /// The version definitions (`DT_VERDEF`, `DT_VERDEFNUM` entries).
    ///
    /// # Errors
    ///
    /// [`ElfError::MissingDynamic`] unless both tags are present, the [`Elf::bytes_from`] errors for
    /// the address; each entry's own errors come from the iterator.
    pub fn verdef(&self) -> Result<Option<Verdefs<'a>>, ElfError> {
        let Some((addr, num)) = self.pair(dt::VERDEF, dt::VERDEFNUM)? else { return Ok(None) };
        let b = self.elf.bytes_from(addr)?;
        Ok(Some(Verdefs { dynm: *self, rd: Rd { b, be: self.rd.be }, off: 0, left: num, done: false }))
    }

    /// The version requirements (`DT_VERNEED`, `DT_VERNEEDNUM` entries).
    ///
    /// # Errors
    ///
    /// As [`Dynamic::verdef`].
    pub fn verneed(&self) -> Result<Option<Verneeds<'a>>, ElfError> {
        let Some((addr, num)) = self.pair(dt::VERNEED, dt::VERNEEDNUM)? else { return Ok(None) };
        let b = self.elf.bytes_from(addr)?;
        Ok(Some(Verneeds { dynm: *self, rd: Rd { b, be: self.rd.be }, off: 0, left: num, done: false }))
    }

    /// A RELA table at `addr` of `size` bytes (entry size already checked).
    fn rela_table<A: Arch>(&self, table: Table, addr: u64, size: u64) -> Result<Relas<'a, A>, ElfError> {
        let ent = u64::try_from(RELA_SIZE).map_err(|_| ElfError::TableSize(table))?;
        if size.checked_rem(ent) != Some(0) {
            return Err(ElfError::TableSize(table));
        }
        Ok(Relas { rd: Rd { b: self.elf.bytes_at(addr, size)?, be: self.rd.be }, off: 0, arch: PhantomData })
    }

    /// The RELA relocations (`DT_RELA`, `DT_RELASZ`, `DT_RELAENT` = 24). The whole table is checked here,
    /// so the iterator cannot fail; each entry's type is classified by `A`.
    ///
    /// # Errors
    ///
    /// [`ElfError::MissingDynamic`], [`ElfError::BadDynamicValue`]`(DT_RELAENT)`, [`ElfError::TableSize`],
    /// or the [`Elf::bytes_at`] errors.
    pub fn rela<A: Arch>(&self) -> Result<Option<Relas<'a, A>>, ElfError> {
        let Some((addr, size)) = self.pair(dt::RELA, dt::RELASZ)? else { return Ok(None) };
        match self.once(dt::RELAENT)? {
            None => return Err(ElfError::MissingDynamic(dt::RELAENT)),
            Some(e) if usize::try_from(e).ok() != Some(RELA_SIZE) => return Err(ElfError::BadDynamicValue(dt::RELAENT)),
            Some(_) => {}
        }
        self.rela_table(Table::Rela, addr, size).map(Some)
    }

    /// The PLT relocations (`DT_JMPREL`, `DT_PLTRELSZ`; `DT_PLTREL` must be `DT_RELA`).
    ///
    /// # Errors
    ///
    /// [`ElfError::MissingDynamic`], [`ElfError::BadDynamicValue`]`(DT_PLTREL)`, [`ElfError::TableSize`],
    /// or the [`Elf::bytes_at`] errors.
    pub fn jmprel<A: Arch>(&self) -> Result<Option<Relas<'a, A>>, ElfError> {
        let Some((addr, size)) = self.pair(dt::JMPREL, dt::PLTRELSZ)? else { return Ok(None) };
        match self.once(dt::PLTREL)? {
            None => return Err(ElfError::MissingDynamic(dt::PLTREL)),
            Some(dt::RELA) => {}
            Some(_) => return Err(ElfError::BadDynamicValue(dt::PLTREL)),
        }
        self.rela_table(Table::Jmprel, addr, size).map(Some)
    }

    /// The RELR relocations (`DT_RELR`, `DT_RELRSZ`, `DT_RELRENT` = 8), decoded into the addresses they
    /// name.
    ///
    /// # Errors
    ///
    /// [`ElfError::MissingDynamic`], [`ElfError::BadDynamicValue`]`(DT_RELRENT)`, [`ElfError::TableSize`],
    /// or the [`Elf::bytes_at`] errors; each entry's own errors come from the iterator.
    pub fn relr(&self) -> Result<Option<Relrs<'a>>, ElfError> {
        let Some((addr, size)) = self.pair(dt::RELR, dt::RELRSZ)? else { return Ok(None) };
        match self.once(dt::RELRENT)? {
            None => return Err(ElfError::MissingDynamic(dt::RELRENT)),
            Some(e) if usize::try_from(e).ok() != Some(RELR_SIZE) => return Err(ElfError::BadDynamicValue(dt::RELRENT)),
            Some(_) => {}
        }
        let ent = u64::try_from(RELR_SIZE).map_err(|_| ElfError::TableSize(Table::Relr))?;
        if size.checked_rem(ent) != Some(0) {
            return Err(ElfError::TableSize(Table::Relr));
        }
        let b = self.elf.bytes_at(addr, size)?;
        Ok(Some(Relrs { rd: Rd { b, be: self.rd.be }, idx: 0, base: None, bits: 0, bit_base: 0, bit_idx: 0, done: false }))
    }
}

/// The SysV hash table: `nbucket` buckets and `nchain` chain words, one per dynamic symbol.
#[derive(Clone, Copy, Debug)]
pub struct SysvHash<'a> {
    rd: Rd<'a>,
    nbucket: u32,
    nchain: u32,
}

impl<'a> SysvHash<'a> {
    /// The number of buckets.
    #[must_use]
    pub fn nbucket(&self) -> u32 {
        self.nbucket
    }

    /// The number of chain words, which is the number of dynamic symbols.
    #[must_use]
    pub fn nchain(&self) -> u32 {
        self.nchain
    }

    fn word(&self, i: u64) -> Option<u32> {
        self.rd.u32(usize::try_from(i.checked_mul(4)?.checked_add(8)?).ok()?)
    }

    /// Bucket `i` (`None` past `nbucket`).
    #[must_use]
    pub fn bucket(&self, i: u32) -> Option<u32> {
        if i < self.nbucket { self.word(u64::from(i)) } else { None }
    }

    /// The symbol indices of the chain for hash value `hash` (bucket `hash % nbucket`), in chain order.
    /// The iterator yields one error and then stops; a table without buckets yields nothing.
    #[must_use]
    pub fn chain(&self, hash: u32) -> SysvChain<'a> {
        let start = hash.checked_rem(self.nbucket).and_then(|b| self.bucket(b)).unwrap_or(0);
        SysvChain { h: *self, next: start, steps: 0, done: false }
    }
}

/// The symbol indices of one SysV hash chain.
#[derive(Clone, Debug)]
pub struct SysvChain<'a> {
    h: SysvHash<'a>,
    next: u32,
    steps: u32,
    done: bool,
}

impl Iterator for SysvChain<'_> {
    type Item = Result<u32, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.next == 0 {
            return None;
        }
        let i = self.next;
        let r = if i >= self.h.nchain {
            Err(ElfError::HashIndex(Table::SysvHash, i))
        } else if self.steps >= self.h.nchain {
            Err(ElfError::HashChainLoop(i))
        } else {
            self.steps = self.steps.saturating_add(1);
            match self.h.word(u64::from(self.h.nbucket).saturating_add(u64::from(i))) {
                Some(n) => {
                    self.next = n;
                    Ok(i)
                }
                None => Err(ElfError::HashIndex(Table::SysvHash, i)),
            }
        };
        self.done = r.is_err();
        Some(r)
    }
}

/// The GNU hash table. Symbols below `symoffset` are not hashed; the chain word of symbol `i`
/// (`i >= symoffset`) is its hash with bit 0 replaced by "last in chain".
#[derive(Clone, Copy, Debug)]
pub struct GnuHash<'a> {
    bloom: Rd<'a>,
    buckets: Rd<'a>,
    chain: Rd<'a>,
    nbuckets: u32,
    symoffset: u32,
    bloom_shift: u32,
}

/// One entry of a GNU hash chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GnuEntry {
    /// The symbol index.
    pub sym: u32,
    /// The chain word: the symbol's hash, bit 0 set on the chain's last entry.
    pub hash: u32,
}

impl<'a> GnuHash<'a> {
    /// The number of buckets.
    #[must_use]
    pub fn nbuckets(&self) -> u32 {
        self.nbuckets
    }

    /// The index of the first hashed symbol.
    #[must_use]
    pub fn symoffset(&self) -> u32 {
        self.symoffset
    }

    /// The number of 64-bit Bloom words (at least 1).
    #[must_use]
    pub fn bloom_size(&self) -> usize {
        self.bloom.b.len().checked_div(8).unwrap_or(0)
    }

    /// The second Bloom shift (below 64).
    #[must_use]
    pub fn bloom_shift(&self) -> u32 {
        self.bloom_shift
    }

    /// Bloom word `i` (`None` past the last).
    #[must_use]
    pub fn bloom_word(&self, i: usize) -> Option<u64> {
        self.bloom.u64(i.checked_mul(8)?)
    }

    /// Bucket `i`: the first symbol of its chain, 0 for an empty bucket (`None` past `nbuckets`).
    #[must_use]
    pub fn bucket(&self, i: u32) -> Option<u32> {
        self.buckets.u32(usize::try_from(i).ok()?.checked_mul(4)?)
    }

    /// The chain word of symbol `sym`.
    fn chain_word(&self, sym: u32) -> Result<u32, ElfError> {
        let bad = ElfError::HashIndex(Table::GnuHash, sym);
        let rel = sym.checked_sub(self.symoffset).ok_or(bad)?;
        self.chain.u32(usize::try_from(rel).map_err(|_| bad)?.checked_mul(4).ok_or(bad)?).ok_or(bad)
    }

    /// The chain for hash value `hash` (bucket `hash % nbuckets`), in order. The iterator yields one
    /// error and then stops; an empty bucket or a table without buckets yields nothing.
    #[must_use]
    pub fn chain(&self, hash: u32) -> GnuChain<'a> {
        let start = hash.checked_rem(self.nbuckets).and_then(|b| self.bucket(b)).unwrap_or(0);
        GnuChain { h: *self, next: start, done: start == 0 }
    }

    /// The number of dynamic symbols the table covers: one past the end of the chain that starts at
    /// the highest bucket, or `symoffset` when every bucket is empty.
    ///
    /// # Errors
    ///
    /// [`ElfError::HashIndex`] for a bucket below `symoffset` or a chain that runs past the mapped bytes.
    pub fn symbol_count(&self) -> Result<u32, ElfError> {
        let mut top = 0u32;
        for i in 0..self.nbuckets {
            let b = self.bucket(i).ok_or(ElfError::HashIndex(Table::GnuHash, i))?;
            if b != 0 && b < self.symoffset {
                return Err(ElfError::HashIndex(Table::GnuHash, b));
            }
            top = top.max(b);
        }
        if top == 0 {
            return Ok(self.symoffset);
        }
        let mut last = top;
        for e in (GnuChain { h: *self, next: top, done: false }) {
            last = e?.sym;
        }
        last.checked_add(1).ok_or(ElfError::HashIndex(Table::GnuHash, last))
    }
}

/// The entries of one GNU hash chain.
#[derive(Clone, Debug)]
pub struct GnuChain<'a> {
    h: GnuHash<'a>,
    next: u32,
    done: bool,
}

impl Iterator for GnuChain<'_> {
    type Item = Result<GnuEntry, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let sym = self.next;
        let r = self.h.chain_word(sym).and_then(|hash| {
            if hash & 1 == 1 {
                self.done = true;
            } else {
                self.next = sym.checked_add(1).ok_or(ElfError::HashIndex(Table::GnuHash, sym))?;
            }
            Ok(GnuEntry { sym, hash })
        });
        if r.is_err() {
            self.done = true;
        }
        Some(r)
    }
}

/// A `versym` entry: a version index and the hidden bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionIndex {
    /// The index: 0 local, 1 global (base), otherwise a `vd_ndx` or `vna_other`.
    pub index: u16,
    /// The symbol's version is hidden (not the default version).
    pub hidden: bool,
}

/// The symbol version table.
#[derive(Clone, Copy, Debug)]
pub struct Versym<'a> {
    rd: Rd<'a>,
    count: Option<u32>,
    max: u16,
}

impl Versym<'_> {
    /// The dynamic symbol count from the hash tables (see [`Dynamic::symbol_count`]), if known.
    #[must_use]
    pub fn count(&self) -> Option<u32> {
        self.count
    }

    /// The highest version index a definition or requirement gives (at least 1).
    #[must_use]
    pub fn max_index(&self) -> u16 {
        self.max
    }

    /// The version of symbol `sym`.
    ///
    /// # Errors
    ///
    /// [`ElfError::SymbolIndex`] for an entry past the mapped bytes, [`ElfError::VersionIndex`] for an
    /// index above [`Versym::max_index`].
    pub fn get(&self, sym: u32) -> Result<VersionIndex, ElfError> {
        let off = usize::try_from(sym).ok().and_then(|s| s.checked_mul(2));
        let raw = off.and_then(|o| self.rd.u16(o)).ok_or(ElfError::SymbolIndex(sym))?;
        let index = raw & !VERSYM_HIDDEN;
        if index > self.max {
            return Err(ElfError::VersionIndex(sym, raw));
        }
        Ok(VersionIndex { index, hidden: raw & VERSYM_HIDDEN != 0 })
    }
}

/// `off + rel` as a new offset in a version table, checked (`None` on overflow).
fn advance(off: usize, rel: u32) -> Option<usize> {
    off.checked_add(usize::try_from(rel).ok()?)
}

/// One version definition (`Elf64_Verdef`).
#[derive(Clone, Copy, Debug)]
pub struct Verdef<'a> {
    /// Offset of the entry in the table.
    pub offset: usize,
    /// `vd_flags` (`VER_FLG_BASE` 1, `VER_FLG_WEAK` 2).
    pub flags: u16,
    /// `vd_ndx`: the version index `versym` entries use.
    pub ndx: u16,
    /// `vd_cnt`: the number of names (the version, then its parents).
    pub cnt: u16,
    /// `vd_hash`.
    pub hash: u32,
    aux: u32,
    dynm: Dynamic<'a>,
    rd: Rd<'a>,
}

impl<'a> Verdef<'a> {
    /// The names (`Elf64_Verdaux`): the version's own name, then its parents. Yields one error and stops.
    #[must_use]
    pub fn names(&self) -> Verdauxes<'a> {
        Verdauxes { dynm: self.dynm, rd: self.rd, at: self.offset, off: advance(self.offset, self.aux), left: self.cnt, done: false }
    }
}

/// The version definitions; yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Verdefs<'a> {
    dynm: Dynamic<'a>,
    rd: Rd<'a>,
    off: usize,
    left: u64,
    done: bool,
}

impl<'a> Verdefs<'a> {
    fn step(&mut self) -> Result<Verdef<'a>, ElfError> {
        let at = self.off;
        let bad = ElfError::VersionTruncated(Table::Verdef, at);
        let f16 = |o: usize| self.rd.u16(at.checked_add(o)?);
        let f32 = |o: usize| self.rd.u32(at.checked_add(o)?);
        let version = f16(0).ok_or(bad)?;
        let (flags, ndx, cnt) = (f16(2).ok_or(bad)?, f16(4).ok_or(bad)?, f16(6).ok_or(bad)?);
        let (hash, aux, next) = (f32(8).ok_or(bad)?, f32(12).ok_or(bad)?, f32(16).ok_or(bad)?);
        if version != VER_CURRENT {
            return Err(ElfError::BadVersionRevision(Table::Verdef, version));
        }
        self.left = self.left.saturating_sub(1);
        if self.left > 0 {
            if next == 0 {
                return Err(ElfError::VersionCount(Table::Verdef, at));
            }
            self.off = advance(at, next).ok_or(bad)?;
        }
        Ok(Verdef { offset: at, flags, ndx, cnt, hash, aux, dynm: self.dynm, rd: self.rd })
    }
}

impl<'a> Iterator for Verdefs<'a> {
    type Item = Result<Verdef<'a>, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.left == 0 {
            return None;
        }
        let r = self.step();
        self.done = r.is_err();
        Some(r)
    }
}

/// One name of a version definition (`Elf64_Verdaux`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdaux<'a> {
    /// Offset of the entry in the table.
    pub offset: usize,
    /// The name.
    pub name: &'a [u8],
}

/// The names of one version definition; yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Verdauxes<'a> {
    dynm: Dynamic<'a>,
    rd: Rd<'a>,
    /// Offset of the owning definition (for errors).
    at: usize,
    off: Option<usize>,
    left: u16,
    done: bool,
}

impl<'a> Verdauxes<'a> {
    fn step(&mut self) -> Result<Verdaux<'a>, ElfError> {
        let at = self.off.ok_or(ElfError::VersionTruncated(Table::Verdef, self.at))?;
        let bad = ElfError::VersionTruncated(Table::Verdef, at);
        let name = self.rd.u32(at).ok_or(bad)?;
        let next = self.rd.u32(at.checked_add(4).ok_or(bad)?).ok_or(bad)?;
        self.left = self.left.saturating_sub(1);
        if self.left > 0 {
            if next == 0 {
                return Err(ElfError::VersionCount(Table::Verdef, self.at));
            }
            self.off = Some(advance(at, next).ok_or(bad)?);
        }
        Ok(Verdaux { offset: at, name: self.dynm.string(u64::from(name))? })
    }
}

impl<'a> Iterator for Verdauxes<'a> {
    type Item = Result<Verdaux<'a>, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.left == 0 {
            return None;
        }
        let r = self.step();
        self.done = r.is_err();
        Some(r)
    }
}

/// One version requirement (`Elf64_Verneed`): a needed file and the versions required of it.
#[derive(Clone, Copy, Debug)]
pub struct Verneed<'a> {
    /// Offset of the entry in the table.
    pub offset: usize,
    /// `vn_cnt`: the number of required versions.
    pub cnt: u16,
    /// The file name (`vn_file`).
    pub file: &'a [u8],
    aux: u32,
    dynm: Dynamic<'a>,
    rd: Rd<'a>,
}

impl<'a> Verneed<'a> {
    /// The required versions (`Elf64_Vernaux`). Yields one error and stops.
    #[must_use]
    pub fn auxes(&self) -> Vernauxes<'a> {
        Vernauxes { dynm: self.dynm, rd: self.rd, at: self.offset, off: advance(self.offset, self.aux), left: self.cnt, done: false }
    }
}

/// The version requirements; yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Verneeds<'a> {
    dynm: Dynamic<'a>,
    rd: Rd<'a>,
    off: usize,
    left: u64,
    done: bool,
}

impl<'a> Verneeds<'a> {
    fn step(&mut self) -> Result<Verneed<'a>, ElfError> {
        let at = self.off;
        let bad = ElfError::VersionTruncated(Table::Verneed, at);
        let f32 = |o: usize| self.rd.u32(at.checked_add(o)?);
        let version = self.rd.u16(at).ok_or(bad)?;
        let cnt = self.rd.u16(at.checked_add(2).ok_or(bad)?).ok_or(bad)?;
        let (file, aux, next) = (f32(4).ok_or(bad)?, f32(8).ok_or(bad)?, f32(12).ok_or(bad)?);
        if version != VER_CURRENT {
            return Err(ElfError::BadVersionRevision(Table::Verneed, version));
        }
        self.left = self.left.saturating_sub(1);
        if self.left > 0 {
            if next == 0 {
                return Err(ElfError::VersionCount(Table::Verneed, at));
            }
            self.off = advance(at, next).ok_or(bad)?;
        }
        Ok(Verneed { offset: at, cnt, file: self.dynm.string(u64::from(file))?, aux, dynm: self.dynm, rd: self.rd })
    }
}

impl<'a> Iterator for Verneeds<'a> {
    type Item = Result<Verneed<'a>, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.left == 0 {
            return None;
        }
        let r = self.step();
        self.done = r.is_err();
        Some(r)
    }
}

/// One required version (`Elf64_Vernaux`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vernaux<'a> {
    /// Offset of the entry in the table.
    pub offset: usize,
    /// `vna_hash`.
    pub hash: u32,
    /// `vna_flags` (`VER_FLG_WEAK` 2).
    pub flags: u16,
    /// `vna_other`: the version index `versym` entries use.
    pub other: u16,
    /// The version name.
    pub name: &'a [u8],
}

/// The required versions of one file; yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Vernauxes<'a> {
    dynm: Dynamic<'a>,
    rd: Rd<'a>,
    at: usize,
    off: Option<usize>,
    left: u16,
    done: bool,
}

impl<'a> Vernauxes<'a> {
    fn step(&mut self) -> Result<Vernaux<'a>, ElfError> {
        let at = self.off.ok_or(ElfError::VersionTruncated(Table::Verneed, self.at))?;
        let bad = ElfError::VersionTruncated(Table::Verneed, at);
        let f32 = |o: usize| self.rd.u32(at.checked_add(o)?);
        let hash = self.rd.u32(at).ok_or(bad)?;
        let flags = self.rd.u16(at.checked_add(4).ok_or(bad)?).ok_or(bad)?;
        let other = self.rd.u16(at.checked_add(6).ok_or(bad)?).ok_or(bad)?;
        let (name, next) = (f32(8).ok_or(bad)?, f32(12).ok_or(bad)?);
        self.left = self.left.saturating_sub(1);
        if self.left > 0 {
            if next == 0 {
                return Err(ElfError::VersionCount(Table::Verneed, self.at));
            }
            self.off = Some(advance(at, next).ok_or(bad)?);
        }
        Ok(Vernaux { offset: at, hash, flags, other, name: self.dynm.string(u64::from(name))? })
    }
}

impl<'a> Iterator for Vernauxes<'a> {
    type Item = Result<Vernaux<'a>, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.left == 0 {
            return None;
        }
        let r = self.step();
        self.done = r.is_err();
        Some(r)
    }
}

/// One RELA relocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rela {
    /// `r_offset`: the address of the field to relocate.
    pub offset: u64,
    /// The symbol index (`r_info >> 32`).
    pub sym: u32,
    /// The raw type (`r_info & 0xffff_ffff`), for diagnostics only: code matches on `kind`.
    pub r_type: u32,
    /// The type as the architecture classifies it (an unknown type is `Unsupported`).
    pub kind: RelocKind,
    /// `r_addend`.
    pub addend: i64,
}

/// The entries of a RELA table, in order. The table was range-checked when made, so items cannot fail.
#[derive(Clone, Debug)]
pub struct Relas<'a, A: Arch> {
    rd: Rd<'a>,
    off: usize,
    arch: PhantomData<A>,
}

impl<'a, A: Arch> Relas<'a, A> {
    /// The number of entries in the table.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.rd.b.len().checked_div(RELA_SIZE).unwrap_or(0)
    }
}

impl<A: Arch> Iterator for Relas<'_, A> {
    type Item = Rela;
    fn next(&mut self) -> Option<Rela> {
        let at = self.off;
        let offset = self.rd.u64(at)?;
        let info = self.rd.u64(at.checked_add(8)?)?;
        let addend = i64::from_ne_bytes(self.rd.u64(at.checked_add(16)?)?.to_ne_bytes());
        self.off = at.checked_add(RELA_SIZE)?;
        let r_type = u32::try_from(info & 0xffff_ffff).ok()?;
        let sym = u32::try_from(info.checked_shr(32)?).ok()?;
        Some(Rela { offset, sym, r_type, kind: A::classify(r_type), addend })
    }
}

/// The bytes one RELR bitmap covers: 63 words of 8 bytes; the next bitmap starts after them.
const RELR_BITMAP_SPAN: u64 = 504;

/// The addresses a RELR table names, in order: an even entry is an address, an odd entry a bitmap of
/// the 63 words after the previous address or bitmap. Yields one error and then stops.
#[derive(Clone, Debug)]
pub struct Relrs<'a> {
    rd: Rd<'a>,
    idx: usize,
    /// Where the next bitmap's first bit points (`None`: no address entry yet; `Err`: past the address space).
    base: Option<Result<u64, ElfError>>,
    /// The current bitmap's remaining bits (bit 0 = `bit_base`).
    bits: u64,
    bit_base: u64,
    /// Entry index of the current bitmap (for errors).
    bit_idx: usize,
    done: bool,
}

impl Relrs<'_> {
    fn step(&mut self) -> Option<Result<u64, ElfError>> {
        loop {
            if self.bits != 0 {
                let k = self.bits.trailing_zeros();
                self.bits &= self.bits.wrapping_sub(1);
                let addr = u64::from(k).checked_mul(8).and_then(|d| self.bit_base.checked_add(d));
                return Some(addr.ok_or(ElfError::RelrOverflow(self.bit_idx)));
            }
            let i = self.idx;
            let e = self.rd.u64(i.checked_mul(RELR_SIZE)?)?;
            self.idx = i.checked_add(1)?;
            if e & 1 == 0 {
                self.base = Some(e.checked_add(8).ok_or(ElfError::RelrOverflow(i)));
                return Some(Ok(e));
            }
            let base = match self.base {
                None => return Some(Err(ElfError::RelrNoBase(i))),
                Some(Err(_)) => return Some(Err(ElfError::RelrOverflow(i))),
                Some(Ok(b)) => b,
            };
            self.bits = e.checked_shr(1).unwrap_or(0);
            self.bit_base = base;
            self.bit_idx = i;
            self.base = Some(base.checked_add(RELR_BITMAP_SPAN).ok_or(ElfError::RelrOverflow(i)));
        }
    }
}

impl Iterator for Relrs<'_> {
    type Item = Result<u64, ElfError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let r = self.step()?;
        self.done = r.is_err();
        Some(r)
    }
}
