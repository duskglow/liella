//! The ELF reader (`liella_ld::elf`, IP-24 part 1) over a synthetic image and its malformed variants:
//! every check of decision D-27.7 returns its typed error, never a panic or an out-of-range read.
//! The image is built here byte by byte, so these tests are pure logic and run on any host.
//!
//! Trace: the identity tests cite s27_1_f_0 (wrong ELF class); the rest is spec/security/threat-model.md
//! rule 4 ("parsing never trusts lengths, offsets or counts"), which has no clause ID.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use liella_ld::arch::{Arch, RelocKind, X86_64, ELFDATA2MSB};
use liella_ld::elf::{dt, pt, DynEntry, Elf, ElfError, Part};

// Layout of the valid image (offsets in the file; the one PT_LOAD maps file offset == vaddr).
const PHNUM: usize = 5;
const PH0: usize = 64;
const INTERP_AT: usize = PH0 + PHNUM * 56; // 344
const INTERP: &[u8] = b"/lib/ld-test.so\0"; // 16 bytes
const STRTAB_AT: usize = INTERP_AT + 16; // 360
const STRTAB: &[u8] = b"\0libfoo.so\0libbar.so.1\0"; // 23 bytes: "libfoo.so" at 1, "libbar.so.1" at 11
const LOAD_FILESZ: usize = 384;
const DYN_AT: usize = 384;
const DYN_N: usize = 6;
const NOTE4_AT: usize = DYN_AT + DYN_N * 16; // 480
const NOTE4_LEN: usize = 56;
const NOTE8_AT: usize = NOTE4_AT + NOTE4_LEN; // 536
const NOTE8_LEN: usize = 32;
const TOTAL: usize = NOTE8_AT + NOTE8_LEN; // 568

// Program header indices and field offsets.
const LOAD: usize = 0;
const DYNAMIC: usize = 1;
const INTERP_PH: usize = 2;
const NOTE4: usize = 3;
const NOTE8: usize = 4;
const P_TYPE: usize = 0;
const P_OFFSET: usize = 8;
const P_VADDR: usize = 16;
const P_FILESZ: usize = 32;
const P_MEMSZ: usize = 40;
const P_ALIGN: usize = 48;

/// A test architecture that is not x86_64 (machine 183, big-endian) for the seam tests.
struct BigOther;
impl Arch for BigOther {
    const NAME: &'static str = "test-big-endian";
    const E_MACHINE: u16 = 183;
    const EI_CLASS: u8 = 2;
    const EI_DATA: u8 = ELFDATA2MSB;
    fn classify(r_type: u32) -> RelocKind {
        RelocKind::Unsupported(r_type)
    }
}

struct Img {
    b: Vec<u8>,
    be: bool,
}

impl Img {
    fn put(&mut self, at: usize, v: u64, n: usize) {
        let bytes = if self.be { v.to_be_bytes()[8 - n..].to_vec() } else { v.to_le_bytes()[..n].to_vec() };
        self.b[at..at + n].copy_from_slice(&bytes);
    }
    fn p16(&mut self, at: usize, v: u16) {
        self.put(at, u64::from(v), 2);
    }
    fn p32(&mut self, at: usize, v: u32) {
        self.put(at, u64::from(v), 4);
    }
    fn p64(&mut self, at: usize, v: u64) {
        self.put(at, v, 8);
    }
    fn ph(&mut self, i: usize, field: usize, v: u64) {
        if field == P_TYPE {
            self.p32(PH0 + i * 56, v as u32);
        } else {
            self.p64(PH0 + i * 56 + field, v);
        }
    }
    fn dynent(&mut self, i: usize, tag: u64, val: u64) {
        self.p64(DYN_AT + i * 16, tag);
        self.p64(DYN_AT + i * 16 + 8, val);
    }
}

