//! `dlopen`'s decisions that need no process state (phase 14 DL-16a `DL-DLOPEN-LOAD`,
//! plan/ph14/dl-16-dlopen.md §1, §3, §4; spec/behaviour/dynlink.md, "dlerror texts" and probes P-1,
//! P-2, P-13; D-27.34): the mode word, the verdict on a candidate file, the open count and the
//! dlerror texts. The boundary driver (`crates/boundary/src/ld.rs`) opens, maps, relocates and runs.

use crate::elf::{et, pt, Elf, ElfError};

/// `RTLD_LAZY` (`<dlfcn.h>`). Bound eagerly, like `RTLD_NOW`, until DL-20 brings lazy binding.
pub const RTLD_LAZY: i32 = 0x1;
/// `RTLD_NOW`.
pub const RTLD_NOW: i32 = 0x2;
/// `RTLD_NOLOAD`: never map; the handle of an object already loaded, or NULL with no text.
pub const RTLD_NOLOAD: i32 = 0x4;
/// `RTLD_DEEPBIND`: the new set looks in its own local scope before the global scope.
pub const RTLD_DEEPBIND: i32 = 0x8;
/// `RTLD_GLOBAL`: the opened object's local scope joins the global scope.
pub const RTLD_GLOBAL: i32 = 0x100;
/// `RTLD_NODELETE`: the object is never unloaded.
pub const RTLD_NODELETE: i32 = 0x1000;
/// Every defined mode bit; a mode with any other bit is refused before the search (P-1, D-27.34).
pub const KNOWN: i32 = RTLD_LAZY | RTLD_NOW | RTLD_NOLOAD | RTLD_DEEPBIND | RTLD_GLOBAL | RTLD_NODELETE;

/// `DT_FLAGS_1` (GNU extension of the gABI dynamic tags).
pub const DT_FLAGS_1: u64 = 0x6fff_fffb;
/// `DF_1_NODELETE` in `DT_FLAGS_1`: the object is never unloaded.
pub const DF_1_NODELETE: u64 = 0x8;
/// `DF_1_PIE` in `DT_FLAGS_1`: a position-independent executable.
pub const DF_1_PIE: u64 = 0x0800_0000;

/// A mode word that [`mode`] accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    /// `RTLD_NOLOAD`.
    pub noload: bool,
    /// `RTLD_GLOBAL`.
    pub global: bool,
    /// `RTLD_DEEPBIND`.
    pub deepbind: bool,
    /// `RTLD_NODELETE`.
    pub nodelete: bool,
}

/// Why a mode word is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeError {
    /// A bit outside [`KNOWN`] (checked first, so also without a binding mode; P-1).
    Unknown,
    /// Neither `RTLD_LAZY` nor `RTLD_NOW`.
    NoBinding,
}

impl ModeError {
    /// The dlerror text. [`ModeError::NoBinding`]'s follows the name as given and `": "`
    /// ([`ModeError::prefixed`]); [`ModeError::Unknown`]'s stands alone (probed).
    pub fn text(self) -> &'static [u8] {
        match self {
            ModeError::Unknown => b"invalid mode parameter",
            ModeError::NoBinding => b"invalid mode for dlopen(): Invalid argument",
        }
    }

    /// Whether the text follows the object's name.
    pub fn prefixed(self) -> bool {
        self == ModeError::NoBinding
    }
}

/// The mode word `m` of a `dlopen` call.
///
/// # Errors
///
/// [`ModeError::Unknown`] for any bit outside [`KNOWN`], else [`ModeError::NoBinding`] without
/// `RTLD_LAZY` or `RTLD_NOW` (both together are accepted).
pub fn mode(m: i32) -> Result<Mode, ModeError> {
    if m & !KNOWN != 0 {
        return Err(ModeError::Unknown);
    }
    if m & (RTLD_LAZY | RTLD_NOW) == 0 {
        return Err(ModeError::NoBinding);
    }
    Ok(Mode { noload: m & RTLD_NOLOAD != 0, global: m & RTLD_GLOBAL != 0, deepbind: m & RTLD_DEEPBIND != 0, nodelete: m & RTLD_NODELETE != 0 })
}

/// The text of a name that names no loadable file (also an existing file of another machine).
pub const FILE_MISSING: &[u8] = b"cannot open shared object file: No such file or directory";

/// Why a candidate file is not loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Shorter than an ELF header.
    TooShort,
    /// No ELF magic.
    NotElf,
    /// `EI_CLASS` is `ELFCLASS32`.
    Class32,
    /// Another `EI_CLASS` value.
    Class,
    /// `EI_DATA` is not this machine's byte order.
    Data,
    /// `EI_OSABI` is neither System V nor GNU.
    OsAbi,
    /// `e_type` is neither `ET_DYN` nor `ET_EXEC`.
    Type,
    /// `e_machine` is another machine's.
    Machine,
    /// A header range lies past the end of the file.
    Truncated,
    /// Any other malformed header.
    Malformed,
    /// An `ET_EXEC` executable.
    Executable,
    /// A position-independent executable (`DF_1_PIE`).
    PieExecutable,
    /// A directory.
    Directory,
    /// No dynamic section.
    NoDynamic,
    /// A `PT_TLS` segment: the static surplus for `dlopen`ed modules is DL-16d's (plan §5).
    StaticTls,
}

