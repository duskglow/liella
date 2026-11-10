//! The architecture seam of the loader (plan/ph14/design.md §0a, L14-13, A-1 `ARCH-PH14-NEUTRAL`).
//!
//! Everything in `liella-ld` that changes per architecture is a constant or a pure function of the
//! [`Arch`] trait, implemented by a zero-sized type per architecture. x86_64 ([`X86_64`]) is the only
//! implementation; the rest of the crate names no architecture and takes `A: Arch` (or [`Native`]).
//!
//! DL-2 (`DL-ELF-READ-1`) adds the first row of the §0a table, the ELF identity: the reader in
//! [`crate::elf`] checks `EI_CLASS`, `EI_DATA` and `e_machine` only through these constants. DL-3
//! (`DL-ELF-READ-2`) adds the second row, the relocation types: [`Arch::classify`] turns an `r_type`
//! into the arch-neutral [`RelocKind`], and the RELA iterators of [`crate::elf`] yield only that. Later
//! stories add the other rows (relocation arithmetic, TLS variant, loader name and paths, ...) here.

use liella_kernel::auxv::Auxv;
use liella_kernel::machine::Machine;
use liella_rt::cpu::Features;

/// `EI_CLASS` value of a 64-bit object (ELF gABI).
pub const ELFCLASS64: u8 = 2;
/// `EI_CLASS` value of a 32-bit object (ELF gABI).
pub const ELFCLASS32: u8 = 1;
/// `EI_DATA` value of a two's-complement little-endian object (ELF gABI).
pub const ELFDATA2LSB: u8 = 1;
/// `EI_DATA` value of a two's-complement big-endian object (ELF gABI).
pub const ELFDATA2MSB: u8 = 2;

/// One architecture's loader facts (design §0a). Associated constants only, so a second
/// implementation can be tested as pure logic on any host.
pub trait Arch {
    /// A short name for messages and tests.
    const NAME: &'static str;
    /// The `e_machine` value of this architecture's objects.
    const E_MACHINE: u16;
    /// The `EI_CLASS` of this architecture's objects. The reader decodes 64-bit objects only, so
    /// any other value makes every object a wrong-class error.
    const EI_CLASS: u8;
    /// The `EI_DATA` (byte order) of this architecture's objects; the reader decodes every field in
    /// this byte order.
    const EI_DATA: u8;
    /// The interpreter's file name (design §0a "Loader name, paths, strings"): the soname of the
    /// `ld.so` product, the name `cargo xtask so` gives it, and the last component of a program's
    /// `PT_INTERP` path. The directory part comes from the compile-time prefix (R14-3 (a), DL-8).
    const INTERP: &'static str;
    /// The kind of relocation type `r_type` (design §0a "Relocation types"). Every value maps to a
    /// kind: a type this table does not know is [`RelocKind::Unsupported`], never an error or a panic.
    fn classify(r_type: u32) -> RelocKind;
    /// The CPU features of this process (design §0a "auxv / CPU features"): the **only** code in the
    /// loader that turns the auxiliary vector (`AT_HWCAP`, `AT_HWCAP2`, `AT_PLATFORM`) or the CPU's
    /// own identification instructions (executed through `m`) into a [`Features`] value. The hwcaps
    /// search (§4) and the trampoline variant (§12) take that value only (DL-6, D-27.14).
    fn features(aux: &Auxv<'_>, m: &impl Machine) -> Features;
    /// The smallest page this architecture has (design §0a "Pages"): the page size when the
    /// auxiliary vector gives none or an unusable one.
    const BASE_PAGE: u64;
    /// The end of user address space: a fixed-address (`ET_EXEC`) object must end at or below it.
    const USER_END: u64;
    /// The page size the loader maps with (DL-7, D-27.15): `AT_PAGESZ` when it is a power of two
    /// not below [`Arch::BASE_PAGE`], otherwise [`Arch::BASE_PAGE`]. The only source of the page size
    /// in the loader.
    fn page_size(aux: &Auxv<'_>) -> u64 {
        match aux.get(AT_PAGESZ) {
            Some(p) if p.is_power_of_two() && p >= Self::BASE_PAGE => p,
            _ => Self::BASE_PAGE,
        }
    }
    /// The value of the `$LIB` dynamic string token (design §4, DL-8, D-27.16): a fixed string, no
    /// environment input (threat model rule 2).
    const LIB: &'static str;
    /// The system's default directories, in search order, searched after `<prefix>/lib` when the
    /// compile-time prefix is `/` (R14-3 (a), [`crate::search`]).
    const SYSTEM_DIRS: &'static [&'static str];
    /// The system directories that are trusted (threat model rule 5) when the prefix is `/`, besides
    /// `<prefix>/lib`.
    const SYSTEM_TRUSTED: &'static [&'static str];
    /// The `flags` an `ld.so.cache` entry for this architecture carries (spec Appendix D.3.1); an entry
    /// with other flags is skipped.
    const CACHE_FLAGS: u32;
    /// The `glibc-hwcaps` subdirectories `f` supports, highest priority first (design §4, #940).
    fn hwcaps(f: &Features) -> &'static [&'static str];
    /// The value of the `$PLATFORM` dynamic string token for `f` (a fixed string per CPU class).
    fn platform(f: &Features) -> &'static str;
}