/// The valid image: ET_DYN for `machine`, PT_LOAD, PT_DYNAMIC, PT_INTERP, a 4-aligned PT_NOTE (build ID +
/// ABI tag) and an 8-aligned PT_NOTE (one GNU property).
fn image_for(machine: u16, be: bool) -> Img {
    let mut m = Img { b: vec![0; TOTAL], be };
    m.b[..4].copy_from_slice(b"\x7fELF");
    m.b[4] = 2;
    m.b[5] = if be { 2 } else { 1 };
    m.b[6] = 1;
    m.p16(16, 3);
    m.p16(18, machine);
    m.p32(20, 1);
    m.p64(24, 0x1000);
    m.p64(32, PH0 as u64);
    m.p16(52, 64);
    m.p16(54, 56);
    m.p16(56, PHNUM as u16);
    let segs: [(u32, u32, usize, usize, u64, u64); PHNUM] = [
        (pt::LOAD, 4, 0, LOAD_FILESZ, 0x2000, 0x1000),
        (pt::DYNAMIC, 6, DYN_AT, DYN_N * 16, DYN_N as u64 * 16, 8),
        (pt::INTERP, 4, INTERP_AT, INTERP.len(), INTERP.len() as u64, 1),
        (pt::NOTE, 4, NOTE4_AT, NOTE4_LEN, NOTE4_LEN as u64, 4),
        (pt::NOTE, 4, NOTE8_AT, NOTE8_LEN, NOTE8_LEN as u64, 8),
    ];
    for (i, (t, f, off, fsz, msz, al)) in segs.into_iter().enumerate() {
        let at = PH0 + i * 56;
        m.p32(at, t);
        m.p32(at + 4, f);
        m.p64(at + 8, off as u64);
        m.p64(at + 16, off as u64);
        m.p64(at + 24, off as u64);
        m.p64(at + 32, fsz as u64);
        m.p64(at + 40, msz);
        m.p64(at + 48, al);
    }
    m.b[INTERP_AT..INTERP_AT + 16].copy_from_slice(INTERP);
    m.b[STRTAB_AT..STRTAB_AT + STRTAB.len()].copy_from_slice(STRTAB);
    let dyns = [(dt::NEEDED, 1), (dt::NEEDED, 11), (dt::SONAME, 11), (dt::STRTAB, STRTAB_AT as u64), (dt::STRSZ, STRTAB.len() as u64), (dt::NULL, 0)];
    for (i, (t, v)) in dyns.into_iter().enumerate() {
        m.dynent(i, t, v);
    }
    // Build ID note (namesz 4, descsz 8) and ABI tag note (descsz 16: Linux 3.2.0).
    let n = NOTE4_AT;
    m.p32(n, 4);
    m.p32(n + 4, 8);
    m.p32(n + 8, 3);
    m.b[n + 12..n + 16].copy_from_slice(b"GNU\0");
    m.b[n + 16..n + 24].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let n = NOTE4_AT + 24;
    m.p32(n, 4);
    m.p32(n + 4, 16);
    m.p32(n + 8, 1);
    m.b[n + 12..n + 16].copy_from_slice(b"GNU\0");
    for (k, v) in [0u32, 3, 2, 0].into_iter().enumerate() {
        m.p32(n + 16 + 4 * k, v);
    }
    // Property note: one property 0xc0000002 with 4 data bytes, padded to 8.
    let n = NOTE8_AT;
    m.p32(n, 4);
    m.p32(n + 4, 16);
    m.p32(n + 8, 5);
    m.b[n + 12..n + 16].copy_from_slice(b"GNU\0");
    m.p32(n + 16, 0xc000_0002);
    m.p32(n + 20, 4);
    m.p32(n + 24, 3);
    m
}

fn image() -> Img {
    image_for(X86_64::E_MACHINE, false)
}

/// What a full walk read.
#[derive(Debug, PartialEq)]
struct Summary {
    interp: Option<Vec<u8>>,
    needed: Vec<Vec<u8>>,
    soname: Option<Vec<u8>>,
    notes: Vec<(Vec<u8>, u32, Vec<u8>)>,
    properties: Vec<(u32, Vec<u8>)>,
}

