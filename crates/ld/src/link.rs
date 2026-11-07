//! The interpreter's link invariant (L14-1, plan/ph14/design.md §0 and §1, D-27.13).
//!
//! `ld.so` relocates itself before any of its code may read an address from its data (design §1),
//! and the relocator handles only `RelocKind::Relative` entries and RELR words. So the linked
//! interpreter must have: no undefined dynamic symbol, no `DT_NEEDED`, no relocation of any other
//! kind (matched through [`RelocKind`], never a raw type number: §0a checklist item 2), and no text
//! relocations. [`check`] reports each of the four facts over the linked file; `cargo xtask so`
//! refuses an `ld.so` for which [`LinkReport::holds`] is false.

use crate::arch::{Arch, RelocKind};
use crate::elf::{Elf, ElfError};

/// `DT_TEXTREL` (ELF gABI, "Dynamic Section"): the object has relocations against read-only segments.
pub const DT_TEXTREL: u64 = 22;
/// `DT_FLAGS` (ELF gABI).
pub const DT_FLAGS: u64 = 30;
/// `DF_TEXTREL` in `DT_FLAGS` (ELF gABI): as `DT_TEXTREL`.
pub const DF_TEXTREL: u64 = 0x4;
/// `SHN_UNDEF` (ELF gABI): the section index of an undefined symbol.
const SHN_UNDEF: u16 = 0;

/// The four L14-1 facts of one linked object (counts, with the first offender of each).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkReport<'a> {
    /// The object has a dynamic array (`PT_DYNAMIC`); without one nothing else is checked.
    pub dynamic: bool,
    /// Undefined symbols in the dynamic symbol table (index 0, the null symbol, excluded).
    pub undefined: usize,
    /// The name of the first undefined symbol.
    pub first_undefined: Option<&'a [u8]>,
    /// `DT_NEEDED` entries.
    pub needed: usize,
    /// RELA entries (`DT_RELA` and `DT_JMPREL`) of kind [`RelocKind::Relative`].
    pub relative: usize,
    /// RELR words (`DT_RELR`).
    pub relr_words: usize,
    /// RELA entries of any other kind.
    pub other: usize,
    /// The kind of the first such entry.
    pub first_other: Option<RelocKind>,
    /// `DT_TEXTREL`, or `DF_TEXTREL` in `DT_FLAGS`.
    pub textrel: bool,
}

impl LinkReport<'_> {
    /// Whether the object meets L14-1: dynamic, nothing undefined, nothing needed, only relative
    /// relocations, no text relocations.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.dynamic && self.undefined == 0 && self.needed == 0 && self.other == 0 && !self.textrel
    }
}

/// The L14-1 facts of the linked object `bytes` (the file as written by the linker).
///
/// # Errors
///
/// The reader's errors ([`Elf::parse`], [`Elf::dynamic`] and the table accessors) for a malformed file.
pub fn check<A: Arch>(bytes: &[u8]) -> Result<LinkReport<'_>, ElfError> {
    let elf = Elf::parse::<A>(bytes)?;
    let mut r = LinkReport::default();
    let Some(d) = elf.dynamic()? else {
        return Ok(r);
    };
    r.dynamic = true;
    for name in d.needed() {
        name?;
        r.needed = r.needed.saturating_add(1);
    }
    // The symbols to look at: the hash table's count, and every symbol a relocation names (a GNU
    // hash table counts from its first hashed symbol on; unhashed, undefined symbols lie below it,
    // and an object with no hashed symbol may report fewer than it has).
    let mut count = d.symbol_count()?.unwrap_or(0);
    for table in [d.rela::<A>()?, d.jmprel::<A>()?].into_iter().flatten() {
        for rela in table {
            count = count.max(rela.sym.saturating_add(1));
        }
    }
    if let Some(syms) = d.symtab()? {
        for i in 1..count {
            let sym = syms.get(i)?;
            if sym.shndx == SHN_UNDEF {
                r.undefined = r.undefined.saturating_add(1);
                if r.first_undefined.is_none() {
                    r.first_undefined = Some(d.string(u64::from(sym.name))?);
                }
            }
        }
    }
    for table in [d.rela::<A>()?, d.jmprel::<A>()?].into_iter().flatten() {
        for rela in table {
            if rela.kind == RelocKind::Relative {
                r.relative = r.relative.saturating_add(1);
            } else {
                r.other = r.other.saturating_add(1);
                r.first_other.get_or_insert(rela.kind);
            }
        }
    }
    if let Some(relr) = d.relr()? {
        for word in relr {
            word?;
            r.relr_words = r.relr_words.saturating_add(1);
        }
    }
    r.textrel = d.get(DT_TEXTREL).is_some() || d.get(DT_FLAGS).is_some_and(|f| f & DF_TEXTREL != 0);
    Ok(r)
}
