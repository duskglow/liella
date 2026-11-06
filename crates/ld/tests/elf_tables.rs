//! The ELF reader, part 2 (`liella_ld::elf`, IP-24 part 2, DL-ELF-READ-2) over a synthetic image and its
//! malformed variants: the hash tables, the symbol version tables and the RELA/RELR iterators. Every rule
//! of decision D-27.8 returns its typed error, never a panic or an out-of-range read. The image is built
//! here byte by byte, so these tests are pure logic and run on any host.
//!
//! Trace: no spec clause; spec/security/threat-model.md rule 4 ("parsing never trusts lengths, offsets or
//! counts"), which has no clause ID. The relocation-kind test takes an `Arch` parameter (design §0a item 7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use liella_ld::arch::{Arch, RelocKind, X86_64};
use liella_ld::elf::{dt, pt, Dynamic, Elf, ElfError, GnuEntry, Table, VersionIndex};

// Layout (file offset == vaddr; one PT_LOAD covers the whole file).
const DYN_AT: usize = 176;
const DYNS: [(u64, u64); 19] = [
    (dt::STRTAB, STR_AT as u64),
    (dt::STRSZ, STR.len() as u64),
    (dt::HASH, SYSV_AT as u64),
    (dt::GNU_HASH, GNU_AT as u64),
    (dt::VERSYM, VERSYM_AT as u64),
    (dt::VERDEF, VERDEF_AT as u64),
    (dt::VERDEFNUM, 3),
    (dt::VERNEED, VERNEED_AT as u64),
    (dt::VERNEEDNUM, 1),
    (dt::RELA, RELA_AT as u64),
    (dt::RELASZ, 72),
    (dt::RELAENT, 24),
    (dt::JMPREL, JMPREL_AT as u64),
    (dt::PLTRELSZ, 24),
    (dt::PLTREL, dt::RELA),
    (dt::RELR, RELR_AT as u64),
    (dt::RELRSZ, 24),
    (dt::RELRENT, 8),
    (dt::NULL, 0),
];
const STR_AT: usize = 480;
const STR: &[u8] = b"\0lib.so\0V1\0V2\0libc.so.6\0G1\0"; // lib.so 1, V1 8, V2 11, libc.so.6 14, G1 24
const SYSV_AT: usize = 512; // nbucket 2, nchain 4: bucket 0 -> 3 -> 1, bucket 1 -> 2
const GNU_AT: usize = 544; // nbuckets 1, symoffset 2, one Bloom word, shift 6; chain words at 572
const GNU_CHAIN_AT: usize = 572;
const VERSYM_AT: usize = 584; // [0, 4, 2, 0x8003]
const VERDEF_AT: usize = 592; // lib.so (1, base), V1 (2), V2 (3, parent V1); entries at +0, +28, +56
const VERNEED_AT: usize = 688; // libc.so.6: G1 (4)
const RELA_AT: usize = 720;
const JMPREL_AT: usize = 792;
const RELR_AT: usize = 816; // 0x2000, bitmap 0b1011, bitmap 0b11
const TOTAL: usize = 840;

/// A test architecture whose table differs from x86_64's: only type 1 is known, as `Relative`.
struct OtherTable;
impl Arch for OtherTable {
    const NAME: &'static str = "test-other-table";
    const E_MACHINE: u16 = 62;
    const EI_CLASS: u8 = 2;
    const EI_DATA: u8 = 1;
    fn classify(r_type: u32) -> RelocKind {
        if r_type == 1 { RelocKind::Relative } else { RelocKind::Unsupported(r_type) }
    }
}

struct Img(Vec<u8>);