/// Parse and read everything the reader offers, in a fixed order: header and program headers, interpreter,
/// dynamic section and its strings, then the notes of every PT_NOTE in index order.
fn walk_as<A: Arch>(b: &[u8]) -> Result<Summary, ElfError> {
    let elf = Elf::parse::<A>(b)?;
    let interp = elf.interp()?.map(<[u8]>::to_vec);
    let (mut needed, mut soname) = (Vec::new(), None);
    if let Some(d) = elf.dynamic()? {
        for n in d.needed() {
            needed.push(n?.to_vec());
        }
        if let Some(off) = d.get(dt::SONAME) {
            soname = Some(d.string(off)?.to_vec());
        }
    }
    let (mut notes, mut properties) = (Vec::new(), Vec::new());
    for ph in elf.program_headers().filter(|p| p.p_type == pt::NOTE) {
        for n in elf.notes(&ph)? {
            let n = n?;
            for p in n.properties() {
                let p = p?;
                properties.push((p.pr_type, p.data.to_vec()));
            }
            notes.push((n.name.to_vec(), n.n_type, n.desc.to_vec()));
        }
    }
    Ok(Summary { interp, needed, soname, notes, properties })
}

fn walk(b: &[u8]) -> Result<Summary, ElfError> {
    walk_as::<X86_64>(b)
}

fn err(m: &Img) -> ElfError {
    walk(&m.b).expect_err("malformed image accepted")
}

#[test]
fn valid_image_reads_back() {
    // no clause ID: threat-model rule 4; the unmutated image every malformed case starts from.
    let s = walk(&image().b).unwrap();
    assert_eq!(s.interp.as_deref(), Some(&b"/lib/ld-test.so"[..]));
    assert_eq!(s.needed, vec![b"libfoo.so".to_vec(), b"libbar.so.1".to_vec()]);
    assert_eq!(s.soname.as_deref(), Some(&b"libbar.so.1"[..]));
    assert_eq!(s.notes.len(), 3);
    assert_eq!(s.notes[0], (b"GNU".to_vec(), 3, vec![1, 2, 3, 4, 5, 6, 7, 8]));
    assert_eq!(s.notes[1].1, 1);
    assert_eq!(s.notes[2].1, 5);
    assert_eq!(s.properties, vec![(0xc000_0002, 3u32.to_le_bytes().to_vec())]);
    let m = image();
    let elf = Elf::parse::<X86_64>(&m.b).unwrap();
    let h = elf.header();
    assert_eq!((h.e_type, h.machine, h.entry, h.phoff, h.phnum), (3, X86_64::E_MACHINE, 0x1000, 64, PHNUM as u16));
    let d = elf.dynamic().unwrap().unwrap();
    assert_eq!(d.len(), DYN_N);
    assert_eq!(d.entries().last(), Some(DynEntry { tag: dt::NULL, val: 0 }));
    assert_eq!(elf.program_header(PHNUM as u16), None);
}

#[test]
fn truncated_at_every_length() {
    // no clause ID: threat-model rule 4. Every prefix of the image fails at the part the cut lands in.
    let full = image().b;
    for len in 0..TOTAL {
        let want = match len {
            0..64 => ElfError::TooShort,
            64..INTERP_AT => ElfError::OutOfRange(Part::ProgramHeaders),
            INTERP_AT..STRTAB_AT => ElfError::OutOfRange(Part::Segment(INTERP_PH as u16)),
            STRTAB_AT..NOTE4_AT => ElfError::OutOfRange(Part::Segment(DYNAMIC as u16)),
            NOTE4_AT..NOTE8_AT => ElfError::OutOfRange(Part::Segment(NOTE4 as u16)),
            _ => ElfError::OutOfRange(Part::Segment(NOTE8 as u16)),
        };
        assert_eq!(walk(&full[..len]), Err(want), "prefix of {len} bytes");
    }
    assert!(walk(&full).is_ok());
}

#[test]
fn bad_magic() {
    // no clause ID: threat-model rule 4.
    for i in 0..4 {
        let mut m = image();
        m.b[i] ^= 0x20;
        assert_eq!(err(&m), ElfError::BadMagic);
    }
    // A file shorter than the ELF header is refused as short before its identity is judged.
    let mut m = image();
    m.b[0] = 0;
    assert_eq!(walk(&m.b[..40]), Err(ElfError::TooShort));
}

