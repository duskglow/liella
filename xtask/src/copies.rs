//! Direct internal copies (D-X.15), a step of `cargo xtask so`.
//!
//! rustc lowers slice copies, moves and fills to calls to `memcpy`, `memmove` and `memset`
//! through the GOT (`call *memcpy@GOTPCREL(%rip)`, relocation `R_X86_64_GOTPCREL`, which the
//! linker may not relax: stable rustc does not emit the relaxable `R_X86_64_GOTPCRELX`). In the
//! products those names are the exported, checked C entries. This pass edits the archive rustc
//! built, before either product is made from it:
//!
//! 1. In the member that defines `memcpy` (the LTO object), every relocation naming `memcpy`,
//!    `memmove` or `memset` on a call or jump, or on a load of the call target from the GOT, is
//!    pointed at `liella_internal_memcpy`, `_memmove` or `_memset`
//!    (`crates/boundary/src/conv.rs`), defined in the same member: only the
//!    relocation's symbol index changes. A load is LLVM hoisting the target of repeated calls
//!    out of a loop (`mov memcpy@GOTPCREL(%rip), %r12` ... `call *%r12`): the library's source
//!    never takes the address of these functions, so the names reach the object only through
//!    the compiler's lowering. Any other use of a name is left as it is and counted.
//! 2. In members that only reference the names (the C layer, `compiler_builtins`), the
//!    undefined symbol is renamed (`objcopy --redefine-sym`), if every use of it is of those
//!    kinds; otherwise the member is left as it is.
//! 3. Every `R_X86_64_GOTPCREL` of those kinds, in every member and whatever symbol it names,
//!    becomes `R_X86_64_GOTPCRELX` (`R_X86_64_REX_GOTPCRELX` for a `REX.W` load): the same
//!    relocation, which the linker may rewrite into a direct call, jump or `lea` once the symbol
//!    is known to be local (a static link; in `libc.so.6` the symbols the version script makes
//!    local, such as the csys helpers `liella_machine_tp` and `liella_sys_*`, PJ-19). This is the
//!    relocation C compilers emit for these instructions: the type only says which instruction
//!    holds it, and the linker keeps the GOT for a symbol a program may interpose (an exported
//!    name of `libc.so.6`), so binding is unchanged. Any other use of a GOT entry (an opcode not
//!    recognised) is left as it is and counted.
//!
//! The pass checks that no internal entry calls an internal entry or a name it replaces (a
//! copy that lowered to itself would recurse).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// The names rustc's lowering calls and the internal entries they are pointed at.
pub const TARGETS: [(&str, &str); 3] = [("memcpy", "liella_internal_memcpy"), ("memmove", "liella_internal_memmove"), ("memset", "liella_internal_memset")];

const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHF_EXECINSTR: u64 = 4;
const R_PC32: u32 = 2;
const R_GOTPCREL: u32 = 9;
const R_PLT32: u32 = 4;
const R_GOTPCRELX: u32 = 41;
const R_REX_GOTPCRELX: u32 = 42;

fn rd16(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?)))
}
fn rd32(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)))
}
fn rd64(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

struct Section {
    ty: u32,
    flags: u64,
    off: usize,
    size: usize,
    link: usize,
    info: usize,
}

struct Sym {
    name: String,
    shndx: usize,
    value: usize,
    size: usize,
}

/// A relocatable ELF64 little-endian object: sections and the symbol table.
struct Obj {
    secs: Vec<Section>,
    syms: Vec<Sym>,
}

fn cstr(b: &[u8], o: usize) -> String {
    let s = b.get(o..).unwrap_or_default();
    String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]).into_owned()
}

fn parse(b: &[u8]) -> Option<Obj> {
    if b.get(..4)? != b"\x7fELF" || b.get(4)? != &2 || b.get(5)? != &1 {
        return None;
    }
    let shoff = usize::try_from(rd64(b, 0x28)?).ok()?;
    let shentsize = usize::try_from(rd16(b, 0x3a)?).ok()?;
    let shnum = usize::try_from(rd16(b, 0x3c)?).ok()?;
    let mut secs = Vec::new();
    for i in 0..shnum {
        let h = shoff + i * shentsize;
        secs.push(Section {
            ty: u32::try_from(rd32(b, h + 4)?).ok()?,
            flags: rd64(b, h + 8)?,
            off: usize::try_from(rd64(b, h + 24)?).ok()?,
            size: usize::try_from(rd64(b, h + 32)?).ok()?,
            link: usize::try_from(rd32(b, h + 40)?).ok()?,
            info: usize::try_from(rd32(b, h + 44)?).ok()?,
        });
    }
    let mut syms = Vec::new();
    if let Some(st) = secs.iter().find(|s| s.ty == SHT_SYMTAB) {
        let strtab = secs.get(st.link)?;
        for k in 0..st.size / 24 {
            let e = st.off + k * 24;
            let name = cstr(b, strtab.off + usize::try_from(rd32(b, e)?).ok()?);
            syms.push(Sym { name, shndx: usize::try_from(rd16(b, e + 6)?).ok()?, value: usize::try_from(rd64(b, e + 8)?).ok()?, size: usize::try_from(rd64(b, e + 16)?).ok()? });
        }
    }
    Some(Obj { secs, syms })
}

