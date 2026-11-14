//! `ld.so`'s start decisions (phase 14 DL-6, plan/ph14/design.md §1 steps 3 to 5, D-27.14). Pure
//! logic over words and byte strings the boundary (`boundary/src/ld.rs`) reads from the initial
//! stack; nothing here touches memory it was not given.
//!
//! * [`secure`]: secure mode is `AT_SECURE` only (owner approval #2661, spec/security/threat-model.md
//!   "Who clears, and when"). The boundary removes `rt::start::UNSECURE` from the environment array
//!   before [`ld_env`] reads any `LD_*` name.
//! * [`program`]: the program the kernel mapped (`AT_PHDR`, `AT_PHNUM`, `AT_PHENT`, `AT_ENTRY`),
//!   refused when the vector is malformed (a missing entry, a wrong header size, a count of zero or
//!   one the gABI reserves).
//! * [`rewrite`]: for `ld.so PROGRAM`, the auxiliary vector entries that describe the program the
//!   loader mapped instead of the loader itself.
//! * [`ld_env`], [`preload_names`]: the `LD_*` settings, read once.
//! * [`bias`]: the mapped program's load bias, from `PT_PHDR` or, without one, its ELF header.
//! * [`needs_objects`]: whether a dynamic array names a `DT_NEEDED` object (loading dependencies
//!   comes with DL-8; until then such a program is refused).
//! * [`private`]: the C library's private start interface (`LIELLA_PRIVATE`, design §14, L14-10,
//!   D-27.22): the build tag both products carry, [`private::StartInfo`] and the lookup of
//!   `__liella_libc_early_init` with the tag check.

use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::{AT_BASE, AT_ENTRY, AT_PHDR, AT_PHENT, AT_PHNUM, AT_SECURE};

/// `sizeof(Elf64_Phdr)`: the only program-header size the loader reads.
pub const PHENT: u64 = 56;

/// `PN_XNUM` (ELF gABI): an `e_phnum` of this value means the real count is elsewhere (section
/// header 0). The kernel never passes it for an executable it mapped; the loader refuses it and
/// anything above it.
pub const PN_XNUM: u64 = 0xffff;

/// `DT_NULL` and `DT_NEEDED` (ELF gABI dynamic tags).
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;

/// The program the kernel mapped, from the auxiliary vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program {
    /// Run-time address of the program headers (`AT_PHDR`).
    pub phdr: usize,
    /// Number of program headers (`AT_PHNUM`), 1 to `PN_XNUM - 1`.
    pub phnum: usize,
    /// Run-time entry point (`AT_ENTRY`).
    pub entry: usize,
}

/// Why the auxiliary vector does not describe a program the loader can start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// No `AT_PHDR` (or zero).
    NoPhdr,
    /// No `AT_ENTRY` (or zero).
    NoEntry,
    /// `AT_PHENT` present and not [`PHENT`].
    BadPhent,
    /// `AT_PHNUM` missing, zero, or `PN_XNUM` and above.
    BadPhnum,
}

impl StartError {
    /// The diagnostic line (standard error) for a refused start.
    pub fn message(self) -> &'static [u8] {
        match self {
            StartError::NoPhdr => b"ld.so: the auxiliary vector has no program headers (AT_PHDR)\n",
            StartError::NoEntry => b"ld.so: the auxiliary vector has no entry point (AT_ENTRY)\n",
            StartError::BadPhent => b"ld.so: the auxiliary vector has a wrong program header size (AT_PHENT)\n",
            StartError::BadPhnum => b"ld.so: the auxiliary vector has a bad program header count (AT_PHNUM)\n",
        }
    }
}

/// Secure mode: `AT_SECURE` present and non-zero, and nothing else (owner approval #2661; the
/// hosted rule on real and effective IDs stays in the tests). The same rule as libc's start.
pub fn secure(aux: &Auxv<'_>) -> bool {
    aux.get(AT_SECURE).is_some_and(|v| v != 0)
}

/// The program the kernel mapped. A missing `AT_PHENT` is taken as [`PHENT`] (the kernel always
/// passes it; the value is checked when present).
///
/// # Errors
///
/// [`StartError`] for a malformed vector.
pub fn program(aux: &Auxv<'_>) -> Result<Program, StartError> {
    let word = |key| aux.get(key).filter(|&v| v != 0).and_then(|v| usize::try_from(v).ok());
    let phdr = word(AT_PHDR).ok_or(StartError::NoPhdr)?;
    let entry = word(AT_ENTRY).ok_or(StartError::NoEntry)?;
    if aux.get(AT_PHENT).is_some_and(|v| v != PHENT) {
        return Err(StartError::BadPhent);
    }
    let phnum = aux.get(AT_PHNUM).filter(|&n| n != 0 && n < PN_XNUM).ok_or(StartError::BadPhnum)?;
    let phnum = usize::try_from(phnum).map_err(|_| StartError::BadPhnum)?;
    Ok(Program { phdr, phnum, entry })
}