#[test]
fn s27_1_f_0_wrong_class() {
    // s27_1_f_0: a 32-bit (or unknown-class) object is a wrong-class error.
    for c in [0u8, 1, 3, 0xff] {
        let mut m = image();
        m.b[4] = c;
        assert_eq!(err(&m), ElfError::WrongClass(c));
    }
    // The reader decodes ELF64 only: an architecture declaring a 32-bit class accepts no object.
    struct Class32;
    impl Arch for Class32 {
        const NAME: &'static str = "test-class32";
        const E_MACHINE: u16 = 62;
        const EI_CLASS: u8 = 1;
        const EI_DATA: u8 = 1;
        fn classify(r_type: u32) -> RelocKind {
            RelocKind::Unsupported(r_type)
        }
    }
    let mut m = image();
    m.b[4] = 1;
    assert_eq!(walk_as::<Class32>(&m.b), Err(ElfError::WrongClass(1)));
    assert_eq!(walk_as::<Class32>(&image().b), Err(ElfError::WrongClass(2)));
}

#[test]
fn s27_1_f_0_wrong_data() {
    // s27_1_f_0 (wrong ELF identity): the byte order must be the architecture's.
    let mut m = image();
    m.b[5] = 2;
    assert_eq!(err(&m), ElfError::WrongData(2));
}

#[test]
fn bad_ident_version() {
    // no clause ID: threat-model rule 4 (header fields).
    let mut m = image();
    m.b[6] = 2;
    assert_eq!(err(&m), ElfError::BadIdentVersion(2));
}

#[test]
fn osabi_sysv_and_gnu_only() {
    // no clause ID: plan/ph14/design.md §2 (System V and ELFOSABI_GNU objects).
    let mut m = image();
    m.b[7] = 3;
    assert!(walk(&m.b).is_ok());
    m.b[7] = 9;
    assert_eq!(err(&m), ElfError::UnsupportedOsAbi(9));
}

#[test]
fn exec_and_dyn_only() {
    // no clause ID: plan/ph14/design.md §2 (ET_EXEC and ET_DYN).
    let mut m = image();
    m.p16(16, 2);
    assert!(walk(&m.b).is_ok());
    for t in [0u16, 1, 4, 0xfe00] {
        m.p16(16, t);
        assert_eq!(err(&m), ElfError::UnsupportedType(t));
    }
}

#[test]
fn s27_1_f_0_wrong_machine() {
    // s27_1_f_0 (an object for another machine is refused); x86_64 fact: EM_386 is 3.
    let mut m = image();
    m.p16(18, 3);
    assert_eq!(err(&m), ElfError::WrongMachine(3));
}

#[test]
fn bad_version() {
    // no clause ID: threat-model rule 4 (header fields).
    let mut m = image();
    m.p32(20, 0);
    assert_eq!(err(&m), ElfError::BadVersion(0));
}

#[test]
fn bad_ehsize() {
    // no clause ID: threat-model rule 4 (header fields).
    let mut m = image();
    m.p16(52, 52);
    assert_eq!(err(&m), ElfError::BadEhsize(52));
}

#[test]
fn bad_phentsize() {
    // no clause ID: threat-model rule 4 (header fields).
    let mut m = image();
    m.p16(54, 32);
    assert_eq!(err(&m), ElfError::BadPhentsize(32));
    m.p16(56, 0);
    let s = walk(&m.b).unwrap();
    assert_eq!((s.interp, s.needed.len(), s.notes.len()), (None, 0, 0), "no program headers: nothing to read");
}

#[test]
fn phnum_extended() {
    // no clause ID: threat-model rule 4 (counts).
    let mut m = image();
    m.p16(56, 0xffff);
    assert_eq!(err(&m), ElfError::PhnumExtended);
}

#[test]
fn huge_phnum_and_phoff() {
    // no clause ID: threat-model rule 4 (counts and offsets out of range).
    let mut m = image();
    m.p16(56, 0xfffe);
    assert_eq!(err(&m), ElfError::OutOfRange(Part::ProgramHeaders));
    for off in [u64::MAX, u64::MAX - 55, TOTAL as u64, (TOTAL - 56 * PHNUM + 1) as u64] {
        let mut m = image();
        m.p64(32, off);
        assert_eq!(err(&m), ElfError::OutOfRange(Part::ProgramHeaders), "phoff {off:#x}");
    }
}