/// The relaxable type for a relocation of `ty` at `at` in `code` that is on a call or jump
/// (`ff 15`, `ff 25`, `e8`, `e9`) or on a GOT load into a register (`8b` with a RIP-relative
/// operand, `REX.W` or not); `None` for any other use. Direct calls keep their type.
fn branch_kind(code: &[u8], at: usize, ty: u32) -> Option<u32> {
    let at1 = |k: usize| at.checked_sub(k).and_then(|i| code.get(i)).copied();
    match ty {
        R_GOTPCREL | R_GOTPCRELX | R_REX_GOTPCRELX => match (at1(3), at1(2), at1(1)) {
            (_, Some(0xff), Some(0x15 | 0x25)) => Some(R_GOTPCRELX),
            (Some(0x48..=0x4f), Some(0x8b), Some(m)) if m & 0xc7 == 0x05 => Some(R_REX_GOTPCRELX),
            // A REX prefix without W (a 32-bit load of a GOT entry) is not a form we make relaxable.
            (Some(0x40..=0x47), Some(0x8b), _) => None,
            (_, Some(0x8b), Some(m)) if m & 0xc7 == 0x05 => Some(R_GOTPCRELX),
            _ => None,
        },
        R_PLT32 | R_PC32 => matches!(at1(1), Some(0xe8 | 0xe9)).then_some(ty),
        _ => None,
    }
}

fn on_branch(code: &[u8], at: usize, ty: u32) -> bool {
    branch_kind(code, at, ty).is_some()
}

/// Every relocation of `o` in an executable section: (file offset of the entry, symbol index,
/// type, offset in the section, the patched section's index).
fn relocs(b: &[u8], o: &Obj) -> Vec<(usize, usize, u32, usize, usize)> {
    let mut v = Vec::new();
    for s in o.secs.iter().filter(|s| s.ty == SHT_RELA) {
        let Some(target) = o.secs.get(s.info) else { continue };
        if target.flags & SHF_EXECINSTR == 0 {
            continue;
        }
        for k in 0..s.size / 24 {
            let e = s.off + k * 24;
            let (Some(off), Some(info)) = (rd64(b, e), rd64(b, e + 8)) else { continue };
            let (Ok(sym), Ok(ty), Ok(off)) = (usize::try_from(info >> 32), u32::try_from(info & 0xffff_ffff), usize::try_from(off)) else { continue };
            v.push((e, sym, ty, off, s.info));
        }
    }
    v
}

fn set_info(b: &mut [u8], e: usize, sym: usize, ty: u32) {
    let info = ((sym as u64) << 32) | u64::from(ty);
    b[e + 8..e + 16].copy_from_slice(&info.to_le_bytes());
}

/// What the pass changed.
#[derive(Default)]
pub struct Stats {
    /// Calls, jumps and call-target loads now naming an internal entry, per replaced name.
    pub redirected: BTreeMap<&'static str, usize>,
    /// GOT calls, jumps and loads marked relaxable in this run, any symbol (step 3).
    pub relaxable: usize,
    /// Of those, the ones naming an internal entry.
    pub relaxable_internal: usize,
    /// `R_X86_64_GOTPCREL` uses on instructions not recognised, left as they are.
    pub unrecognised: usize,
    /// Other uses of a replaced name, left alone, per name.
    pub kept: BTreeMap<&'static str, usize>,
    /// References to the internal entries in code after the pass (an archive rustc did not
    /// rebuild was redirected by an earlier run: the pass is idempotent).
    pub internal: usize,
}

