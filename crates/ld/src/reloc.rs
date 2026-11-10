//! Eager relocation of loaded objects (phase 14 DL-10; plan/ph14/design.md §6, L14-6, L14-8; D-27.17).
//!
//! Pure logic: the objects are seen through the ELF reader (their files) and their load bias; every
//! store, load, resolver call and protection change goes through the [`Image`] trait, which the
//! boundary implements on the process's memory and the tests on a model. The order (L14-8):
//!
//! 1. [`check_versions`] for every object before anything is relocated (D-27.11, ruling #3655): a
//!    versioned reference to an object without version tables stops the load.
//! 2. [`relocate_all`]: objects in reverse load order (dependencies before the objects that need
//!    them, the program last). Inside one object: RELR, then RELA, then `JMPREL`; `IRELATIVE` entries
//!    and references bound to `STT_GNU_IFUNC` definitions are deferred to a second pass over the same
//!    tables, so a resolver runs only once its object and every object after it in load order are
//!    relocated (threat rule 6). `COPY` (program only) copies from a provider relocated before.
//! 3. [`relro_ranges`]: the caller makes each object's RELRO pages read-only after all of the above.
//!
//! A text-relocated object (`DT_TEXTREL` or `DF_TEXTREL`) has its read-only segments made writable
//! and not executable for the first pass and restored before the second (never W+X, L14-6); a
//! deferred store into such a segment is refused. Relocation code matches on [`RelocKind`] only.

use crate::arch::{ApplyError, Arch, Operands, RelocKind, ELFDATA2LSB};
use crate::elf::{dt, pt, Dynamic, Elf, ElfError, ProgramHeader, Rela, STB_LOCAL, STB_WEAK, STT_GNU_IFUNC, STV_DEFAULT, SHN_UNDEF};
use crate::map::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::symbols::{self, Binding, Found, LookupError, Object, Request, Scope, UniqueRegistry};

/// `SHN_ABS` (gABI): an absolute symbol, whose value the load bias does not move.
const SHN_ABS: u16 = 0xfff1;
/// `VER_FLG_WEAK` (GNU symbol-versioning document): a version whose absence is not fatal.
const VER_FLG_WEAK: u16 = 2;
/// `PF_X`, `PF_W`, `PF_R` (ELF gABI).
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// One loaded object as the relocator sees it.
#[derive(Clone, Copy, Debug)]
pub struct Obj<'a> {
    /// The object's file, through the reader.
    pub elf: Elf<'a>,
    /// Its dynamic section.
    pub dynm: Dynamic<'a>,
    /// Run-time address minus link-time address.
    pub bias: u64,
    /// The name it was loaded by (its `DT_NEEDED` string, or the program's path): messages, and the
    /// `vn_file` of a version need when the object has no `DT_SONAME`.
    pub name: &'a [u8],
}

/// The objects of one load, in load order (index 0 is the program), with their lookup views and the
/// global scope (indices in lookup order).
#[derive(Clone, Copy, Debug)]
pub struct Objects<'s, 'a> {
    /// The objects.
    pub objs: &'s [Obj<'a>],
    /// `syms[i]` is the lookup view of `objs[i]`.
    pub syms: &'s [Object<'a>],
    /// The global scope.
    pub global: &'s [usize],
}

/// The process memory the relocator writes. Addresses are run-time addresses; the relocator has
/// checked each range against the object's segments before a call.
pub trait Image {
    /// Store `bytes` at `at` (inside a segment that is writable at that moment).
    fn store(&mut self, at: u64, bytes: &[u8]);
    /// Read `out.len()` bytes at `at` (inside a mapped segment).
    fn load(&mut self, at: u64, out: &mut [u8]);
    /// Copy `len` bytes from `from` (a provider's segment) to `to` (the program's writable segment).
    fn copy(&mut self, to: u64, from: u64, len: u64);
    /// Call the IFUNC resolver at `resolver` and return its result.
    fn call(&mut self, resolver: u64) -> u64;
    /// Change the protection of `len` bytes at `at` (page multiples) to `prot`.
    #[allow(clippy::result_unit_err)]
    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()>;
}