#[test]
fn phdrs_overlap_header() {
    // no clause ID: threat-model rule 4 (overlapping header).
    for off in [0u64, 8, 63] {
        let mut m = image();
        m.p64(32, off);
        assert_eq!(err(&m), ElfError::OverlapsHeaders(Part::ProgramHeaders), "phoff {off}");
    }
}

#[test]
fn segment_offset_overflow() {
    // no clause ID: threat-model rule 4 (offset + size overflow).
    let mut m = image();
    m.ph(NOTE4, P_OFFSET, u64::MAX - 8);
    assert_eq!(err(&m), ElfError::SegmentOverflow(NOTE4 as u16));
}

#[test]
fn segment_vaddr_overflow() {
    // no clause ID: threat-model rule 4 (address + size overflow).
    let mut m = image();
    m.ph(LOAD, P_VADDR, u64::MAX - 0xfff);
    assert_eq!(err(&m), ElfError::SegmentOverflow(LOAD as u16));
}

#[test]
fn load_filesz_exceeds_memsz() {
    // no clause ID: threat-model rule 4.
    let mut m = image();
    m.ph(LOAD, P_MEMSZ, LOAD_FILESZ as u64 - 1);
    assert_eq!(err(&m), ElfError::FileszExceedsMemsz(LOAD as u16));
    let mut m = image();
    m.ph(DYNAMIC, P_MEMSZ, 0);
    assert!(walk(&m.b).is_ok(), "the rule is for PT_LOAD only");
}

#[test]
fn bad_align() {
    // no clause ID: threat-model rule 4.
    for a in [3u64, 6, 0x1001, u64::MAX] {
        let mut m = image();
        m.ph(LOAD, P_ALIGN, a);
        assert_eq!(err(&m), ElfError::BadAlign(LOAD as u16), "align {a:#x}");
    }
}

#[test]
fn content_segments_overlap_headers() {
    // no clause ID: threat-model rule 4 (overlapping header).
    for (i, off) in [(DYNAMIC, 0u64), (DYNAMIC, 100), (INTERP_PH, 60), (NOTE4, PH0 as u64 + 56 * 4 + 50)] {
        let mut m = image();
        m.ph(i, P_OFFSET, off);
        assert_eq!(err(&m), ElfError::OverlapsHeaders(Part::Segment(i as u16)), "segment {i} at {off}");
    }
    let mut m = image();
    m.ph(LOAD, P_OFFSET, 0);
    assert!(walk(&m.b).is_ok(), "PT_LOAD may cover the headers");
}

#[test]
fn duplicate_segments() {
    // no clause ID: threat-model rule 4.
    for t in [pt::DYNAMIC, pt::INTERP] {
        let mut m = image();
        m.ph(NOTE8, P_TYPE, u64::from(t));
        assert_eq!(err(&m), ElfError::DuplicateSegment(t));
    }
}

#[test]
fn bad_interp() {
    // no clause ID: threat-model rule 4 (strings are terminated inside their range).
    let mut m = image();
    m.b[INTERP_AT + 15] = b'x';
    assert_eq!(err(&m), ElfError::BadInterp);
    let mut m = image();
    m.b[INTERP_AT + 4] = 0;
    assert_eq!(err(&m), ElfError::BadInterp);
    for len in [0u64, 1] {
        let mut m = image();
        m.b[INTERP_AT] = 0;
        m.ph(INTERP_PH, P_FILESZ, len);
        assert_eq!(err(&m), ElfError::BadInterp, "filesz {len}");
    }
}

#[test]
fn dynamic_size_not_multiple() {
    // no clause ID: threat-model rule 4.
    let mut m = image();
    m.ph(DYNAMIC, P_FILESZ, DYN_N as u64 * 16 - 8);
    assert_eq!(err(&m), ElfError::DynamicSize);
}

#[test]
fn dynamic_unterminated() {
    // no clause ID: threat-model rule 4 (no DT_NULL inside the segment).
    let mut m = image();
    m.dynent(DYN_N - 1, 0x6fff_fff0, 0);
    assert_eq!(err(&m), ElfError::DynamicUnterminated);
    let mut m = image();
    m.ph(DYNAMIC, P_FILESZ, 0);
    assert_eq!(err(&m), ElfError::DynamicUnterminated);
}