/// Step 1 and 3 on one member; `rename` collects step 2's undefined names. Errors for an
/// object whose internal entry is missing or calls itself.
fn edit(b: &mut [u8], member: &str, stats: &mut Stats, rename: &mut Vec<(&'static str, &'static str)>) -> Result<bool, String> {
    let Some(o) = parse(b) else { return Ok(false) };
    let view: &[u8] = b;
    let idx = |n: &str| o.syms.iter().position(|s| s.name == n);
    let code = |sec: usize| o.secs.get(sec).and_then(|s| view.get(s.off..s.off + s.size)).unwrap_or_default();
    let all = relocs(view, &o);
    let mut changes: Vec<(usize, usize, u32)> = Vec::new();
    for (name, internal) in TARGETS {
        let Some(t) = idx(name) else { continue };
        let uses: Vec<_> = all.iter().filter(|r| r.1 == t).collect();
        let branches = uses.iter().filter(|r| on_branch(code(r.4), r.3, r.2)).count();
        if o.syms[t].shndx != 0 {
            let Some(i) = idx(internal) else {
                // The direct entries in machine/fold.S (D-4.22 COPY-FILL) define the names and
                // never use them; the Rust members' references are renamed (step 2).
                if uses.is_empty() {
                    continue;
                }
                return Err(format!("{member}: defines {name} but not {internal}"));
            };
            for r in uses.iter().filter(|r| on_branch(code(r.4), r.3, r.2)) {
                changes.push((r.0, i, r.2));
            }
            *stats.redirected.entry(name).or_default() += branches;
            *stats.kept.entry(name).or_default() += uses.len() - branches;
        } else if let Some(i) = o.syms.iter().position(|s| s.name == internal && s.shndx != 0) {
            // The joined library object (the direct entries live in machine/fold.S): it only
            // references the name and defines the internal entry, so point the relocations at
            // it, as for a definer (renaming would add a second symbol of the internal's name).
            for r in uses.iter().filter(|r| on_branch(code(r.4), r.3, r.2)) {
                changes.push((r.0, i, r.2));
            }
            *stats.redirected.entry(name).or_default() += branches;
            *stats.kept.entry(name).or_default() += uses.len() - branches;
        } else if !uses.is_empty() && branches == uses.len() {
            rename.push((name, internal));
            *stats.redirected.entry(name).or_default() += branches;
        } else {
            *stats.kept.entry(name).or_default() += uses.len();
        }
    }
    // The internal entries must not call one another or the names they replace.
    for (_, internal) in TARGETS {
        let Some(s) = o.syms.iter().find(|s| s.name == internal && s.shndx != 0) else { continue };
        for r in all.iter().filter(|r| r.4 == s.shndx && r.3 >= s.value && r.3 < s.value + s.size.max(1)) {
            let callee = o.syms.get(r.1).map(|s| s.name.as_str()).unwrap_or("");
            if TARGETS.iter().any(|(n, i)| callee == *n || callee == *i) {
                return Err(format!("{member}: {internal} calls {callee}, which would recurse (D-4.11)"));
            }
        }
    }
    for &(e, sym, ty) in &changes {
        set_info(b, e, sym, ty);
    }
    Ok(!changes.is_empty())
}