/// Why an object could not be relocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocError<'a> {
    /// A relocation type the architecture's table does not handle (the raw `r_type`).
    Unsupported(u32),
    /// A TLS relocation (DL-12 adds them).
    Tls(u32),
    /// A text-field relocation in an object without `DT_TEXTREL` (design §6).
    TextRel(u32),
    /// A strong reference without a definition (the symbol's name).
    Undefined(&'a [u8]),
    /// A field outside the object's writable segments, or a copy source outside its provider.
    Outside(u64),
    /// The value does not fit the field.
    Overflow(u64),
    /// A `COPY` relocation outside the program, or one whose definition is the program's own.
    Copy(u64),
    /// The symbol tables are malformed.
    Lookup(LookupError),
    /// The relocation tables are malformed.
    Elf(ElfError),
    /// A protection change for a text-relocated object failed.
    Protect,
}

/// A relocation failure and the object (index into the load) it happened in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure<'a> {
    /// The object.
    pub obj: usize,
    /// What went wrong.
    pub error: RelocError<'a>,
}

impl<'a> RelocError<'a> {
    /// The diagnostic text (after `ld.so: <object>: `), without the symbol name of
    /// [`RelocError::Undefined`] (the caller prints it after this text).
    pub fn message(self) -> &'static [u8] {
        match self {
            RelocError::Unsupported(_) => b"unsupported relocation type\n",
            RelocError::Tls(_) => b"thread-local storage relocations are not available in this build\n",
            RelocError::TextRel(_) => b"relocation of a text field in an object not marked DT_TEXTREL\n",
            RelocError::Undefined(_) => b"undefined symbol: ",
            RelocError::Outside(_) => b"relocation outside the object's writable segments\n",
            RelocError::Overflow(_) => b"relocated value does not fit its field\n",
            RelocError::Copy(_) => b"COPY relocation outside the program or without a provider\n",
            RelocError::Lookup(_) => b"malformed symbol tables\n",
            RelocError::Elf(_) => b"malformed relocation tables\n",
            RelocError::Protect => b"cannot change the protection of a text-relocated segment\n",
        }
    }
}

/// The page range \[start, end) covering link-time range \[from, from + len), in run-time addresses.
fn pages(bias: u64, from: u64, len: u64, page: u64) -> (u64, u64) {
    let mask = !page.wrapping_sub(1);
    let start = bias.wrapping_add(from) & mask;
    let end = bias.wrapping_add(from).wrapping_add(len).wrapping_add(page.wrapping_sub(1)) & mask;
    (start, end)
}

/// The `mmap` protection of a segment's `p_flags`.
fn prot_of(flags: u32) -> i32 {
    let bit = |f: u32, p: i32| if flags & f != 0 { p } else { 0 };
    bit(PF_R, PROT_READ) | bit(PF_W, PROT_WRITE) | bit(PF_X, PROT_EXEC)
}

/// Whether \[at, at + len) (link-time) lies inside one `PT_LOAD`'s memory, writable (or any, when
/// `any`).
fn inside(elf: &Elf<'_>, at: u64, len: u64, any: bool) -> bool {
    let Some(end) = at.checked_add(len) else { return false };
    elf.program_headers().any(|p| {
        p.p_type == pt::LOAD && (any || p.flags & PF_W != 0) && p.vaddr <= at && p.vaddr.checked_add(p.memsz).is_some_and(|e| end <= e)
    })
}

/// Whether the object asks for text relocations.
fn textrel(d: &Dynamic<'_>) -> bool {
    d.get(dt::TEXTREL).is_some() || d.get(dt::FLAGS).is_some_and(|f| f & dt::DF_TEXTREL != 0)
}

/// What a symbol reference binds to.
#[derive(Clone, Copy, Debug)]
struct Target {
    /// S: the run-time value (the resolver's address for an IFUNC).
    s: u64,
    /// The definition is an `STT_GNU_IFUNC`: S is its resolver.
    ifunc: bool,
    /// The defining object (`None` for an undefined weak reference or symbol 0).
    obj: Option<usize>,
    /// The definition's size.
    size: u64,
}

/// The run-time value of definition `st` in object `o`.
fn value_of(set: &Objects<'_, '_>, o: usize, st: &crate::elf::Sym) -> u64 {
    let bias = if st.shndx == SHN_ABS { 0 } else { set.objs.get(o).map_or(0, |x| x.bias) };
    bias.wrapping_add(st.value)
}