/// For `ld.so PROGRAM`: make the auxiliary vector `words` (key/value pairs, up to `AT_NULL`)
/// describe `prog`, which the loader mapped, with the loader itself at `base` (`AT_BASE`, as when
/// the kernel starts the loader for a program's `PT_INTERP`). Only entries already present change.
pub fn rewrite(words: &mut [u64], prog: &Program, base: usize) {
    let as_word = |v: usize| u64::try_from(v).unwrap_or(u64::MAX);
    for pair in words.chunks_exact_mut(2) {
        let value = match pair.first().copied() {
            Some(0) | None => return,
            Some(AT_PHDR) => as_word(prog.phdr),
            Some(AT_PHNUM) => as_word(prog.phnum),
            Some(AT_PHENT) => PHENT,
            Some(AT_ENTRY) => as_word(prog.entry),
            Some(AT_BASE) => as_word(base),
            Some(_) => continue,
        };
        if let Some(v) = pair.get_mut(1) {
            *v = value;
        }
    }
}

/// The `LD_*` settings the loader reads (ld.so(8) "Environment"), read once, after secure-mode
/// clearing. The first entry of a name counts, as for `getenv`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LdEnv<'a> {
    /// `LD_PRELOAD`: objects to load before the program's dependencies.
    pub preload: Option<&'a [u8]>,
    /// `LD_LIBRARY_PATH`: directories searched before the defaults (DL-8).
    pub library_path: Option<&'a [u8]>,
    /// `LD_BIND_NOW` set to a non-empty string: resolve every symbol at start (DL-10, DL-13).
    pub bind_now: bool,
    /// `GLIBC_TUNABLES` (D-1.8): parsed by the caller with the secure filter.
    pub tunables: Option<&'a [u8]>,
    /// `LD_TRACE_LOADED_OBJECTS` is set, to any value (ld.so(8)); the loader ignores it in secure
    /// mode (threat model rule 11, DL-8).
    pub trace: bool,
    /// `LD_ORIGIN_PATH` (ld.so(8)); the loader ignores it in secure mode (rule 2, DL-8).
    pub origin_path: Option<&'a [u8]>,
}

/// The value of `name` in the environment entry `entry` (`NAME=value`), if it is that name's entry.
pub fn value_of<'a>(entry: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    entry.strip_prefix(name)?.strip_prefix(b"=")
}

/// The loader's settings from the environment entries `env` (each `NAME=value`, without the NUL).
/// The caller passes the environment after secure-mode clearing (threat model rule 1).
pub fn ld_env<'a>(env: impl IntoIterator<Item = &'a [u8]>) -> LdEnv<'a> {
    let mut out = LdEnv::default();
    let mut bind_now = None;
    for e in env {
        let first = |slot: &mut Option<&'a [u8]>, name: &[u8]| {
            if slot.is_none() {
                *slot = value_of(e, name);
            }
        };
        first(&mut out.preload, b"LD_PRELOAD");
        first(&mut out.library_path, b"LD_LIBRARY_PATH");
        first(&mut out.tunables, b"GLIBC_TUNABLES");
        first(&mut bind_now, b"LD_BIND_NOW");
        first(&mut out.origin_path, b"LD_ORIGIN_PATH");
        out.trace |= value_of(e, b"LD_TRACE_LOADED_OBJECTS").is_some();
    }
    out.bind_now = bind_now.is_some_and(|v| !v.is_empty());
    out
}

/// The object names in an `LD_PRELOAD` value: separated by spaces or colons (ld.so(8)), empty
/// names skipped.
pub fn preload_names(value: &[u8]) -> impl Iterator<Item = &[u8]> {
    value.split(|&b| b == b' ' || b == b':').filter(|n| !n.is_empty())
}

/// The diagnostic for a preload object this loader does not load yet (standard error, in order):
/// the line format of the reference's "cannot be preloaded ... ignored" diagnostic (probe: a
/// nonexistent `LD_PRELOAD` object under the reference), with this loader's reason. The program
/// still runs, as there.
pub fn preload_ignored(name: &[u8]) -> [&[u8]; 3] {
    [b"ERROR: ld.so: object '", name, b"' from LD_PRELOAD cannot be preloaded (loading objects is not available in this build): ignored.\n"]
}