#[test]
fn dynamic_stops_at_first_null() {
    // no clause ID: threat-model rule 4 (entries after DT_NULL are not read).
    let mut m = image();
    m.dynent(0, 0x6fff_fff0, 0);
    m.dynent(1, dt::NULL, 0);
    let elf = Elf::parse::<X86_64>(&m.b).unwrap();
    let d = elf.dynamic().unwrap().unwrap();
    assert_eq!(d.len(), 2);
    assert_eq!(d.strtab(), None, "DT_STRTAB after DT_NULL is not read");
    let mut m = image();
    m.dynent(2, dt::NULL, 0);
    assert_eq!(err(&m), ElfError::MissingStrtab, "DT_NEEDED before DT_NULL, DT_STRTAB after it");
}

#[test]
fn duplicate_strtab_or_strsz() {
    // no clause ID: threat-model rule 4 (one string table).
    for tag in [dt::STRTAB, dt::STRSZ] {
        let mut m = image();
        m.dynent(2, tag, 1);
        assert_eq!(err(&m), ElfError::DuplicateDynamic(tag));
    }
}

#[test]
fn missing_strtab_or_strsz() {
    // no clause ID: threat-model rule 4.
    let mut m = image();
    m.dynent(4, dt::NEEDED, 1);
    assert_eq!(err(&m), ElfError::MissingStrsz);
    let mut m = image();
    m.dynent(3, dt::NEEDED, 1);
    assert_eq!(err(&m), ElfError::MissingStrtab);
    let mut m = image();
    m.dynent(3, dt::NEEDED, 1);
    m.dynent(4, dt::NEEDED, 1);
    assert_eq!(Elf::parse::<X86_64>(&m.b).unwrap().dynamic().map(|_| ()), Err(ElfError::MissingStrtab));
    // DT_STRSZ alone, with no string-valued entry, is refused as well.
    let mut m = image();
    for i in 0..4 {
        m.dynent(i, 0x6fff_fff0, 0);
    }
    assert_eq!(Elf::parse::<X86_64>(&m.b).unwrap().dynamic().map(|_| ()), Err(ElfError::MissingStrtab));
}

#[test]
fn strtab_not_mapped() {
    // no clause ID: threat-model rule 4 (the string table lies in one PT_LOAD's file-backed part).
    for (tag, v) in [(dt::STRTAB, 0x1_0000u64), (dt::STRTAB, u64::MAX - 4), (dt::STRSZ, LOAD_FILESZ as u64), (dt::STRSZ, u64::MAX)] {
        let mut m = image();
        m.dynent(if tag == dt::STRTAB { 3 } else { 4 }, tag, v);
        let at = if tag == dt::STRTAB { v } else { STRTAB_AT as u64 };
        assert_eq!(err(&m), ElfError::AddressNotMapped(at), "{tag} = {v:#x}");
    }
    // In memsz but past filesz (bss) is not file-backed.
    let mut m = image();
    m.dynent(3, dt::STRTAB, LOAD_FILESZ as u64);
    assert_eq!(err(&m), ElfError::AddressNotMapped(LOAD_FILESZ as u64));
}

#[test]
fn string_offsets() {
    // no clause ID: threat-model rule 4 (string offsets and terminators inside DT_STRSZ).
    let mut m = image();
    m.dynent(0, dt::NEEDED, STRTAB.len() as u64);
    assert_eq!(err(&m), ElfError::StringOutOfRange(STRTAB.len() as u64));
    let mut m = image();
    m.dynent(0, dt::NEEDED, u64::MAX);
    assert_eq!(err(&m), ElfError::StringOutOfRange(u64::MAX));
    let mut m = image();
    m.dynent(4, dt::STRSZ, STRTAB.len() as u64 - 1);
    assert_eq!(err(&m), ElfError::StringUnterminated(11));
    let mut m = image();
    m.dynent(0, dt::NEEDED, STRTAB.len() as u64 - 1);
    assert_eq!(walk(&m.b).unwrap().needed[0], b"", "the last byte is an empty string");
}