/// Resolve symbol `sym` of object `i`. A defined local, or a defined symbol whose visibility is not
/// default, binds in its own object; every other reference is looked up in the global scope (without
/// the program for a `COPY`).
fn target<'a>(set: &Objects<'_, 'a>, i: usize, sym: u32, copy: bool, unique: &mut dyn UniqueRegistry) -> Result<Target, RelocError<'a>> {
    if sym == 0 {
        return Ok(Target { s: 0, ifunc: false, obj: None, size: 0 });
    }
    let me = set.syms.get(i).ok_or(RelocError::Lookup(LookupError::NoObject(i)))?;
    let st = me.symtab().get(sym).map_err(RelocError::Elf)?;
    if !copy && st.shndx != SHN_UNDEF && (st.bind() == STB_LOCAL || st.visibility() != STV_DEFAULT) {
        return Ok(Target { s: value_of(set, i, &st), ifunc: st.kind() == STT_GNU_IFUNC, obj: Some(i), size: st.size });
    }
    let dynm = set.objs.get(i).map(|o| o.dynm).ok_or(RelocError::Lookup(LookupError::NoObject(i)))?;
    let name = dynm.string(u64::from(st.name)).map_err(RelocError::Elf)?;
    let want = me.reference_want(sym).map_err(RelocError::Lookup)?;
    let mut scope = Scope::global(set.global);
    if copy {
        scope = scope.excluding(0);
    }
    match symbols::resolve(set.syms, scope, &Request::new(name, want), st.bind() == STB_WEAK, unique) {
        Ok(Binding::Defined(Found { obj, def })) => {
            Ok(Target { s: value_of(set, obj, &def.st), ifunc: def.st.kind() == STT_GNU_IFUNC, obj: Some(obj), size: def.st.size })
        }
        Ok(Binding::WeakUndefined) => Ok(Target { s: 0, ifunc: false, obj: None, size: 0 }),
        Err(LookupError::Undefined) => Err(RelocError::Undefined(name)),
        Err(e) => Err(RelocError::Lookup(e)),
    }
}