impl Refusal {
    /// The text after `<name>: ` (spec/behaviour/dynlink.md, "dlerror texts").
    pub fn text(self) -> &'static [u8] {
        match self {
            Refusal::TooShort => b"file too short",
            Refusal::NotElf | Refusal::Class => b"invalid ELF header",
            Refusal::Class32 => b"wrong ELF class: ELFCLASS32",
            // ARCH(x86_64): this machine's objects are little-endian (aarch64's too).
            Refusal::Data => b"ELF file data encoding not little-endian",
            Refusal::OsAbi => b"ELF file OS ABI invalid",
            Refusal::Type => b"only ET_DYN and ET_EXEC can be loaded",
            Refusal::Machine => FILE_MISSING,
            Refusal::Truncated => b"cannot read file data",
            Refusal::Malformed => b"malformed ELF object",
            Refusal::Executable => b"cannot dynamically load executable",
            Refusal::PieExecutable => b"cannot dynamically load position-independent executable",
            Refusal::Directory => b"cannot read file data: Is a directory",
            Refusal::NoDynamic => b"object has no dynamic section",
            Refusal::StaticTls => b"cannot allocate memory in static TLS block",
        }
    }

    /// Whether a candidate refused for this is passed over and the search goes on (plan §1): another
    /// machine's object always (probed, also by path), and, in a search (`searched`), an object of
    /// another class, byte order or OS ABI.
    pub fn skipped(self, searched: bool) -> bool {
        match self {
            Refusal::Machine => true,
            Refusal::Class32 | Refusal::Class | Refusal::Data | Refusal::OsAbi => searched,
            _ => false,
        }
    }
}

/// The refusal for a header the reader rejected.
pub fn of_elf_error(e: ElfError) -> Refusal {
    match e {
        ElfError::TooShort => Refusal::TooShort,
        ElfError::BadMagic => Refusal::NotElf,
        ElfError::WrongClass(1) => Refusal::Class32,
        ElfError::WrongClass(_) => Refusal::Class,
        ElfError::WrongData(_) => Refusal::Data,
        ElfError::UnsupportedOsAbi(_) => Refusal::OsAbi,
        ElfError::UnsupportedType(_) => Refusal::Type,
        ElfError::WrongMachine(_) => Refusal::Machine,
        ElfError::OutOfRange(_) => Refusal::Truncated,
        _ => Refusal::Malformed,
    }
}

/// Whether a parsed candidate may be loaded by `dlopen`: executables are refused (P-13), an object
/// needs a dynamic section, and an object with `PT_TLS` waits for DL-16d.
///
/// # Errors
///
/// The [`Refusal`].
pub fn loadable(elf: &Elf<'_>) -> Result<(), Refusal> {
    if elf.header().e_type == et::EXEC {
        return Err(Refusal::Executable);
    }
    let Ok(Some(d)) = elf.dynamic() else { return Err(Refusal::NoDynamic) };
    if d.get(DT_FLAGS_1).is_some_and(|f| f & DF_1_PIE != 0) {
        return Err(Refusal::PieExecutable);
    }
    if elf.find(pt::TLS).is_some() {
        return Err(Refusal::StaticTls);
    }
    Ok(())
}

/// The open count after one more `dlopen` of an object opened `opens` times, and whether the object
/// must now stay loaded for good: the count saturates at `u32::MAX` and pins the object, so a
/// wrapped count can never unload a live object (plan §1, DL-16-PLAN pending decision 3).
pub fn reopen(opens: u32) -> (u32, bool) {
    match opens.checked_add(1) {
        Some(n) => (n, n == u32::MAX),
        None => (u32::MAX, true),
    }
}

/// Write `parts` one after another into `buf`, cut to leave room for the NUL written after them:
/// the length written (without the NUL). An empty `buf` gets nothing.
pub fn compose(buf: &mut [u8], parts: &[&[u8]]) -> usize {
    let room = buf.len().saturating_sub(1);
    let mut n = 0usize;
    for part in parts {
        for &b in *part {
            if n >= room {
                break;
            }
            if let Some(slot) = buf.get_mut(n) {
                *slot = b;
            }
            n = n.saturating_add(1);
        }
    }
    if let Some(slot) = buf.get_mut(n) {
        *slot = 0;
    }
    n
}

/// Whether `name` is a bare name (no `/`): matched against the loaded objects' `DT_SONAME` before
/// any search (P-2), never against their file names.
pub fn bare(name: &[u8]) -> bool {
    !name.contains(&b'/')
}