/// Step 3 on one member: GOT calls, jumps and loads become relaxable, whatever symbol they name.
fn mark_relaxable(b: &mut [u8], stats: &mut Stats) -> bool {
    let Some(o) = parse(b) else { return false };
    let mut marks = Vec::new();
    for (e, sym, ty, off, sec) in relocs(b, &o) {
        let internal = o.syms.get(sym).is_some_and(|s| TARGETS.iter().any(|(_, i)| s.name == *i));
        stats.internal += usize::from(internal);
        if ty != R_GOTPCREL {
            continue;
        }
        let code = o.secs.get(sec).and_then(|s| b.get(s.off..s.off + s.size)).unwrap_or_default();
        match branch_kind(code, off, ty) {
            Some(new) => {
                marks.push((e, sym, new));
                stats.relaxable_internal += usize::from(internal);
            }
            None => stats.unrecognised += 1,
        }
    }
    for &(e, sym, new) in &marks {
        set_info(b, e, sym, new);
    }
    stats.relaxable += marks.len();
    !marks.is_empty()
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

/// Joins the library's objects into the one object the release profile makes. The release
/// profile (fat LTO, one codegen unit) puts the whole library in one member, with every symbol
/// that is not exported made local. The fast profile (TOOL-PROFILE: thin LTO, 16 units) leaves
/// each crate's units as members (`c-<h>.<crate>-<h>.<crate>.<h>-cgu.<n>...`), their private
/// symbols hidden but global. Two things then differ from the product: step 1 needs `memcpy`
/// and `liella_internal_memcpy` in the same object (a relocation can only be pointed at a symbol
/// of its own object's table), and a global `__rustc::__rust_alloc` in the static `libc.a`
/// clashes with the allocator shim of any Rust program linked against it. So all those members
/// (and the staticlib crate's own, such as its allocator shims, and `liella_mem`'s: they use `core`'s private symbols) are joined (`ld -r`) into the first, and hidden symbols are made local (`objcopy
/// --localize-hidden`, and Rust-mangled names too), which is what LTO's internalisation does. Nothing happens when there is
/// one such member (release). Returns the member list afterwards.
fn merge_codegen_units(ar: &str, archive: &Path, work: &Path, members: Vec<String>) -> Result<Vec<String>, String> {
    let g: Vec<String> = members.iter().filter(|m| m.starts_with("c-") || m.starts_with("liella_mem-")).cloned().collect();
    if g.len() < 2 {
        return Ok(members);
    }
    // Joined whatever defines `memcpy`: since the direct entries moved into machine/fold.S
    // (D-4.22 COPY-FILL) no library unit does, and keying the join on it left `core`'s and
    // `alloc`'s symbols global in the fast `libc.a` ("multiple definition" in a Rust program's
    // static link, COPY-FILL-3). `rust_symbols_stay_local` checks the result.
    let ld = std::env::var("LD").unwrap_or_else(|_| "ld".into());
    let objcopy = std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into());
    let merged = work.join("merged-codegen-units.o");
    run(Command::new(&ld).current_dir(work).arg("-r").arg("-o").arg(&merged).args(&g))?;
    // Rust-mangled names (v0 `_R<uppercase>`, legacy `_ZN`) are never exports; `core` and `alloc`
    // generics are default-visibility in thin LTO and would clash with the program's own copies.
    run(Command::new(&objcopy).arg("--localize-hidden").arg("--wildcard").arg("--localize-symbol=_R[A-Z]*").arg("--localize-symbol=_ZN*").arg(&merged))?;
    let first = g[0].clone();
    fs::rename(&merged, work.join(&first)).map_err(|e| e.to_string())?;
    run(Command::new(ar).current_dir(work).arg("d").arg(archive).args(&g))?;
    run(Command::new(ar).current_dir(work).arg("r").arg(archive).arg(&first))?;
    println!("so: {} members of the library joined into {first}, hidden symbols made local", g.len());
    Ok(members.into_iter().filter(|m| !g.contains(m) || *m == first).collect())
}

