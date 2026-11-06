//! Symbol lookup (spec §27.1, phase 14 DL-9, plan/ph14/design.md §5): the GNU and SysV hash
//! functions, lookup in one object through either hash table, search scopes, version matching,
//! `STB_GNU_UNIQUE` and the weak rules. The rules are decision D-27.11 (plan/07-decisions/s26-29.md);
//! the version rules were settled by black-box probes of the reference loader
//! (spec/behaviour/dynlink.md, "Symbol binding").
//!
//! Pure logic over the [`crate::elf`] reader: no I/O, no allocation, no process. Every index from an
//! object goes through the reader's checked accessors, so a malformed table gives a [`LookupError`],
//! never a panic or a read outside the object. Nothing here is architecture-specific (design §0a).
//!
//! Version names are compared as bytes; no version node is special (`LIELLA_PRIVATE` and any
//! other name are ordinary nodes, ruling R14-12).

use core::cell::OnceCell;

use crate::elf::{
    Dynamic, ElfError, GnuHash, Sym, Symtab, SysvHash, Versym, SHN_UNDEF, STB_GLOBAL, STB_GNU_UNIQUE, STB_WEAK,
    STT_COMMON, STT_FUNC, STT_GNU_IFUNC, STT_NOTYPE, STT_OBJECT, STT_TLS, STV_HIDDEN, STV_INTERNAL, VERSYM_HIDDEN,
};

/// `VER_NDX_LOCAL`: the symbol is local to its object and never binds.
pub const VER_NDX_LOCAL: u16 = 0;
/// `VER_NDX_GLOBAL`: the symbol has the object's base version (it is unversioned).
pub const VER_NDX_GLOBAL: u16 = 1;

/// The SysV ELF hash of `name` (gABI "Hash Table" section).
#[must_use]
pub fn sysv_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in name {
        h = h.wrapping_shl(4).wrapping_add(u32::from(c));
        let g = h & 0xf000_0000;
        h ^= g.wrapping_shr(24);
        h &= !g;
    }
    h
}

/// The GNU hash of `name`: `h = h * 33 + c` from 5381, modulo 2^32.
#[must_use]
pub fn gnu_hash(name: &[u8]) -> u32 {
    name.iter().fold(5381u32, |h, &c| h.wrapping_mul(33).wrapping_add(u32::from(c)))
}

/// Why a lookup failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// A table of the object is malformed (the reader's error, with the index it was about).
    Elf(ElfError),
    /// The object has no `DT_SYMTAB`.
    NoSymtab,
    /// The object has no hash table of the style asked for (or none at all).
    NoHashTable,
    /// Symbol `sym` carries version index `index`, which no version definition (or, for a
    /// reference, no version requirement) of its object gives.
    UnknownVersion {
        /// The symbol index.
        sym: u32,
        /// The version index from `versym`.
        index: u16,
    },
    /// A scope names an object index that the object list does not have.
    NoObject(usize),
    /// A strong reference has no definition in its scope.
    Undefined,
    /// The unique-symbol registry could not record a definition (allocation failed).
    NoMemory,
}

impl From<ElfError> for LookupError {
    fn from(e: ElfError) -> Self {
        LookupError::Elf(e)
    }
}

/// Which hash table a lookup walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashStyle {
    /// `DT_GNU_HASH` (Bloom filter, buckets, chains).
    Gnu,
    /// `DT_HASH`.
    Sysv,
}

/// Which definitions a request accepts (D-27.11; spec/behaviour/dynlink.md "Symbol binding").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want<'n> {
    /// A relocation's reference without a version (its `versym` index is 0 or 1, or its object has no
    /// `versym`): the definition with the lowest version index, hidden or not, so the base definition
    /// first, then the oldest version.
    Oldest,
    /// A lookup by name alone (`dlsym`): a base or default (not hidden) definition only.
    Default,
    /// A named version (a reference's `vna_other` resolved to its name, or `dlvsym`): the definition of
    /// exactly that version, hidden or not. A base (index 1) definition never matches a name. In an
    /// object without `versym` every definition matches.
    Version(&'n [u8]),
}

/// One symbol request: a name and the definitions it accepts. The hashes are computed once per request
/// (the SysV one only when an object without a GNU table is searched).
#[derive(Clone, Debug)]
pub struct Request<'n> {
    name: &'n [u8],
    want: Want<'n>,
    gnu: u32,
    sysv: OnceCell<u32>,
}

