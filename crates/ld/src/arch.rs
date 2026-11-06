//! The architecture seam of the loader (plan/ph14/design.md §0a, L14-13, A-1 `ARCH-PH14-NEUTRAL`).
//!
//! Everything in `liella-ld` that changes per architecture is a constant or a pure function of the
//! [`Arch`] trait, implemented by a zero-sized type per architecture. x86_64 ([`X86_64`]) is the only
//! implementation; the rest of the crate names no architecture and takes `A: Arch` (or [`Native`]).
//!
//! DL-2 (`DL-ELF-READ-1`) adds the first row of the §0a table, the ELF identity: the reader in
//! [`crate::elf`] checks `EI_CLASS`, `EI_DATA` and `e_machine` only through these constants. Later
//! stories add the other rows (relocation kinds, TLS variant, loader name and paths, ...) here.

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
}

/// The architecture this crate is built for.
///
/// Defined only on x86_64. Design §0a asks for a `compile_error!` on other targets until A-12; this
/// module leaves `Native` undefined there instead, because `cargo xtask arch-check` (A-2) builds
/// `liella-ld` for aarch64 and must keep passing: nothing in the crate uses `Native` yet, and the
/// first use on another target fails to compile just as the design intends.
#[cfg(target_arch = "x86_64")]
pub type Native = X86_64;