#[test]
fn note_truncated() {
    // no clause ID: threat-model rule 4 (note sizes inside the segment).
    let mut m = image();
    m.ph(NOTE4, P_FILESZ, 5);
    assert_eq!(err(&m), ElfError::NoteTruncated(0));
    let mut m = image();
    m.p32(NOTE4_AT, 0xffff_ffff);
    assert_eq!(err(&m), ElfError::NoteTruncated(0));
    let mut m = image();
    m.p32(NOTE4_AT + 24 + 4, 0xffff_ffff);
    assert_eq!(err(&m), ElfError::NoteTruncated(24));
    let mut m = image();
    m.p32(NOTE4_AT + 24 + 4, 17);
    assert_eq!(err(&m), ElfError::NoteTruncated(24));
    let mut m = image();
    m.ph(NOTE4, P_FILESZ, NOTE4_LEN as u64 + 4);
    m.ph(NOTE4, P_MEMSZ, NOTE4_LEN as u64 + 4);
    assert_eq!(err(&m), ElfError::NoteTruncated(NOTE4_LEN), "a trailing partial header");
}

#[test]
fn note_padding_follows_align() {
    // no clause ID: threat-model rule 4. A 4-byte descriptor is padded to the segment's alignment: with
    // p_align 8 the next note starts 8-aligned, read 4-aligned the padding is taken as the next header.
    let mut m = image();
    m.p32(NOTE4_AT + 4, 4);
    m.ph(NOTE4, P_ALIGN, 8);
    let s = walk(&m.b).unwrap();
    assert_eq!(s.notes[0].2, vec![1, 2, 3, 4]);
    assert_eq!(s.notes.len(), 3);
    m.ph(NOTE4, P_ALIGN, 4);
    assert_eq!(err(&m), ElfError::NoteTruncated(20));
}

#[test]
fn note_name_unterminated() {
    // no clause ID: threat-model rule 4 (terminated names).
    let mut m = image();
    m.b[NOTE4_AT + 15] = b'!';
    assert_eq!(err(&m), ElfError::BadNoteName(0));
}

#[test]
fn bad_note_align() {
    // no clause ID: threat-model rule 4.
    for a in [16u64, 32, 0x1000] {
        let mut m = image();
        m.ph(NOTE4, P_ALIGN, a);
        assert_eq!(err(&m), ElfError::BadNoteAlign(NOTE4 as u16), "align {a}");
    }
}

#[test]
fn property_truncated() {
    // no clause ID: threat-model rule 4 (property sizes inside the note descriptor).
    let mut m = image();
    m.p32(NOTE8_AT + 20, 0x100);
    assert_eq!(err(&m), ElfError::PropertyTruncated(0));
    let mut m = image();
    m.p32(NOTE8_AT + 20, 0xffff_ffff);
    assert_eq!(err(&m), ElfError::PropertyTruncated(0));
    let mut m = image();
    m.p32(NOTE8_AT + 4, 4);
    m.p32(NOTE8_AT + 20, 0);
    assert_eq!(err(&m), ElfError::PropertyTruncated(0), "a descriptor of 4 bytes holds a partial property header");
}

#[test]
fn arch_seam_machine_and_byte_order() {
    // no spec clause: the Arch seam (plan/ph14/design.md §0a). A second, big-endian architecture reads its own
    // image exactly as x86_64 reads the little-endian one, and each refuses the other's.
    let mut le = walk_as::<X86_64>(&image().b).unwrap();
    let be_img = image_for(BigOther::E_MACHINE, true);
    let mut be = walk_as::<BigOther>(&be_img.b).unwrap();
    // Descriptors and property data are raw bytes: the big-endian image stored their words big-endian.
    be.properties[0].1.reverse();
    for s in [&mut le, &mut be] {
        s.notes.iter_mut().skip(1).for_each(|n| n.2.clear());
    }
    assert_eq!(le, be);
    assert_eq!(walk_as::<BigOther>(&image().b), Err(ElfError::WrongData(1)));
    assert_eq!(walk_as::<X86_64>(&be_img.b), Err(ElfError::WrongData(2)));
    let mut other_le = image_for(BigOther::E_MACHINE, false);
    assert_eq!(walk_as::<X86_64>(&other_le.b), Err(ElfError::WrongMachine(BigOther::E_MACHINE)));
    other_le.p16(18, X86_64::E_MACHINE);
    assert!(walk_as::<X86_64>(&other_le.b).is_ok());
}
