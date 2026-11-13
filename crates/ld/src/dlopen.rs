//! `dlopen`'s decisions that need no process state (phase 14 DL-16a `DL-DLOPEN-LOAD`,
//! plan/ph14/dl-16-dlopen.md §1, §3, §4; spec/behaviour/dynlink.md, "dlerror texts" and probes P-1,
//! P-2, P-13; D-27.34): the mode word, the verdict on a candidate file, the open count and the
//! dlerror texts. The boundary driver (`crates/boundary/src/ld.rs`) opens, maps, relocates and runs.

use crate::arch::Arch;
use crate::elf::{et, Elf, ElfError};
use crate::map::{self, MapError};

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
    ///
    /// The bits `0x01000000` to `0x80000000` (eight bits) are reserved for Liella: they are
    /// refused here today, owe glibc's internal use of them nothing, and stay available for a
    /// possible future Liella-defined meaning (none planned; any use is a new public-interface
    /// decision, not a change made here).
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
    /// Another `EI_CLASS` value (probed: the same text as `ELFCLASS32`, tools/probe/dl/gaps).
    Class,
    /// `EI_VERSION` is not `EV_CURRENT`.
    IdentVersion,
    /// `e_version` is not `EV_CURRENT`.
    Version,
    /// `e_phentsize` is not the ELF64 program header size.
    Phentsize,
    /// No `PT_LOAD`, or none holding the program headers.
    NoSegments,
    /// A malformed `PT_LOAD` layout (misaligned, unsorted, overlapping, too large).
    Segments,
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
    /// A `PT_TLS` block that does not fit in the static surplus (plan §5; dynamic blocks are
    /// DL-19's), or one that cannot have a place there (its alignment, no module slot left).
    StaticTls,
}

impl Refusal {
    /// The text after `<name>: ` (spec/behaviour/dynlink.md, "dlerror texts").
    pub fn text(self) -> &'static [u8] {
        match self {
            Refusal::TooShort => b"file too short",
            Refusal::NotElf => b"invalid ELF header",
            Refusal::Class32 | Refusal::Class => b"wrong ELF class: ELFCLASS32",
            Refusal::IdentVersion => b"ELF file version ident does not match current one",
            Refusal::Version => b"ELF file version does not match current one",
            Refusal::Phentsize => b"ELF file's phentsize not the expected size",
            Refusal::NoSegments => b"object file has no loadable segments",
            Refusal::Segments => b"ELF load command address/offset not page-aligned",
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
            Refusal::NoDynamic => b"object file has no dynamic section",
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
        ElfError::BadIdentVersion(_) => Refusal::IdentVersion,
        ElfError::BadVersion(_) => Refusal::Version,
        ElfError::BadPhentsize(_) => Refusal::Phentsize,
        ElfError::OutOfRange(_) => Refusal::Truncated,
        _ => Refusal::Malformed,
    }
}

/// The refusal for a file the mapping planner rejected.
pub fn of_map_error(e: MapError) -> Refusal {
    match e {
        MapError::Elf(e) => of_elf_error(e),
        MapError::NotLoadable => Refusal::Type,
        MapError::NoSegments => Refusal::NoSegments,
        _ => Refusal::Segments,
    }
}

/// The verdict on a candidate file's bytes, in the probed order (tools/probe/dl/gaps): the header,
/// then the loadable segments, then [`loadable`]. `page` is the page size.
///
/// # Errors
///
/// The [`Refusal`].
pub fn check<A: Arch>(file: &[u8], page: u64) -> Result<Elf<'_>, Refusal> {
    let elf = Elf::parse::<A>(file).map_err(of_elf_error)?;
    if elf.header().e_type == et::EXEC {
        return Err(Refusal::Executable);
    }
    map::plan::<A>(&elf, page).map_err(of_map_error)?;
    loadable(&elf)?;
    Ok(elf)
}

/// Whether a parsed candidate may be loaded by `dlopen`: executables are refused (P-13), an object
/// needs a dynamic section. A `PT_TLS` segment is placed later, in the static surplus (DL-16d).
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

/// Whether `name` is a bare name (no `/`): matched against the loaded objects' `DT_SONAME` before
/// any search (P-2), never against their file names.
pub fn bare(name: &[u8]) -> bool {
    !name.contains(&b'/')
}