impl<'n> Request<'n> {
    /// A request for `name` accepting `want`.
    #[must_use]
    pub fn new(name: &'n [u8], want: Want<'n>) -> Self {
        Request { name, want, gnu: gnu_hash(name), sysv: OnceCell::new() }
    }
    /// The name.
    #[must_use]
    pub fn name(&self) -> &'n [u8] {
        self.name
    }
    /// What the request accepts.
    #[must_use]
    pub fn want(&self) -> Want<'n> {
        self.want
    }
    fn sysv(&self) -> u32 {
        *self.sysv.get_or_init(|| sysv_hash(self.name))
    }
}

/// A definition found in one object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Def {
    /// The symbol index in the defining object.
    pub sym: u32,
    /// The symbol.
    pub st: Sym,
    /// Its version index (1, the base, when the object has no `versym`).
    pub version: u16,
    /// Whether that version is hidden (not the default).
    pub hidden: bool,
}

/// The hash table an [`Object`] walks.
#[derive(Clone, Copy, Debug)]
enum Hash<'a> {
    Gnu(GnuHash<'a>),
    Sysv(SysvHash<'a>),
}

/// The lookup view of one loaded object: its symbol table, one hash table and its version tables.
#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    dynm: Dynamic<'a>,
    symtab: Symtab<'a>,
    hash: Hash<'a>,
    versym: Option<Versym<'a>>,
}

/// Whether `s` is a definition another object may bind: defined, a global, weak or unique binding, a
/// data, code, TLS or IFUNC type, and not hidden or internal.
fn exported(s: &Sym) -> bool {
    s.shndx != SHN_UNDEF
        && matches!(s.bind(), STB_GLOBAL | STB_WEAK | STB_GNU_UNIQUE)
        && matches!(s.kind(), STT_NOTYPE | STT_OBJECT | STT_FUNC | STT_COMMON | STT_TLS | STT_GNU_IFUNC)
        && !matches!(s.visibility(), STV_HIDDEN | STV_INTERNAL)
}

impl<'a> Object<'a> {
    /// The object's lookup view, walking the GNU hash table when there is one, else the SysV table.
    ///
    /// # Errors
    ///
    /// [`LookupError::NoSymtab`], [`LookupError::NoHashTable`], or the reader's errors for the symbol,
    /// hash and version tables.
    pub fn new(dynm: Dynamic<'a>) -> Result<Self, LookupError> {
        let style = if dynm.gnu_hash()?.is_some() { HashStyle::Gnu } else { HashStyle::Sysv };
        Self::with_style(dynm, style)
    }

    /// The object's lookup view, walking the hash table of style `style`.
    ///
    /// # Errors
    ///
    /// As [`Object::new`]; [`LookupError::NoHashTable`] when the object has no table of that style.
    pub fn with_style(dynm: Dynamic<'a>, style: HashStyle) -> Result<Self, LookupError> {
        let symtab = dynm.symtab()?.ok_or(LookupError::NoSymtab)?;
        let hash = match style {
            HashStyle::Gnu => Hash::Gnu(dynm.gnu_hash()?.ok_or(LookupError::NoHashTable)?),
            HashStyle::Sysv => Hash::Sysv(dynm.sysv_hash()?.ok_or(LookupError::NoHashTable)?),
        };
        Ok(Object { dynm, symtab, hash, versym: dynm.versym()? })
    }