/// One relocation of object `i` in pass `deferred` (false: everything but the deferred entries;
/// true: only them). Returns whether the entry is deferred (pass 1) or was applied (pass 2).
fn one<'a, A: Arch, I: Image>(set: &Objects<'_, 'a>, i: usize, r: &Rela, deferred: bool, text: bool, img: &mut I, unique: &mut dyn UniqueRegistry) -> Result<bool, RelocError<'a>> {
    let o = set.objs.get(i).ok_or(RelocError::Lookup(LookupError::NoObject(i)))?;
    let p = o.bias.wrapping_add(r.offset);
    let ops = |s: u64| Operands { s, a: r.addend, b: o.bias, p };
    let field = |kind, s| match A::apply(kind, ops(s)) {
        Ok(f) => Ok(f),
        Err(ApplyError::Overflow) => Err(RelocError::Overflow(r.offset)),
        Err(ApplyError::NoArithmetic) => Err(RelocError::Unsupported(r.r_type)),
    };
    // Text fields may be written in pass 1 only (the segment is writable then).
    let put = |img: &mut I, f: Option<crate::arch::Field>, any: bool| -> Result<(), RelocError<'a>> {
        let Some(f) = f else { return Ok(()) };
        let w = usize::from(f.width);
        if !inside(&o.elf, r.offset, u64::from(f.width), any) {
            return Err(RelocError::Outside(r.offset));
        }
        let (le, be) = (f.value.to_le_bytes(), f.value.to_be_bytes());
        let bytes = if A::EI_DATA == ELFDATA2LSB { le.get(..w) } else { be.get(8usize.saturating_sub(w)..) };
        img.store(p, bytes.ok_or(RelocError::Overflow(r.offset))?);
        Ok(())
    };
    match r.kind {
        RelocKind::None => Ok(false),
        RelocKind::Relative if !deferred => put(img, field(r.kind, 0)?, text).map(|()| false),
        RelocKind::Relative => Ok(false),
        RelocKind::Irelative if !deferred => Ok(true),
        RelocKind::Irelative => {
            let resolver = field(r.kind, 0)?.map_or(0, |f| f.value);
            let v = img.call(resolver);
            put(img, Some(crate::arch::Field { value: v, width: 8 }), false).map(|()| true)
        }
        RelocKind::Abs64 | RelocKind::GlobDat | RelocKind::JumpSlot | RelocKind::TextRel { .. } => {
            if matches!(r.kind, RelocKind::TextRel { .. }) && !text {
                return Err(RelocError::TextRel(r.r_type));
            }
            let t = target(set, i, r.sym, false, unique)?;
            match (t.ifunc, deferred) {
                (false, false) => put(img, field(r.kind, t.s)?, text).map(|()| false),
                (false, true) => Ok(false),
                (true, false) => Ok(true),
                (true, true) => {
                    let s = img.call(t.s);
                    put(img, field(r.kind, s)?, false).map(|()| true)
                }
            }
        }
        RelocKind::Copy if deferred => Ok(false),
        RelocKind::Copy => {
            let me = set.syms.get(i).ok_or(RelocError::Lookup(LookupError::NoObject(i)))?;
            let size = me.symtab().get(r.sym).map_err(RelocError::Elf)?.size;
            let t = target(set, i, r.sym, true, unique)?;
            let src = t.obj.filter(|&d| i == 0 && d != 0).and_then(|d| set.objs.get(d)).ok_or(RelocError::Copy(r.offset))?;
            // The program's size; a smaller definition copies only what it has.
            let len = size.min(t.size);
            if !inside(&src.elf, t.s.wrapping_sub(src.bias), len, true) || !inside(&o.elf, r.offset, size, false) {
                return Err(RelocError::Outside(r.offset));
            }
            img.copy(p, t.s, len);
            Ok(false)
        }
        RelocKind::DtpMod | RelocKind::DtpOff | RelocKind::TpOff | RelocKind::TlsDesc => Err(RelocError::Tls(r.r_type)),
        RelocKind::Unsupported(n) => Err(RelocError::Unsupported(n)),
    }
}

/// Make the non-writable `PT_LOAD`s of a text-relocated object writable and not executable (`open`)
/// or give them back their own protection.
fn text_segments<I: Image>(o: &Obj<'_>, img: &mut I, page: u64, open: bool) -> Result<(), ()> {
    for ph in o.elf.program_headers().filter(|p: &ProgramHeader| p.p_type == pt::LOAD && p.memsz != 0 && p.flags & PF_W == 0) {
        let (start, end) = pages(o.bias, ph.vaddr, ph.memsz, page);
        let prot = if open { PROT_READ | PROT_WRITE } else { prot_of(ph.flags) };
        img.protect(start, end.wrapping_sub(start), prot)?;
    }
    Ok(())
}

/// Relocate object `i` of the load `set` (design §6 order inside one object). `page` is the page size.
///
/// # Errors
///
/// The first [`RelocError`] met.
pub fn relocate<'a, A: Arch, I: Image>(set: &Objects<'_, 'a>, i: usize, img: &mut I, page: u64, unique: &mut dyn UniqueRegistry) -> Result<(), RelocError<'a>> {
    let o = *set.objs.get(i).ok_or(RelocError::Lookup(LookupError::NoObject(i)))?;
    let text = textrel(&o.dynm);
    if text {
        text_segments(&o, img, page, true).map_err(|()| RelocError::Protect)?;
    }
    let first = pass1::<A, I>(set, i, &o, text, img, unique);
    if text {
        // Restored before any resolver can run, also on failure.
        text_segments(&o, img, page, false).map_err(|()| RelocError::Protect)?;
    }
    if first? {
        for table in [o.dynm.rela::<A>(), o.dynm.jmprel::<A>()] {
            for r in table.map_err(RelocError::Elf)?.into_iter().flatten() {
                one::<A, I>(set, i, &r, true, text, img, unique)?;
            }
        }
    }
    Ok(())
}

