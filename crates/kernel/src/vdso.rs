//! vDSO symbol lookup over a byte slice (spec Appendix A.2 "vDSO"; ELF is a normative reference,
//! OD-2). This is the ELF subset of IP-24 needed before the dynamic linker exists: the dynamic
//! section, the symbol and string tables, both hash tables, and symbol versions.
//!
//! The boundary turns the kernel's mapping (`AT_SYSINFO_EHDR`) into a slice: first
//! [`HEADER_LEN`] bytes, then [`image_len`] bytes. Everything here is bounds-checked; a
//! malformed image yields `None`, never a panic.

/// Bytes of the ELF header.
pub const HEADER_LEN: usize = 64;

/// Largest image accepted (the x86_64 vDSO is two pages; this leaves room to grow).
pub const MAX_IMAGE: usize = 1 << 20;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const DT_NULL: u64 = 0;
const DT_HASH: u64 = 4;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_GNU_HASH: u64 = 0x6fff_fef5;
const DT_VERSYM: u64 = 0x6fff_fff0;
const DT_VERDEF: u64 = 0x6fff_fffc;
const SYM_SIZE: usize = 24;
const STT_FUNC: u8 = 2;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const SHN_UNDEF: u16 = 0;

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([s.first().copied()?, s.get(1).copied()?]))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    let mut w = [0u8; 4];
    w.copy_from_slice(s);
    Some(u32::from_le_bytes(w))
}

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off.checked_add(8)?)?;
    let mut w = [0u8; 8];
    w.copy_from_slice(s);
    Some(u64::from_le_bytes(w))
}

fn usz(v: u64) -> Option<usize> {
    usize::try_from(v).ok()
}

/// Validate an ELF header (64-bit, little-endian, `ET_DYN`, x86-64) and return the image
/// length it describes: the end of the section header table or of the last program segment,
/// whichever is later. `None` if the header is not a plausible vDSO.
pub fn image_len(header: &[u8]) -> Option<usize> {
    if header.get(..4)? != b"\x7fELF" || *header.get(4)? != 2 || *header.get(5)? != 1 {
        return None;
    }
    if u16_at(header, 16)? != 3 || u16_at(header, 18)? != 62 {
        return None;
    }
    let shoff = usz(u64_at(header, 40)?)?;
    let shentsize = usize::from(u16_at(header, 58)?);
    let shnum = usize::from(u16_at(header, 60)?);
    let phoff = usz(u64_at(header, 32)?)?;
    let phentsize = usize::from(u16_at(header, 54)?);
    let phnum = usize::from(u16_at(header, 56)?);
    let sh_end = shoff.checked_add(shentsize.checked_mul(shnum)?)?;
    let ph_end = phoff.checked_add(phentsize.checked_mul(phnum)?)?;
    let len = sh_end.max(ph_end).max(HEADER_LEN);
    (len <= MAX_IMAGE).then_some(len)
}

/// A parsed vDSO image.
#[derive(Clone, Copy, Debug)]
pub struct Vdso<'a> {
    image: &'a [u8],
    /// Subtract from a virtual address to get an offset in `image`.
    bias: u64,
    symtab: usize,
    strtab: usize,
    nsyms: usize,
    versym: Option<usize>,
    verdef: Option<usize>,
}