/// The pass over `archive`, in place; `work` is a scratch directory.
pub fn direct_internal_copies(ar: &str, archive: &Path, work: &Path) -> Result<Stats, String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let objcopy = std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into());
    let archive = fs::canonicalize(archive).map_err(|e| e.to_string())?;
    let listed = Command::new(ar).arg("t").arg(&archive).output().map_err(|e| format!("ar t: {e}"))?;
    let members: Vec<String> = String::from_utf8_lossy(&listed.stdout).lines().map(str::to_string).collect();
    let mut unique = members.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != members.len() {
        return Err(format!("{}: duplicate member names; cannot extract them safely", archive.display()));
    }
    run(Command::new(ar).current_dir(work).arg("x").arg(&archive))?;
    let members = merge_codegen_units(ar, &archive, work, members)?;
    let mut stats = Stats::default();
    let mut changed = Vec::new();
    for m in &members {
        let path = work.join(m);
        let mut b = fs::read(&path).map_err(|e| format!("{m}: {e}"))?;
        let mut rename = Vec::new();
        let mut dirty = edit(&mut b, m, &mut stats, &mut rename)?;
        if dirty {
            fs::write(&path, &b).map_err(|e| e.to_string())?;
        }
        if !rename.is_empty() {
            let mut cmd = Command::new(&objcopy);
            for (n, i) in &rename {
                cmd.arg(format!("--redefine-sym={n}={i}"));
            }
            run(cmd.arg(&path))?;
            b = fs::read(&path).map_err(|e| e.to_string())?;
            dirty = true;
        }
        if mark_relaxable(&mut b, &mut stats) {
            fs::write(&path, &b).map_err(|e| e.to_string())?;
            dirty = true;
        }
        if dirty {
            changed.push(m.clone());
        }
    }
    if !changed.is_empty() {
        run(Command::new(ar).current_dir(work).arg("r").arg(&archive).args(&changed))?;
    }
    let _ = fs::remove_dir_all(work);
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_are_recognised_by_opcode() {
        // call *x(%rip); mov x(%rip),%rax; call x; mov x(%rip),%r12; add x(%rip),%rax
        let code = [0xff, 0x15, 0, 0, 0, 0, 0x48, 0x8b, 0x05, 0, 0, 0, 0, 0xe8, 0, 0, 0, 0, 0x4c, 0x8b, 0x25, 0, 0, 0, 0, 0x48, 0x03, 0x05, 0, 0, 0, 0];
        assert_eq!(branch_kind(&code, 2, R_GOTPCREL), Some(R_GOTPCRELX));
        assert_eq!(branch_kind(&code, 9, R_GOTPCREL), Some(R_REX_GOTPCRELX));
        assert_eq!(branch_kind(&code, 14, R_PLT32), Some(R_PLT32));
        assert_eq!(branch_kind(&code, 21, R_GOTPCREL), Some(R_REX_GOTPCRELX));
        assert_eq!(branch_kind(&code, 28, R_GOTPCREL), None);
        assert_eq!(branch_kind(&code, 0, R_GOTPCREL), None);
        assert_eq!(branch_kind(&code, 2, 1), None); // R_X86_64_64
        // mov x(%rip),%r8d (REX without W): not made relaxable
        assert_eq!(branch_kind(&[0x44, 0x8b, 0x05, 0, 0, 0, 0], 3, R_GOTPCREL), None);
        // jmp *x(%rip)
        assert_eq!(branch_kind(&[0xff, 0x25, 0, 0, 0, 0], 2, R_GOTPCREL), Some(R_GOTPCRELX));
    }

    /// Assembles `src` with the system assembler (GNU as, on every gate machine) and runs `edit`.
    fn edit_asm(src: &str) -> (Result<bool, String>, Stats) {
        struct Gone(std::path::PathBuf);
        impl Drop for Gone {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = Gone(std::env::temp_dir().join(format!("xtask-copies-{}-{n}", std::process::id())));
        fs::create_dir_all(&dir.0).expect("scratch dir");
        let (s, o) = (dir.0.join("t.s"), dir.0.join("t.o"));
        fs::write(&s, src).expect("write t.s");
        run(Command::new("as").arg("-o").arg(&o).arg(&s)).expect("the assembler runs");
        let mut b = fs::read(&o).expect("read t.o");
        assert!(parse(&b).is_some(), "as produced an object parse() reads");
        let mut stats = Stats::default();
        let mut rename = Vec::new();
        let r = edit(&mut b, "t.o", &mut stats, &mut rename);
        (r, stats)
    }

    /// COPY-FILL relaxed `edit` for a member that defines `memcpy` without using it (the direct
    /// entries in machine/fold.S). Every member the pass rejected or redirected before still is.
    #[test]
    fn only_a_definer_that_never_uses_the_name_is_skipped() {
        // Defines memcpy and never uses it (machine/fold.S): skipped, nothing changed.
        let (r, st) = edit_asm(".text\n.globl memcpy\nmemcpy: ret\n");
        assert_eq!(r, Ok(false));
        assert!(st.redirected.is_empty() && st.kept.is_empty());
        // Defines memcpy and calls it, with no internal entry: still an error (the plant the
        // original rule caught).
        let (r, _) = edit_asm(".text\n.globl memcpy\nmemcpy: ret\n.globl f\nf: call memcpy\n");
        assert!(r.is_err_and(|e| e.contains("defines memcpy but not liella_internal_memcpy")));
        // Defines memset and jumps to it through a GOT load: still an error.
        let (r, _) = edit_asm(".text\n.globl memset\nmemset: ret\n.globl f\nf: jmp *memset@GOTPCREL(%rip)\n");
        assert!(r.is_err_and(|e| e.contains("defines memset but not liella_internal_memset")));
        // The LTO member (defines both, calls memcpy): redirected as before.
        let (r, st) = edit_asm(".text\n.globl memcpy\nmemcpy: ret\n.globl liella_internal_memcpy\nliella_internal_memcpy: ret\n.globl f\nf: call memcpy\n");
        assert_eq!(r, Ok(true));
        assert_eq!(st.redirected.get("memcpy"), Some(&1));
        // An internal entry that calls a replaced name: still the recursion error. `memmove` stays
        // undefined here: the assembler resolves a jump to a symbol defined in the same section
        // without a relocation, and the real members reach it through one.
        let (r, _) = edit_asm(".text\n.globl liella_internal_memmove\nliella_internal_memmove: jmp memmove\n.size liella_internal_memmove, .-liella_internal_memmove\n");
        assert!(r.is_err_and(|e| e.contains("would recurse")));
    }

    #[test]
    fn garbage_is_not_an_object() {
        assert!(parse(b"not an object").is_none());
        assert!(parse(&[]).is_none());
    }
}