/// Pass 1 of object `o` (index `i`): RELR, RELA, `JMPREL`. Whether any entry was deferred.
fn pass1<'a, A: Arch, I: Image>(set: &Objects<'_, 'a>, i: usize, o: &Obj<'a>, text: bool, img: &mut I, unique: &mut dyn UniqueRegistry) -> Result<bool, RelocError<'a>> {
    for at in o.dynm.relr().map_err(RelocError::Elf)?.into_iter().flatten() {
        let at = at.map_err(RelocError::Elf)?;
        if !inside(&o.elf, at, 8, text) {
            return Err(RelocError::Outside(at));
        }
        let p = o.bias.wrapping_add(at);
        let mut w = [0u8; 8];
        img.load(p, &mut w);
        let v = if A::EI_DATA == ELFDATA2LSB { u64::from_le_bytes(w) } else { u64::from_be_bytes(w) }.wrapping_add(o.bias);
        img.store(p, &if A::EI_DATA == ELFDATA2LSB { v.to_le_bytes() } else { v.to_be_bytes() });
    }
    let mut deferred = false;
    for table in [o.dynm.rela::<A>(), o.dynm.jmprel::<A>()] {
        for r in table.map_err(RelocError::Elf)?.into_iter().flatten() {
            deferred |= one::<A, I>(set, i, &r, false, text, img, unique)?;
        }
    }
    Ok(deferred)
}

/// Relocate every object of `set`, in reverse load order (the program last; L14-8).
///
/// # Errors
///
/// The first failure, with the object it happened in.
pub fn relocate_all<'a, A: Arch, I: Image>(set: &Objects<'_, 'a>, img: &mut I, page: u64, unique: &mut dyn UniqueRegistry) -> Result<(), Failure<'a>> {
    for i in (0..set.objs.len()).rev() {
        relocate::<A, I>(set, i, img, page, unique).map_err(|error| Failure { obj: i, error })?;
    }
    Ok(())
}

/// The RELRO pages of object `o` at run time (start, length), whole pages inside `PT_GNU_RELRO`.
pub fn relro_range(o: &Obj<'_>, page: u64) -> Option<(u64, u64)> {
    let r = o.elf.find(pt::GNU_RELRO)?;
    let mask = !page.wrapping_sub(1);
    let start = o.bias.wrapping_add(r.vaddr) & mask;
    let end = o.bias.wrapping_add(r.vaddr).wrapping_add(r.memsz) & mask;
    (end > start).then(|| (start, end.wrapping_sub(start)))
}

/// Make every object's RELRO pages read-only (after [`relocate_all`]; L14-8).
///
/// # Errors
///
/// The index of the object whose protection change failed.
pub fn protect_relro<I: Image>(objs: &[Obj<'_>], img: &mut I, page: u64) -> Result<(), usize> {
    for (i, o) in objs.iter().enumerate() {
        if let Some((at, len)) = relro_range(o, page) {
            img.protect(at, len, PROT_READ).map_err(|()| i)?;
        }
    }
    Ok(())
}

/// A failed load-time version check (D-27.11, ruling #3655).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionError<'a> {
    /// Object `by` needs `version` of `file`, and the loaded object for `file` has no version
    /// definitions at all.
    NoVersions {
        /// The needing object.
        by: usize,
        /// The needed file (`vn_file`).
        file: &'a [u8],
        /// The version name.
        version: &'a [u8],
    },
    /// Object `by` needs `version` of `file`, which the loaded object does not define.
    Missing {
        /// The needing object.
        by: usize,
        /// The needed file (`vn_file`).
        file: &'a [u8],
        /// The version name.
        version: &'a [u8],
    },
    /// The version tables of object `.0` are malformed.
    Elf(usize, ElfError),
}