impl<'a> Vdso<'a> {
    /// Parse `image` (the whole mapping). `None` if anything required is missing or out of
    /// bounds.
    pub fn parse(image: &'a [u8]) -> Option<Vdso<'a>> {
        image_len(image)?;
        let phoff = usz(u64_at(image, 32)?)?;
        let phentsize = usize::from(u16_at(image, 54)?);
        let phnum = usize::from(u16_at(image, 56)?);
        let mut bias = None;
        let mut dynamic = None;
        for i in 0..phnum {
            let ph = phoff.checked_add(i.checked_mul(phentsize)?)?;
            let ptype = u32_at(image, ph)?;
            let offset = u64_at(image, ph.checked_add(8)?)?;
            let vaddr = u64_at(image, ph.checked_add(16)?)?;
            if ptype == PT_LOAD && bias.is_none() {
                bias = Some(vaddr.checked_sub(offset)?);
            } else if ptype == PT_DYNAMIC {
                dynamic = Some((offset, u64_at(image, ph.checked_add(32)?)?));
            }
        }
        let bias = bias?;
        let (dyn_off, dyn_size) = dynamic?;
        let dyn_off = usz(dyn_off)?;
        let (mut gnu_hash, mut versym, mut verdef) = (None, None, None);
        // STARTUP-READ-CONSTANTS: the low tags (`DT_HASH`, `DT_STRTAB`, `DT_SYMTAB`) index this
        // array. A `match` over all tags compiled to a jump table in `.rodata`, one more
        // read-only page that start-up touched, wherever unrelated code moved it. A later entry
        // of a tag replaces an earlier one, as the `match` did.
        let mut low: [Option<usize>; 7] = [None; 7];
        let mut i = 0usize;
        while i < usz(dyn_size)? / 16 {
            let e = dyn_off.checked_add(i.checked_mul(16)?)?;
            let tag = u64_at(image, e)?;
            let val = u64_at(image, e.checked_add(8)?)?;
            let off = || val.checked_sub(bias).and_then(usz);
            if tag == DT_NULL {
                break;
            } else if let Some(slot) = usz(tag).and_then(|t| low.get_mut(t)) {
                *slot = off();
            } else if tag == DT_GNU_HASH {
                gnu_hash = off();
            } else if tag == DT_VERSYM {
                versym = off();
            } else if tag == DT_VERDEF {
                verdef = off();
            }
            i = i.checked_add(1)?;
        }
        let low_tag = |t: u64| usz(t).and_then(|t| low.get(t).copied().flatten());
        let (hash, strtab, symtab) = (low_tag(DT_HASH), low_tag(DT_STRTAB), low_tag(DT_SYMTAB));
        let nsyms = match (hash, gnu_hash) {
            (Some(h), _) => usz(u64::from(u32_at(image, h.checked_add(4)?)?))?,
            (None, Some(g)) => gnu_hash_count(image, g)?,
            (None, None) => return None,
        };
        Some(Vdso { image, bias, symtab: symtab?, strtab: strtab?, nsyms, versym, verdef })
    }

    fn c_str(&self, off: usize) -> Option<&'a [u8]> {
        let rest = self.image.get(off..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        rest.get(..end)
    }

    /// Name of version index `ndx` from the version definitions.
    fn version_name(&self, ndx: u16) -> Option<&'a [u8]> {
        let mut def = self.verdef?;
        loop {
            let vd_ndx = u16_at(self.image, def.checked_add(4)?)?;
            let vd_aux = usz(u64::from(u32_at(self.image, def.checked_add(12)?)?))?;
            let vd_next = usz(u64::from(u32_at(self.image, def.checked_add(16)?)?))?;
            if vd_ndx & 0x7fff == ndx & 0x7fff {
                let aux = def.checked_add(vd_aux)?;
                let name = usz(u64::from(u32_at(self.image, aux)?))?;
                return self.c_str(self.strtab.checked_add(name)?);
            }
            if vd_next == 0 {
                return None;
            }
            def = def.checked_add(vd_next)?;
        }
    }

    /// The offset in the image of the defined function `name` with version `version` (for
    /// example `__vdso_clock_gettime`, `LINUX_2.6`). If the image has no version table, any
    /// definition matches.
    pub fn lookup(&self, name: &[u8], version: &[u8]) -> Option<usize> {
        let [r] = self.lookup_many(&[name], version);
        r
    }

    /// [`Vdso::lookup`] of each of `names` (with the same `version`) in one pass over the symbols
    /// (SETLOCALE-2: five separate walks cost ~5.5k instructions on the first heap slow path). The
    /// answers are those of separate lookups, also for a damaged image: a symbol entry or name that
    /// cannot be read ends every lookup still open (each would have stopped there), a version or
    /// address that cannot be read ends only the lookups of that symbol's name.
    pub fn lookup_many<const N: usize>(&self, names: &[&[u8]; N], version: &[u8]) -> [Option<usize>; N] {
        let mut out = [None; N];
        let mut open = [true; N];
        let mut left = N;
        for i in 0..self.nsyms {
            if left == 0 {
                break;
            }
            let Some((sym_name, value)) = self.defined_function(i) else { return out };
            let Some(sym_name) = sym_name else { continue };
            for k in 0..N {
                let (Some(o), Some(&want)) = (open.get(k).copied(), names.get(k)) else { continue };
                if !o || sym_name != want {
                    continue;
                }
                match self.version_ok(i, version) {
                    Some(false) => continue,
                    Some(true) => {
                        if let Some(slot) = out.get_mut(k) {
                            *slot = value.checked_sub(self.bias).and_then(usz).filter(|&off| off < self.image.len());
                        }
                    }
                    None => {}
                }
                if let Some(o) = open.get_mut(k) {
                    *o = false;
                }
                left = left.saturating_sub(1);
            }
        }
        out
    }