/// `AT_PAGESZ` (kernel UAPI `linux/auxvec.h`): the system page size.
pub const AT_PAGESZ: u64 = 6;

/// What a relocation does, independent of the architecture's numbering (design §0a). Relocation code
/// matches on this, never on a raw `r_type` (§0a checklist item 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocKind {
    /// No operation.
    None,
    /// Load base plus addend (B + A).
    Relative,
    /// A word-sized absolute symbol value plus addend (S + A).
    Abs64,
    /// A GOT slot holding a symbol's address (S).
    GlobDat,
    /// A PLT slot holding a function's address (S).
    JumpSlot,
    /// Copy the symbol's data from its definition into the program.
    Copy,
    /// The result of calling the resolver at B + A (IFUNC).
    Irelative,
    /// The TLS module ID of the symbol's object.
    DtpMod,
    /// The symbol's offset in its module's TLS block.
    DtpOff,
    /// The symbol's offset from the thread pointer (static TLS).
    TpOff,
    /// A TLS descriptor (resolver and argument).
    TlsDesc,
    /// A field allowed only in text-relocated objects: `width` bytes, relative to the field's own
    /// address when `pc_relative`, and checked as a signed value when `signed` (a 32-bit field of a
    /// 64-bit value, sign- or zero-extended).
    TextRel {
        /// Field width in bytes.
        width: u8,
        /// The value is S + A - P rather than S + A.
        pc_relative: bool,
        /// The value must fit the field as a signed (rather than unsigned) number.
        signed: bool,
    },
    /// A type this architecture's table does not handle (the raw `r_type`).
    Unsupported(u32),
}

/// x86-64 (the x86-64 psABI): `EM_X86_64`, 64-bit, little-endian.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct X86_64;

impl Arch for X86_64 {
    const NAME: &'static str = "x86_64";
    /// `EM_X86_64` (ELF gABI machine list; x86-64 psABI §4.1).
    const E_MACHINE: u16 = 62;
    const EI_CLASS: u8 = ELFCLASS64;
    const EI_DATA: u8 = ELFDATA2LSB;
    /// The x86-64 psABI's program interpreter name (§5.2.1, "Program Interpreter").
    const INTERP: &'static str = "ld-linux-x86-64.so.2";
    /// The 4 KiB base page (x86-64 psABI §3.3.3 "Page size": systems may use larger pages).
    const BASE_PAGE: u64 = 4096;
    /// 47-bit user addresses (four-level paging; the kernel gives a program addresses above this
    /// only when it asks for them with a hint, which the loader never does).
    const USER_END: u64 = 1 << 47;

    /// x86-64's features come from CPUID and XGETBV ([`Features::detect`], D-4.25): the same decision
    /// libc's dispatch makes. `AT_HWCAP` on x86-64 is CPUID leaf 1 EDX only (a subset), so it is not read.
    fn features(aux: &Auxv<'_>, m: &impl Machine) -> Features {
        let _ = aux;
        Features::detect(m)
    }