    /// The dynamic section the view reads.
    #[must_use]
    pub fn dynamic(&self) -> Dynamic<'a> {
        self.dynm
    }

    /// The symbol table.
    #[must_use]
    pub fn symtab(&self) -> Symtab<'a> {
        self.symtab
    }

    /// The name of version index `index` in this object's definitions (`None` for 0 and 1, which name no
    /// version, and for an index no definition gives).
    ///
    /// # Errors
    ///
    /// The reader's errors for the version definitions.
    pub fn version_name(&self, index: u16) -> Result<Option<&'a [u8]>, LookupError> {
        if index <= VER_NDX_GLOBAL {
            return Ok(None);
        }
        let Some(defs) = self.dynm.verdef()? else { return Ok(None) };
        for d in defs {
            let d = d?;
            if d.ndx & !VERSYM_HIDDEN == index {
                return Ok(d.names().next().transpose()?.map(|a| a.name));
            }
        }
        Ok(None)
    }

    /// What this object's reference through symbol `sym` accepts: [`Want::Oldest`] without a version,
    /// else the version its `versym` index names (a requirement's `vna_other`, or one of the object's own
    /// definitions).
    ///
    /// # Errors
    ///
    /// The reader's errors, or [`LookupError::UnknownVersion`] for an index neither table gives.
    pub fn reference_want(&self, sym: u32) -> Result<Want<'a>, LookupError> {
        let Some(vs) = self.versym else { return Ok(Want::Oldest) };
        let index = vs.get(sym)?.index;
        if index <= VER_NDX_GLOBAL {
            return Ok(Want::Oldest);
        }
        if let Some(needs) = self.dynm.verneed()? {
            for n in needs {
                for a in n?.auxes() {
                    let a = a?;
                    if a.other & !VERSYM_HIDDEN == index {
                        return Ok(Want::Version(a.name));
                    }
                }
            }
        }
        self.version_name(index)?.map(Want::Version).ok_or(LookupError::UnknownVersion { sym, index })
    }

    /// The candidates for `req` in the hash chain, as symbol indices (one error ends the walk).
    fn for_each_candidate(&self, req: &Request<'_>, mut f: impl FnMut(u32) -> Result<bool, LookupError>) -> Result<(), LookupError> {
        match self.hash {
            Hash::Gnu(g) => {
                let h = req.gnu;
                let words = g.bloom_size();
                let wi = usize::try_from(h.wrapping_shr(6)).ok().and_then(|w| w.checked_rem(words)).unwrap_or(0);
                let word = g.bloom_word(wi).ok_or(ElfError::BadHashHeader(crate::elf::Table::GnuHash))?;
                let second = u64::from(h).checked_shr(g.bloom_shift()).unwrap_or(0) & 63;
                let mask = 1u64.wrapping_shl(h & 63) | 1u64.wrapping_shl(u32::try_from(second).unwrap_or(0));
                if word & mask != mask {
                    return Ok(());
                }
                for e in g.chain(h) {
                    let e = e?;
                    if (e.hash | 1) == (h | 1) && f(e.sym)? {
                        return Ok(());
                    }
                }
            }
            Hash::Sysv(s) => {
                for i in s.chain(req.sysv()) {
                    if f(i?)? {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// The definition of `req` in this object, by the rules of [`Want`].
    ///
    /// # Errors
    ///
    /// The reader's errors for the hash chain, symbol, string and version tables, and
    /// [`LookupError::UnknownVersion`] for a definition whose version index no definition gives.
    pub fn lookup(&self, req: &Request<'_>) -> Result<Option<Def>, LookupError> {
        let mut best: Option<Def> = None;
        self.for_each_candidate(req, |i| {
            let st = self.symtab.get(i)?;
            if !exported(&st) || self.dynm.string(u64::from(st.name))? != req.name {
                return Ok(false);
            }
            let (version, hidden) = match self.versym {
                None => (VER_NDX_GLOBAL, false),
                Some(vs) => {
                    let v = vs.get(i)?;
                    (v.index, v.hidden)
                }
            };
            if version == VER_NDX_LOCAL {
                return Ok(false);
            }
            let def = Def { sym: i, st, version, hidden };
            match req.want {
                Want::Oldest => {
                    if best.is_none_or(|b| version < b.version) {
                        best = Some(def);
                    }
                    Ok(false)
                }
                Want::Default => {
                    let ok = version == VER_NDX_GLOBAL || !hidden;
                    if ok {
                        best = Some(def);
                    }
                    Ok(ok)
                }
                Want::Version(name) => {
                    // An object without `versym` matches any request here; a program that needs a
                    // version from such an object is refused earlier, by the load-time version
                    // check (D-27.11).
                    let ok = self.versym.is_none()
                        || (version > VER_NDX_GLOBAL
                            && self.version_name(version)?.ok_or(LookupError::UnknownVersion { sym: i, index: version })? == name);
                    if ok {
                        best = Some(def);
                    }
                    Ok(ok)
                }
            }
        })?;
        Ok(best)
    }
}

/// An ordered list of objects to search (indices into the namespace's object list): `first`, then
/// `second`, leaving out `exclude` (design §5).
#[derive(Clone, Copy, Debug, Default)]
pub struct Scope<'s> {
    first: &'s [usize],
    second: &'s [usize],
    exclude: Option<usize>,
}

impl<'s> Scope<'s> {
    /// The global scope alone: the main program, its dependencies in load order and the `RTLD_GLOBAL`
    /// objects in order (`RTLD_DEFAULT`, and the relocations of objects in the global scope).
    #[must_use]
    pub fn global(global: &'s [usize]) -> Self {
        Scope { first: global, second: &[], exclude: None }
    }

    /// The scope of an object loaded by `dlopen`: the global scope, then its local scope (the object's
    /// dependency tree, `local`); with `RTLD_DEEPBIND` the local scope comes first.
    #[must_use]
    pub fn group(global: &'s [usize], local: &'s [usize], deepbind: bool) -> Self {
        if deepbind { Scope { first: local, second: global, exclude: None } } else { Scope { first: global, second: local, exclude: None } }
    }

    /// `RTLD_NEXT` from `caller`: the objects after it in the global scope, or, for an object only in a
    /// local scope, the objects after it there; `None` when `caller` is in neither.
    #[must_use]
    pub fn next(global: &'s [usize], local: &'s [usize], caller: usize) -> Option<Self> {
        let after = |l: &'s [usize]| l.iter().position(|&o| o == caller).and_then(|p| l.get(p.checked_add(1)?..));
        after(global).or_else(|| after(local)).map(|first| Scope { first, second: &[], exclude: None })
    }

    /// This scope without object `obj` (a COPY relocation's lookup leaves out the program itself).
    #[must_use]
    pub fn excluding(self, obj: usize) -> Self {
        Scope { exclude: Some(obj), ..self }
    }

    /// The object indices in search order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + 's {
        let ex = self.exclude;
        self.first.iter().chain(self.second.iter()).copied().filter(move |&o| Some(o) != ex)
    }
}

/// A definition and the object (index into the object list) that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    /// The defining object.
    pub obj: usize,
    /// The definition.
    pub def: Def,
}

/// The namespace's table of `STB_GNU_UNIQUE` definitions, keyed by name and version name (empty for
/// an unversioned definition). The first definition looked up wins for the namespace; the owner of the
/// table also marks that object as not unloadable (design §5).
pub trait UniqueRegistry {
    /// The definition recorded for (`name`, `version`), if any.
    fn find(&self, name: &[u8], version: &[u8]) -> Option<Found>;
    /// Records `found` for (`name`, `version`).
    ///
    /// # Errors
    ///
    /// [`LookupError::NoMemory`] when the table cannot grow.
    fn record(&mut self, name: &[u8], version: &[u8], found: Found) -> Result<(), LookupError>;
}

/// The first definition of `req` in `scope` (object indices into `objects`). The first object in scope
/// order that defines the name wins, whether its definition is weak or strong. An `STB_GNU_UNIQUE`
/// definition is replaced by the one `unique` holds for its name and version, or recorded there.
///
/// # Errors
///
/// [`LookupError::NoObject`] for a scope index past `objects`, each object's [`Object::lookup`] errors,
/// and [`LookupError::NoMemory`] from the registry.
pub fn lookup(objects: &[Object<'_>], scope: Scope<'_>, req: &Request<'_>, unique: &mut dyn UniqueRegistry) -> Result<Option<Found>, LookupError> {
    for o in scope.iter() {
        let obj = objects.get(o).ok_or(LookupError::NoObject(o))?;
        let Some(def) = obj.lookup(req)? else { continue };
        let found = Found { obj: o, def };
        if def.st.bind() == STB_GNU_UNIQUE {
            let version = obj.version_name(def.version)?.unwrap_or(&[]);
            if let Some(first) = unique.find(req.name, version) {
                return Ok(Some(first));
            }
            unique.record(req.name, version, found)?;
        }
        return Ok(Some(found));
    }
    Ok(None)
}

/// How a reference binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    /// To this definition.
    Defined(Found),
    /// A weak reference without a definition: its value is 0.
    WeakUndefined,
}

/// [`lookup`] for a reference: `weak` says the reference is `STB_WEAK`, which may stay undefined.
///
/// # Errors
///
/// [`LookupError::Undefined`] for a strong reference without a definition, and the [`lookup`] errors.
pub fn resolve(objects: &[Object<'_>], scope: Scope<'_>, req: &Request<'_>, weak: bool, unique: &mut dyn UniqueRegistry) -> Result<Binding, LookupError> {
    match lookup(objects, scope, req, unique)? {
        Some(f) => Ok(Binding::Defined(f)),
        None if weak => Ok(Binding::WeakUndefined),
        None => Err(LookupError::Undefined),
    }
}