/// The object of the load that `file` (a `vn_file`) names: its `DT_SONAME`, or the name it was
/// loaded by.
fn provider(objs: &[Obj<'_>], file: &[u8]) -> Option<usize> {
    objs.iter().position(|o| o.dynm.get(dt::SONAME).and_then(|s| o.dynm.string(s).ok()) == Some(file) || o.name == file)
}

/// Check every `DT_VERNEED` entry of every object against the named object's `DT_VERDEF` (D-27.11):
/// a needed version (not `VER_FLG_WEAK`) of an object without version definitions, or one it does not
/// define, stops the load. A `vn_file` that names no loaded object is not checked here (the load of
/// that dependency has failed already).
///
/// # Errors
///
/// The first [`VersionError`], objects in load order.
pub fn check_versions<'a>(objs: &[Obj<'a>]) -> Result<(), VersionError<'a>> {
    for (by, o) in objs.iter().enumerate() {
        let Some(needs) = o.dynm.verneed().map_err(|e| VersionError::Elf(by, e))? else { continue };
        for vn in needs {
            let vn = vn.map_err(|e| VersionError::Elf(by, e))?;
            let Some(p) = provider(objs, vn.file).and_then(|j| objs.get(j)) else { continue };
            let defs = p.dynm.verdef().map_err(|e| VersionError::Elf(by, e))?;
            for aux in vn.auxes() {
                let aux = aux.map_err(|e| VersionError::Elf(by, e))?;
                if aux.flags & VER_FLG_WEAK != 0 {
                    continue;
                }
                let Some(defs) = defs.clone() else {
                    return Err(VersionError::NoVersions { by, file: vn.file, version: aux.name });
                };
                let mut found = false;
                for d in defs {
                    let d = d.map_err(|e| VersionError::Elf(by, e))?;
                    if d.names().next().and_then(Result::ok).is_some_and(|n| n.name == aux.name) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(VersionError::Missing { by, file: vn.file, version: aux.name });
                }
            }
        }
    }
    Ok(())
}

/// The `PT_GNU_PROPERTY` properties of `elf` this architecture knows (L14-12: parsed, then ignored by
/// the loader): the count of known property entries. Malformed notes end the walk (they are ignored
/// like the properties themselves).
pub fn known_properties<A: Arch>(elf: &Elf<'_>) -> usize {
    let Some(ph) = elf.find(pt::GNU_PROPERTY) else { return 0 };
    let Ok(notes) = elf.notes(&ph) else { return 0 };
    notes.map_while(Result::ok).flat_map(|n| n.properties().map_while(Result::ok)).filter(|p| A::PROPERTIES.iter().any(|&(t, _)| t == p.pr_type)).count()
}

/// A fixed-size [`UniqueRegistry`] for a load without an allocator: up to `N` (name, version) keys
/// stored in a `B`-byte arena. A full table answers [`LookupError::NoMemory`].
#[derive(Debug)]
pub struct UniqueTable<const N: usize, const B: usize> {
    keys: [(usize, usize, usize); N],
    found: [Option<Found>; N],
    n: usize,
    arena: [u8; B],
    used: usize,
}

impl<const N: usize, const B: usize> Default for UniqueTable<N, B> {
    fn default() -> Self {
        UniqueTable { keys: [(0, 0, 0); N], found: [None; N], n: 0, arena: [0; B], used: 0 }
    }
}

impl<const N: usize, const B: usize> UniqueRegistry for UniqueTable<N, B> {
    fn find(&self, name: &[u8], version: &[u8]) -> Option<Found> {
        let key = |&(at, nl, vl): &(usize, usize, usize)| {
            let n = self.arena.get(at..at.checked_add(nl)?)?;
            let v = self.arena.get(at.checked_add(nl)?..at.checked_add(nl)?.checked_add(vl)?)?;
            Some((n, v))
        };
        self.keys.iter().zip(&self.found).take(self.n).find(|(k, _)| key(k) == Some((name, version))).and_then(|(_, f)| *f)
    }

    fn record(&mut self, name: &[u8], version: &[u8], found: Found) -> Result<(), LookupError> {
        let at = self.used;
        let end = at.checked_add(name.len()).and_then(|e| e.checked_add(version.len())).ok_or(LookupError::NoMemory)?;
        let mid = at.checked_add(name.len()).ok_or(LookupError::NoMemory)?;
        let slot = self.keys.get_mut(self.n).ok_or(LookupError::NoMemory)?;
        self.arena.get_mut(at..mid).ok_or(LookupError::NoMemory)?.copy_from_slice(name);
        self.arena.get_mut(mid..end).ok_or(LookupError::NoMemory)?.copy_from_slice(version);
        *slot = (at, name.len(), version.len());
        if let Some(s) = self.found.get_mut(self.n) {
            *s = Some(found);
        }
        self.n = self.n.wrapping_add(1);
        self.used = end;
        Ok(())
    }
}