    /// `$LIB` as the reference expands it in a `DT_RUNPATH` (spec/behaviour/dynlink.md, DL-0b probe).
    const LIB: &'static str = "lib/x86_64-linux-gnu";
    /// The reference's system search path as its `ld.so --help` prints it (DL-8 probe, D-27.16).
    const SYSTEM_DIRS: &'static [&'static str] = &["/lib/x86_64-linux-gnu", "/usr/lib/x86_64-linux-gnu", "/lib", "/usr/lib"];
    /// Threat model rule 5 (owner-approved, R14-15): `/lib64`, `/usr/lib64` and the multiarch pair.
    const SYSTEM_TRUSTED: &'static [&'static str] = &["/lib64", "/usr/lib64", "/lib/x86_64-linux-gnu", "/usr/lib/x86_64-linux-gnu"];
    /// Type 3 (ELF libc6) with ABI byte 3 (x86-64): `0x0303` (Appendix D.3.1; `0x0003` and `0x0803`
    /// were observed skipped by the reference x86-64 loader).
    const CACHE_FLAGS: u32 = 0x0303;

    /// The x86-64 psABI micro-architecture levels (§3.1.1, "Micro-Architecture Levels"), each
    /// subdirectory only when its level and every level below it are supported.
    fn hwcaps(f: &Features) -> &'static [&'static str] {
        const ALL: &[&str] = &["x86-64-v4", "x86-64-v3", "x86-64-v2"];
        let v2 = f.cx16() && f.lahf() && f.popcnt() && f.sse3() && f.sse41() && f.sse42() && f.ssse3();
        let v3 = v2 && f.avx() && f.avx2() && f.bmi1() && f.bmi2() && f.f16c() && f.fma() && f.lzcnt() && f.movbe();
        let v4 = v3 && f.avx512_v4();
        let skip = match (v4, v3, v2) {
            (true, _, _) => 0,
            (false, true, _) => 1,
            (false, false, true) => 2,
            _ => 3,
        };
        ALL.get(skip..).unwrap_or(&[])
    }

    /// `haswell` for the x86-64-v3 class (probed on a Haswell host, dynlink.md), `x86_64` otherwise.
    fn platform(f: &Features) -> &'static str {
        if Self::hwcaps(f).first().is_some_and(|h| *h != "x86-64-v2") { "haswell" } else { "x86_64" }
    }

    /// The `R_X86_64_*` numbers of the x86-64 psABI (§4.4.1, table "Relocation Types") the loader
    /// handles (design §6); every other number, the x32-only ones included, is `Unsupported`.
    fn classify(r_type: u32) -> RelocKind {
        match r_type {
            0 => RelocKind::None,                                                                // R_X86_64_NONE
            1 => RelocKind::Abs64,                                                               // R_X86_64_64
            2 => RelocKind::TextRel { width: 4, pc_relative: true, signed: true },               // R_X86_64_PC32
            5 => RelocKind::Copy,                                                                // R_X86_64_COPY
            6 => RelocKind::GlobDat,                                                             // R_X86_64_GLOB_DAT
            7 => RelocKind::JumpSlot,                                                            // R_X86_64_JUMP_SLOT
            8 => RelocKind::Relative,                                                            // R_X86_64_RELATIVE
            10 => RelocKind::TextRel { width: 4, pc_relative: false, signed: false },            // R_X86_64_32
            11 => RelocKind::TextRel { width: 4, pc_relative: false, signed: true },             // R_X86_64_32S
            16 => RelocKind::DtpMod,                                                             // R_X86_64_DTPMOD64
            17 => RelocKind::DtpOff,                                                             // R_X86_64_DTPOFF64
            18 => RelocKind::TpOff,                                                              // R_X86_64_TPOFF64
            36 => RelocKind::TlsDesc,                                                            // R_X86_64_TLSDESC
            37 => RelocKind::Irelative,                                                          // R_X86_64_IRELATIVE
            n => RelocKind::Unsupported(n),
        }
    }
}

/// The architecture this crate is built for.
///
/// Defined only on x86_64. Design §0a asks for a `compile_error!` on other targets until A-12; this
/// module leaves `Native` undefined there instead, because `cargo xtask arch-check` (A-2) builds
/// `liella-ld` for aarch64 and must keep passing: nothing in the crate uses `Native` yet, and the
/// first use on another target fails to compile just as the design intends.
// ARCH(x86_64): the loader's Arch seam (design §0a); A-12 adds the aarch64 arm.
#[cfg(target_arch = "x86_64")]
pub type Native = X86_64;