/// Whether the dynamic array `words` (tag/value pairs, up to `DT_NULL`) names a `DT_NEEDED` object.
pub fn needs_objects(words: &[u64]) -> bool {
    words.chunks_exact(2).map(|p| p.first().copied().unwrap_or(DT_NULL)).take_while(|&t| t != DT_NULL).any(|t| t == DT_NEEDED)
}

/// The diagnostic for a program with dependencies (DL-8 loads them).
pub const NEEDS_OBJECTS: &[u8] = b"ld.so: the program needs shared objects; loading them is not available in this build\n";

/// The load bias of a mapped program (D-27.18, ruling #4371): `PT_PHDR`'s, or without one the
/// one its ELF header and the `PT_LOAD` holding its program headers give. One implementation,
/// in libc's start (`liella_rt::start::bias`), shared with libc's own lookups of the executable.
pub use liella_rt::start::bias;

/// The private start interface between `ld.so` and `libc.so.6` (version node `LIELLA_PRIVATE`,
/// plan/ph14/design.md §14, L14-10, R14-12, D-27.22). Never in a public header.
pub mod private {
    use crate::reloc::Obj;
    use crate::symbols::{Object, Request, Want};

    pub use liella_rt::start::private::{BUILD_TAG, EARLY_INIT, NODE, StartInfo, TAG_LEN, TAG_PREFIX, THREAD_WALK};

    /// The loader's build tag (`liella_rt::start::private::tag`), compared with the C library's.
    pub const TAG: [u8; TAG_LEN] = liella_rt::start::private::tag(option_env!("LIELLA_BUILD_TAG"));

    /// [`TAG_LEN`] as a symbol size.
    const TAG_SIZE: u64 = 64;

    /// The C library's early-init entry found among the loaded objects.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Found {
        /// The object defining it.
        pub obj: usize,
        /// Its run-time address.
        pub early_init: u64,
    }

    /// Why the private interface refuses a pair (the object's index).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PrivateError {
        /// The object's tag is missing, malformed or not this loader's: built from another commit.
        Mismatch(usize),
        /// The object's symbol tables cannot be read.
        Malformed(usize),
    }

    impl PrivateError {
        /// The diagnostic after the object's name (standard error).
        pub fn message(self) -> &'static [u8] {
            match self {
                PrivateError::Mismatch(_) => b": LIELLA_PRIVATE build tag differs from the dynamic loader's (the C library and ld.so come from different builds)\n",
                PrivateError::Malformed(_) => b": malformed LIELLA_PRIVATE symbols\n",
            }
        }
    }

    /// The first object but `skip` (the loader itself) that defines [`EARLY_INIT`] at [`NODE`], once its
    /// [`BUILD_TAG`] is checked against [`TAG`] (L14-10): the tag must be a [`TAG_LEN`]-byte object
    /// inside the file part of a `PT_LOAD`, read through `read(address, out)` (the object is mapped).
    /// `None`: no object has the interface (a program without Liella's C library).
    ///
    /// # Errors
    ///
    /// [`PrivateError`] for the object whose tag differs or whose tables cannot be read.
    pub fn find(objs: &[Obj<'_>], syms: &[Object<'_>], skip: Option<usize>, mut read: impl FnMut(u64, &mut [u8])) -> Result<Option<Found>, PrivateError> {
        let (early, tag) = (Request::new(EARLY_INIT, Want::Version(NODE)), Request::new(BUILD_TAG, Want::Version(NODE)));
        for (k, (o, s)) in objs.iter().zip(syms).enumerate().filter(|&(k, _)| Some(k) != skip) {
            let Some(def) = s.lookup(&early).map_err(|_| PrivateError::Malformed(k))? else { continue };
            let t = s.lookup(&tag).map_err(|_| PrivateError::Malformed(k))?.filter(|t| t.st.size == TAG_SIZE);
            let Some(t) = t.filter(|t| {
                let end = t.st.value.checked_add(TAG_SIZE);
                o.elf.program_headers().any(|p| p.p_type == crate::elf::pt::LOAD && p.vaddr <= t.st.value && end.is_some_and(|e| e <= p.vaddr.saturating_add(p.filesz)))
            }) else {
                return Err(PrivateError::Mismatch(k));
            };
            let mut theirs = [0u8; TAG_LEN];
            read(o.bias.wrapping_add(t.st.value), &mut theirs);
            if theirs != TAG {
                return Err(PrivateError::Mismatch(k));
            }
            return Ok(Some(Found { obj: k, early_init: o.bias.wrapping_add(def.st.value) }));
        }
        Ok(None)
    }
}