    /// Symbol `i`: `Some((Some(name), value))` for a defined global or weak function,
    /// `Some((None, _))` for any other symbol, `None` when the entry or its name cannot be read.
    fn defined_function(&self, i: usize) -> Option<(Option<&'a [u8]>, u64)> {
        let sym = self.symtab.checked_add(i.checked_mul(SYM_SIZE)?)?;
        let st_name = usz(u64::from(u32_at(self.image, sym)?))?;
        let info = *self.image.get(sym.checked_add(4)?)?;
        let shndx = u16_at(self.image, sym.checked_add(6)?)?;
        let value = u64_at(self.image, sym.checked_add(8)?)?;
        if info & 0xf != STT_FUNC || !matches!(info >> 4, STB_GLOBAL | STB_WEAK) || shndx == SHN_UNDEF {
            return Some((None, value));
        }
        Some((Some(self.c_str(self.strtab.checked_add(st_name)?)?), value))
    }

    /// Whether symbol `i` has `version` (always, without a version table); `None` when its
    /// version index cannot be read.
    fn version_ok(&self, i: usize, version: &[u8]) -> Option<bool> {
        let Some(vs) = self.versym else { return Some(true) };
        let ndx = u16_at(self.image, vs.checked_add(i.checked_mul(2)?)?)?;
        Some(self.version_name(ndx) == Some(version))
    }

    /// The image offsets of the symbol table and the version index table, and the symbol count
    /// (for the tests, which build damaged images).
    #[doc(hidden)]
    pub fn layout(&self) -> (usize, Option<usize>, usize) {
        (self.symtab, self.versym, self.nsyms)
    }

    /// The single-name walk [`Vdso::lookup_many`] must answer like (for the tests).
    #[doc(hidden)]
    pub fn lookup_one_by_one(&self, name: &[u8], version: &[u8]) -> Option<usize> {
        for i in 0..self.nsyms {
            let sym = self.symtab.checked_add(i.checked_mul(SYM_SIZE)?)?;
            let st_name = usz(u64::from(u32_at(self.image, sym)?))?;
            let info = *self.image.get(sym.checked_add(4)?)?;
            let shndx = u16_at(self.image, sym.checked_add(6)?)?;
            let value = u64_at(self.image, sym.checked_add(8)?)?;
            if info & 0xf != STT_FUNC || !matches!(info >> 4, STB_GLOBAL | STB_WEAK) || shndx == SHN_UNDEF {
                continue;
            }
            if self.c_str(self.strtab.checked_add(st_name)?)? != name {
                continue;
            }
            if let Some(vs) = self.versym {
                let ndx = u16_at(self.image, vs.checked_add(i.checked_mul(2)?)?)?;
                if self.version_name(ndx) != Some(version) {
                    continue;
                }
            }
            let off = usz(value.checked_sub(self.bias)?)?;
            return (off < self.image.len()).then_some(off);
        }
        None
    }
}

/// Number of symbols from a GNU hash table: one past the highest symbol any chain reaches.
fn gnu_hash_count(image: &[u8], at: usize) -> Option<usize> {
    let nbuckets = usz(u64::from(u32_at(image, at)?))?;
    let symoffset = usz(u64::from(u32_at(image, at.checked_add(4)?)?))?;
    let bloom_size = usz(u64::from(u32_at(image, at.checked_add(8)?)?))?;
    let buckets = at.checked_add(16)?.checked_add(bloom_size.checked_mul(8)?)?;
    let chains = buckets.checked_add(nbuckets.checked_mul(4)?)?;
    let mut last = 0usize;
    for b in 0..nbuckets {
        last = last.max(usz(u64::from(u32_at(image, buckets.checked_add(b.checked_mul(4)?)?)?))?);
    }
    if last < symoffset {
        return Some(symoffset);
    }
    loop {
        let h = u32_at(image, chains.checked_add(last.checked_sub(symoffset)?.checked_mul(4)?)?)?;
        last = last.checked_add(1)?;
        if h & 1 != 0 {
            return Some(last);
        }
    }
}