impl Img {
    fn p16(&mut self, at: usize, v: u16) {
        self.0[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn p32(&mut self, at: usize, v: u32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn p64(&mut self, at: usize, v: u64) {
        self.0[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn words32(&mut self, at: usize, w: &[u32]) {
        for (i, v) in w.iter().enumerate() {
            self.p32(at + 4 * i, *v);
        }
    }
    /// Sets the value of dynamic entry `tag`.
    fn set(&mut self, tag: u64, val: u64) {
        let i = DYNS.iter().position(|d| d.0 == tag).unwrap();
        self.p64(DYN_AT + 16 * i + 8, val);
    }
    /// Turns dynamic entry `tag` into one the reader ignores (`DT_LOOS`).
    fn drop_tag(&mut self, tag: u64) {
        let i = DYNS.iter().position(|d| d.0 == tag).unwrap();
        self.p64(DYN_AT + 16 * i, 0x6000_000d);
    }
    /// Turns dynamic entry `victim` into a second `tag` entry with value `val`.
    fn retag(&mut self, victim: u64, tag: u64, val: u64) {
        let i = DYNS.iter().position(|d| d.0 == victim).unwrap();
        self.p64(DYN_AT + 16 * i, tag);
        self.p64(DYN_AT + 16 * i + 8, val);
    }
}

fn image() -> Img {
    let mut m = Img(vec![0; TOTAL]);
    m.0[..4].copy_from_slice(b"\x7fELF");
    m.0[4..7].copy_from_slice(&[2, 1, 1]);
    m.p16(16, 3);
    m.p16(18, X86_64::E_MACHINE);
    m.p32(20, 1);
    m.p64(32, 64);
    m.p16(52, 64);
    m.p16(54, 56);
    m.p16(56, 2);
    for (i, (t, off, len, align)) in [(pt::LOAD, 0, TOTAL, 0x1000u64), (pt::DYNAMIC, DYN_AT, DYNS.len() * 16, 8)].into_iter().enumerate() {
        let at = 64 + 56 * i;
        m.p32(at, t);
        m.p32(at + 4, 6);
        for (f, v) in [(8, off), (16, off), (24, off), (32, len), (40, len)] {
            m.p64(at + f, v as u64);
        }
        m.p64(at + 48, align);
    }
    for (i, (t, v)) in DYNS.into_iter().enumerate() {
        m.p64(DYN_AT + 16 * i, t);
        m.p64(DYN_AT + 16 * i + 8, v);
    }
    m.0[STR_AT..STR_AT + STR.len()].copy_from_slice(STR);
    m.words32(SYSV_AT, &[2, 4, 3, 2, 0, 0, 0, 1]);
    m.words32(GNU_AT, &[1, 2, 1, 6]);
    m.p64(GNU_AT + 16, u64::MAX);
    m.words32(GNU_AT + 24, &[2, 0x10, 0x21]);
    for (i, v) in [0u16, 4, 2, 0x8003].into_iter().enumerate() {
        m.p16(VERSYM_AT + 2 * i, v);
    }
    // Verdef: version, flags, ndx, cnt (u16), hash, aux, next (u32); Verdaux: name, next.
    for (k, (flags, ndx, cnt, next)) in [(1u16, 1u16, 1u16, 28u32), (0, 2, 1, 28), (0, 3, 2, 0)].into_iter().enumerate() {
        let at = VERDEF_AT + 28 * k;
        for (o, v) in [(0, 1), (2, flags), (4, ndx), (6, cnt)] {
            m.p16(at + o, v);
        }
        m.words32(at + 8, &[0x1234, 20, next]);
    }
    m.words32(VERDEF_AT + 20, &[1, 0]);
    m.words32(VERDEF_AT + 48, &[8, 0]);
    m.words32(VERDEF_AT + 76, &[11, 8, 8, 0]);
    // Verneed: version, cnt (u16), file, aux, next; Vernaux: hash, flags, other (u16), name, next.
    m.p16(VERNEED_AT, 1);
    m.p16(VERNEED_AT + 2, 1);
    m.words32(VERNEED_AT + 4, &[14, 16, 0, 0x5678]);
    m.p16(VERNEED_AT + 20, 2);
    m.p16(VERNEED_AT + 22, 4);
    m.words32(VERNEED_AT + 24, &[24, 0]);
    for (i, (off, info, add)) in [(0x1000u64, 8u64, 0x10i64), (0x1008, (2 << 32) | 1, 0), (0x1010, 0xdead, -5), (0x1018, (1 << 32) | 7, 0)].into_iter().enumerate() {
        m.p64(RELA_AT + 24 * i, off);
        m.p64(RELA_AT + 24 * i + 8, info);
        m.p64(RELA_AT + 24 * i + 16, add as u64);
    }
    for (i, v) in [0x2000u64, 0b1011, 0b11].into_iter().enumerate() {
        m.p64(RELR_AT + 8 * i, v);
    }
    m
}

/// One required version: flags, index, name.
type Need = (u16, u16, Vec<u8>);

/// Everything a full walk read.
#[derive(Debug, PartialEq)]
struct Summary {
    sysv: (u32, Vec<u32>, Vec<u32>),
    gnu: (u32, Vec<GnuEntry>),
    versym: Vec<VersionIndex>,
    verdef: Vec<(usize, u16, u16, Vec<Vec<u8>>)>,
    verneed: Vec<(Vec<u8>, Vec<Need>)>,
    rela: Vec<(u64, u32, RelocKind, i64)>,
    jmprel: Vec<(u64, u32, RelocKind, i64)>,
    relr: Vec<u64>,
}

fn relas(it: Option<impl Iterator<Item = liella_ld::elf::Rela>>) -> Vec<(u64, u32, RelocKind, i64)> {
    it.into_iter().flatten().map(|r| (r.offset, r.sym, r.kind, r.addend)).collect()
}

fn walk_dyn<A: Arch>(d: &Dynamic<'_>) -> Result<Summary, ElfError> {
    let need = ElfError::MissingDynamic;
    let h = d.sysv_hash()?.ok_or(need(dt::HASH))?;
    let sysv = (h.nchain(), h.chain(0).collect::<Result<_, _>>()?, h.chain(1).collect::<Result<_, _>>()?);
    let g = d.gnu_hash()?.ok_or(need(dt::GNU_HASH))?;
    let gnu = (g.symbol_count()?, g.chain(0).collect::<Result<_, _>>()?);
    let vs = d.versym()?.ok_or(need(dt::VERSYM))?;
    let versym = (0..vs.count().unwrap_or(0)).map(|i| vs.get(i)).collect::<Result<_, _>>()?;
    let mut verdef = Vec::new();
    for v in d.verdef()?.ok_or(need(dt::VERDEF))? {
        let v = v?;
        let names = v.names().map(|a| a.map(|a| a.name.to_vec())).collect::<Result<_, _>>()?;
        verdef.push((v.offset, v.flags, v.ndx, names));
    }
    let mut verneed = Vec::new();
    for n in d.verneed()?.ok_or(need(dt::VERNEED))? {
        let n = n?;
        let aux = n.auxes().map(|a| a.map(|a| (a.flags, a.other, a.name.to_vec()))).collect::<Result<_, _>>()?;
        verneed.push((n.file.to_vec(), aux));
    }
    let rela = relas(d.rela::<A>()?);
    let jmprel = relas(d.jmprel::<A>()?);
    let relr = d.relr()?.into_iter().flatten().collect::<Result<_, _>>()?;
    Ok(Summary { sysv, gnu, versym, verdef, verneed, rela, jmprel, relr })
}

fn walk(b: &[u8]) -> Result<Summary, ElfError> {
    let elf = Elf::parse::<X86_64>(b)?;
    walk_dyn::<X86_64>(&elf.dynamic()?.ok_or(ElfError::MissingDynamic(dt::NULL))?)
}

fn err(m: &Img) -> ElfError {
    walk(&m.0).expect_err("malformed image accepted")
}

fn with(f: impl FnOnce(&mut Img)) -> ElfError {
    let mut m = image();
    f(&mut m);
    err(&m)
}

#[test]
fn tables_read_back() {
    // no spec clause: the reader decodes every part-2 table of a well-formed image (D-27.8).
    let s = walk(&image().0).unwrap();
    assert_eq!(s.sysv, (4, vec![3, 1], vec![2]));
    assert_eq!(s.gnu, (4, vec![GnuEntry { sym: 2, hash: 0x10 }, GnuEntry { sym: 3, hash: 0x21 }]));
    let vi = |index, hidden| VersionIndex { index, hidden };
    assert_eq!(s.versym, vec![vi(0, false), vi(4, false), vi(2, false), vi(3, true)]);
    assert_eq!(
        s.verdef,
        vec![(0, 1, 1, vec![b"lib.so".to_vec()]), (28, 0, 2, vec![b"V1".to_vec()]), (56, 0, 3, vec![b"V2".to_vec(), b"V1".to_vec()])]
    );
    assert_eq!(s.verneed, vec![(b"libc.so.6".to_vec(), vec![(2, 4, b"G1".to_vec())])]);
    assert_eq!(s.rela, vec![(0x1000, 0, RelocKind::Relative, 0x10), (0x1008, 2, RelocKind::Abs64, 0), (0x1010, 0, RelocKind::Unsupported(0xdead), -5)]);
    assert_eq!(s.jmprel, vec![(0x1018, 1, RelocKind::JumpSlot, 0)]);
    assert_eq!(s.relr, vec![0x2000, 0x2008, 0x2018, 0x2200]);
}

#[test]
fn rela_kinds_come_from_the_arch() {
    // no spec clause: design §0a seam; the iterator classifies through the Arch it is given, and a type
    // that Arch does not know is Unsupported(n), never skipped.
    let m = image();
    let elf = Elf::parse::<X86_64>(&m.0).unwrap();
    let d = elf.dynamic().unwrap().unwrap();
    let kinds: Vec<RelocKind> = d.rela::<OtherTable>().unwrap().unwrap().map(|r| r.kind).collect();
    assert_eq!(kinds, vec![RelocKind::Unsupported(8), RelocKind::Relative, RelocKind::Unsupported(0xdead)]);
    assert_eq!(d.rela::<X86_64>().unwrap().unwrap().entry_count(), 3);
}

#[test]
fn truncated_at_every_length() {
    // no spec clause: threat-model rule 4; every prefix of the file is refused without a panic.
    let m = image();
    for len in 0..TOTAL {
        assert!(walk(&m.0[..len]).is_err(), "prefix of {len} bytes accepted");
    }
}

#[test]
fn any_byte_changed_never_panics() {
    // no spec clause: threat-model rule 4; each table byte set to 0xff or 0x80 gives a result, never a panic.
    let m = image();
    for at in DYN_AT..TOTAL {
        for v in [0xff, 0x80, 0x01] {
            let mut b = m.0.clone();
            b[at] = v;
            let _ = walk(&b);
        }
    }
}

#[test]
fn sysv_hash_truncated() {
    // no spec clause: D-27.8, the buckets and chains must be mapped.
    assert_eq!(with(|m| m.p32(SYSV_AT + 4, 0x10_0000)), ElfError::AddressNotMapped(SYSV_AT as u64));
    assert_eq!(with(|m| m.set(dt::HASH, TOTAL as u64 - 4)), ElfError::AddressNotMapped(TOTAL as u64 - 4));
}

#[test]
fn sysv_chain_out_of_range() {
    // no spec clause: D-27.8, a bucket or chain entry at or past nchain.
    assert_eq!(with(|m| m.p32(SYSV_AT + 28, 9)), ElfError::HashIndex(Table::SysvHash, 9));
    assert_eq!(with(|m| m.p32(SYSV_AT + 8, 4)), ElfError::HashIndex(Table::SysvHash, 4));
}

#[test]
fn sysv_chain_loop() {
    // no spec clause: D-27.8, a chain that visits more than nchain entries.
    assert_eq!(with(|m| m.p32(SYSV_AT + 28, 3)), ElfError::HashChainLoop(3));
}

#[test]
fn gnu_hash_bad_header() {
    // no spec clause: D-27.8, at least one Bloom word and a shift below 64.
    assert_eq!(with(|m| m.p32(GNU_AT + 8, 0)), ElfError::BadHashHeader(Table::GnuHash));
    assert_eq!(with(|m| m.p32(GNU_AT + 12, 64)), ElfError::BadHashHeader(Table::GnuHash));
    assert_eq!(with(|m| m.p32(GNU_AT, 0x4000_0000)), ElfError::AddressNotMapped(GNU_AT as u64));
}

#[test]
fn gnu_chain_out_of_range() {
    // no spec clause: D-27.8, a bucket below symoffset, a chain past the mapped bytes.
    assert_eq!(with(|m| m.p32(GNU_AT + 24, 1)), ElfError::HashIndex(Table::GnuHash, 1));
    assert_eq!(with(|m| m.p32(GNU_AT + 24, 0x7fff_0000)), ElfError::HashIndex(Table::GnuHash, 0x7fff_0000));
    // No end bit: the chain runs on through the following tables to the segment end.
    let words = (TOTAL - GNU_CHAIN_AT) / 4;
    let mut m = image();
    for i in 0..words {
        m.p32(GNU_CHAIN_AT + 4 * i, 0x10);
    }
    let elf = Elf::parse::<X86_64>(&m.0).unwrap();
    let g = elf.dynamic().unwrap().unwrap().gnu_hash().unwrap().unwrap();
    assert_eq!(g.symbol_count(), Err(ElfError::HashIndex(Table::GnuHash, 2 + words as u32)));
}

#[test]
fn versym_index_out_of_range() {
    // no spec clause: D-27.8, a versym index no definition or requirement gives.
    assert_eq!(with(|m| m.p16(VERSYM_AT + 2, 5)), ElfError::VersionIndex(1, 5));
    assert_eq!(with(|m| m.p16(VERSYM_AT + 2, 0x8005)), ElfError::VersionIndex(1, 0x8005));
    let m = image();
    let elf = Elf::parse::<X86_64>(&m.0).unwrap();
    let vs = elf.dynamic().unwrap().unwrap().versym().unwrap().unwrap();
    assert_eq!((vs.max_index(), vs.get(4).is_ok(), vs.get(128)), (4, true, Err(ElfError::SymbolIndex(128))));
}

#[test]
fn versym_bounded_by_the_mapping() {
    // no spec clause: D-27.8, versym entries are read within the mapped bytes; the count is a hint.
    let mut m = image();
    m.drop_tag(dt::HASH);
    m.drop_tag(dt::GNU_HASH);
    let elf = Elf::parse::<X86_64>(&m.0).unwrap();
    let vs = elf.dynamic().unwrap().unwrap().versym().unwrap().unwrap();
    assert_eq!((vs.count(), vs.get(3)), (None, Ok(VersionIndex { index: 3, hidden: true })));
    assert_eq!(with(|m| m.set(dt::VERSYM, TOTAL as u64 - 6)), ElfError::SymbolIndex(3));
    assert_eq!(with(|m| m.set(dt::VERSYM, TOTAL as u64 + 2)), ElfError::AddressNotMapped(TOTAL as u64 + 2));
}

#[test]
fn verdef_malformed() {
    // no spec clause: D-27.8, revision, count, truncation of definitions and their names.
    assert_eq!(with(|m| m.p16(VERDEF_AT + 28, 2)), ElfError::BadVersionRevision(Table::Verdef, 2));
    assert_eq!(with(|m| m.set(dt::VERDEFNUM, 4)), ElfError::VersionCount(Table::Verdef, 56));
    assert_eq!(with(|m| m.p16(VERDEF_AT + 56 + 6, 3)), ElfError::VersionCount(Table::Verdef, 56));
    assert_eq!(with(|m| m.p32(VERDEF_AT + 28 + 16, 1000)), ElfError::VersionTruncated(Table::Verdef, 1028));
    assert_eq!(with(|m| m.p32(VERDEF_AT + 12, 1000)), ElfError::VersionTruncated(Table::Verdef, 1000));
    assert_eq!(with(|m| m.p32(VERDEF_AT + 80, u32::MAX)), ElfError::VersionTruncated(Table::Verdef, 76 + u32::MAX as usize));
    assert_eq!(with(|m| m.p32(VERDEF_AT + 20, 99)), ElfError::StringOutOfRange(99));
    assert_eq!(with(|m| m.set(dt::VERDEF, TOTAL as u64 + 8)), ElfError::AddressNotMapped(TOTAL as u64 + 8));
}

#[test]
fn verneed_malformed() {
    // no spec clause: D-27.8, revision, count, truncation of requirements and their versions.
    assert_eq!(with(|m| m.p16(VERNEED_AT, 0)), ElfError::BadVersionRevision(Table::Verneed, 0));
    assert_eq!(with(|m| m.set(dt::VERNEEDNUM, 2)), ElfError::VersionCount(Table::Verneed, 0));
    assert_eq!(with(|m| m.p16(VERNEED_AT + 2, 2)), ElfError::VersionCount(Table::Verneed, 0));
    assert_eq!(with(|m| m.p32(VERNEED_AT + 8, 200)), ElfError::VersionTruncated(Table::Verneed, 200));
    assert_eq!(with(|m| m.p32(VERNEED_AT + 4, 99)), ElfError::StringOutOfRange(99));
    assert_eq!(with(|m| m.p32(VERNEED_AT + 24, 99)), ElfError::StringOutOfRange(99));
    assert_eq!(with(|m| m.set(dt::VERNEED, TOTAL as u64 - 8)), ElfError::VersionTruncated(Table::Verneed, 0));
}

#[test]
fn version_tags_both_or_neither() {
    // no spec clause: D-27.8, an address tag without its count and the reverse.
    assert_eq!(with(|m| m.drop_tag(dt::VERDEFNUM)), ElfError::MissingDynamic(dt::VERDEFNUM));
    assert_eq!(with(|m| m.drop_tag(dt::VERNEED)), ElfError::MissingDynamic(dt::VERNEED));
}

#[test]
fn relocation_tags_checked() {
    // no spec clause: D-27.8, pairs, entry sizes, DT_PLTREL, sizes that are not whole entries, duplicates.
    assert_eq!(with(|m| m.drop_tag(dt::RELASZ)), ElfError::MissingDynamic(dt::RELASZ));
    assert_eq!(with(|m| m.drop_tag(dt::RELAENT)), ElfError::MissingDynamic(dt::RELAENT));
    assert_eq!(with(|m| m.set(dt::RELAENT, 16)), ElfError::BadDynamicValue(dt::RELAENT));
    assert_eq!(with(|m| m.set(dt::RELASZ, 70)), ElfError::TableSize(Table::Rela));
    assert_eq!(with(|m| m.set(dt::RELA, TOTAL as u64 - 48)), ElfError::AddressNotMapped(TOTAL as u64 - 48));
    assert_eq!(with(|m| m.drop_tag(dt::JMPREL)), ElfError::MissingDynamic(dt::JMPREL));
    assert_eq!(with(|m| m.drop_tag(dt::PLTREL)), ElfError::MissingDynamic(dt::PLTREL));
    assert_eq!(with(|m| m.set(dt::PLTREL, 17)), ElfError::BadDynamicValue(dt::PLTREL));
    assert_eq!(with(|m| m.set(dt::PLTRELSZ, 30)), ElfError::TableSize(Table::Jmprel));
    assert_eq!(with(|m| m.drop_tag(dt::RELRSZ)), ElfError::MissingDynamic(dt::RELRSZ));
    assert_eq!(with(|m| m.drop_tag(dt::RELRENT)), ElfError::MissingDynamic(dt::RELRENT));
    assert_eq!(with(|m| m.set(dt::RELRENT, 4)), ElfError::BadDynamicValue(dt::RELRENT));
    assert_eq!(with(|m| m.set(dt::RELRSZ, 20)), ElfError::TableSize(Table::Relr));
    assert_eq!(with(|m| m.retag(dt::RELRENT, dt::HASH, SYSV_AT as u64)), ElfError::DuplicateDynamic(dt::HASH));
    assert_eq!(with(|m| m.retag(dt::RELRENT, dt::RELASZ, 72)), ElfError::DuplicateDynamic(dt::RELASZ));
}

#[test]
fn relr_malformed() {
    // no spec clause: D-27.8, a bitmap before any address, addresses past the address space, a table
    // running past the mapped bytes.
    assert_eq!(with(|m| m.p64(RELR_AT, 0b1011)), ElfError::RelrNoBase(0));
    assert_eq!(with(|m| m.p64(RELR_AT, u64::MAX - 7)), ElfError::RelrOverflow(1));
    assert_eq!(with(|m| m.p64(RELR_AT, u64::MAX - 0x1ff)), ElfError::RelrOverflow(2));
    assert_eq!(with(|m| m.p64(RELR_AT, u64::MAX - 0xff)), ElfError::RelrOverflow(2));
    let high_bit = |m: &mut Img| {
        m.p64(RELR_AT, u64::MAX - 0xff);
        m.p64(RELR_AT + 8, 1 << 63 | 1);
    };
    assert_eq!(with(high_bit), ElfError::RelrOverflow(1));
    assert_eq!(with(|m| m.set(dt::RELRSZ, 32)), ElfError::AddressNotMapped(RELR_AT as u64));
}
