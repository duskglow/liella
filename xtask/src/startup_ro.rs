//! LAYOUT-STARTUP-RODATA (D-X.62, amends D-X.50): the read-only data start-up reads, placed in one
//! output section whose start is aligned, so the 64 KiB fault-around windows it occupies are a
//! function of its own size and not of where `.rodata` starts (that moved with the text size).
//!
//! Renamed to [`SECTION`] (`.rodata1`; GNU ld's default script places `*(.rodata1)` right after
//! `.rodata`) in the archive, after the hot-text grouping (which already places `STARTUP_NAMES`
//! there, D-X.48):
//! * every read-only data section (`.rodata*`) a start-up function's relocations address: the
//!   functions are the keys of `crates/libc-so/startup-text.keys` (all profiles), found by their
//!   symbol, so the compiler-numbered constants (`.rodata..Lanon.*`, `.rodata.cst16`, an assembly
//!   file's `.rodata`) follow the code that reads them;
//! * the section of every read-only key of `crates/libc-so/startup-data.keys` (objects the page-fault
//!   trace saw read, such as the ctype tables).
//!
//! The start object (`csys/src/machine/entry.S`, first in every link) holds an empty `.rodata1`
//! aligned to [`ALIGN`]: the linker aligns an output section to the largest alignment of its input
//! sections, and the start object's comes first, so the group starts on an [`ALIGN`] boundary. With
//! the group no larger than [`ALIGN`] (checked here over the whole archive, an upper bound for any
//! link) and [`ALIGN`] dividing 64 KiB, the group sits in exactly one window.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

/// The output section of the start-up read-only data (the hot-text grouping's start-up names
/// section, [`crate::hottext::STARTUP_SECTION`]).
pub const SECTION: &str = crate::hottext::STARTUP_SECTION;
/// The start alignment of [`SECTION`]: `.balign` of entry.S's `.rodata1`, one fault-around window. Smaller
/// would leave padding smaller, but the fast profile's one codegen unit puts every string literal of the
/// library in one merge section (`.rodata.str1.1`, about 19 KiB) that start-up literals live in: the
/// group's upper bound was 33 KiB on next 2e33a3cb9.
pub const ALIGN: u64 = 65536;
/// The prefix the weakening pass gives a library name (`weaken_archive`); start-up keys name the
/// final archive's symbols, this pass runs before it.
const LIB_PREFIX: &str = "__liella_libc_";

/// A section of one archive member (`readelf -SW`).
#[derive(Clone, Debug, Default, PartialEq)]
struct Sec {
    name: String,
    size: u64,
    align: u64,
    flags: String,
    /// `Inf` (a relocation section's target section index).
    info: usize,
}

/// A symbol of one archive member (`readelf -sW`): name, type, bind, section index (`None`: UND, ABS, COM).
#[derive(Clone, Debug, PartialEq)]
struct Sym {
    name: String,
    ty: String,
    bind: String,
    ndx: Option<usize>,
}

/// One archive member: sections by index, relocation sections (target index, symbol indices), symbols by index.
#[derive(Debug, Default)]
struct Member {
    secs: BTreeMap<usize, Sec>,
    relocs: Vec<(usize, Vec<usize>)>,
    syms: BTreeMap<usize, Sym>,
}

/// `readelf -SrsW <archive>` -> members by name, in archive order.
fn parse(listing: &str) -> Vec<(String, Member)> {
    let mut out: Vec<(String, Member)> = Vec::new();
    // The relocation section being read (index in its member) and whether the symbol table is.
    let mut rel: Option<usize> = None;
    let mut in_syms = false;
    for l in listing.lines() {
        if let Some(m) = l.strip_prefix("File: ").and_then(|f| f.rsplit_once('(')).and_then(|(_, m)| m.strip_suffix(')')) {
            out.push((m.to_string(), Member::default()));
            (rel, in_syms) = (None, false);
            continue;
        }
        let Some((_, m)) = out.last_mut() else { continue };
        if let Some(name) = l.strip_prefix("Relocation section '").and_then(|r| r.split_once('\'')).map(|(n, _)| n) {
            rel = m.secs.iter().find(|(_, s)| s.name == name).map(|(i, _)| *i);
            if let Some(i) = rel {
                m.relocs.push((m.secs[&i].info, Vec::new()));
            }
            in_syms = false;
            continue;
        }
        if l.starts_with("Symbol table '") {
            (rel, in_syms) = (None, true);
            continue;
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        if let Some((_, rest)) = l.trim_start().strip_prefix('[').and_then(|r| r.split_once(']')) {
            // [Nr] Name Type Address Off Size ES Flg Lk Inf Al (no Flg field when a section has no flags).
            let Ok(idx) = l.trim_start()[1..].split(']').next().unwrap_or("").trim().parse::<usize>() else { continue };
            let g: Vec<&str> = rest.split_whitespace().collect();
            let (flags, lk) = match g.len() {
                10 => (g[6], 7),
                9 => ("", 6),
                _ => continue,
            };
            let (Ok(size), Ok(info), Ok(align)) = (u64::from_str_radix(g[4], 16), g[lk + 1].parse(), g[lk + 2].parse()) else { continue };
            m.secs.insert(idx, Sec { name: g[0].to_string(), size, align, flags: flags.to_string(), info });
            continue;
        }
        if in_syms {
            // Num: Value Size Type Bind Vis Ndx Name
            // (a section symbol may show no name)
            if let [num, _, _, ty, bind, _, ndx, rest @ ..] = f.as_slice() {
                if let Some(Ok(n)) = num.strip_suffix(':').map(str::parse::<usize>) {
                    m.syms.insert(n, Sym { name: rest.first().unwrap_or(&"").to_string(), ty: ty.to_string(), bind: bind.to_string(), ndx: ndx.parse().ok() });
                }
            }
            continue;
        }
        if rel.is_some() {
            // Offset Info Type Value Name + Addend: the symbol index is the high half of Info.
            if let [_, info, ..] = f.as_slice() {
                if let (16, Ok(i)) = (info.len(), u64::from_str_radix(info, 16)) {
                    if let Some((_, list)) = m.relocs.last_mut() {
                        list.push((i >> 32) as usize);
                    }
                }
            }
        }
    }
    out
}

/// Whether `s` is a read-only data section this pass moves (or already moved).
fn read_only_data(s: &Sec) -> bool {
    s.flags.contains('A') && !s.flags.contains('W') && !s.flags.contains('X') && !s.flags.contains('T') && (s.name.starts_with(".rodata") || s.name == SECTION)
}

/// The keys of a `startup-*.keys` file (`<key> TAB <tier or ro|rw> TAB <profiles>`) whose second field is
/// accepted by `want`, any profile.
fn keys(text: &str, want: impl Fn(&str) -> bool) -> BTreeSet<String> {
    text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        (f.len() == 3 && want(f[1])).then(|| f[0].to_string())
    }).collect()
}

/// Whether symbol `name` is one of `keys`, by its own key or the name the weakening pass gives it.
fn keyed(name: &str, keys: &BTreeSet<String>) -> bool {
    keys.contains(&crate::hottext::key(name)) || keys.contains(&crate::hottext::key(&format!("{LIB_PREFIX}{name}")))
}

/// The plan: per member, the sections (by name) to rename to [`SECTION`]; and the bytes [`SECTION`]
/// holds over the archive (sizes plus alignment slack, the renamed ones and those already there).
fn plan(members: &[(String, Member)], text_keys: &BTreeSet<String>, ro_keys: &BTreeSet<String>) -> (BTreeMap<String, BTreeSet<String>>, u64, Vec<(u64, String)>) {
    // Defined global symbols -> (member, section index), for a relocation against another member's object.
    let mut global: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (mi, (_, m)) in members.iter().enumerate() {
        for s in m.syms.values() {
            if let (Some(ndx), "GLOBAL" | "WEAK") = (s.ndx, s.bind.as_str()) {
                global.entry(&s.name).or_insert((mi, ndx));
            }
        }
    }
    let mut chosen: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (mi, (_, m)) in members.iter().enumerate() {
        let starts: BTreeSet<usize> = m.syms.values().filter(|s| matches!(s.ty.as_str(), "FUNC" | "NOTYPE") && keyed(&s.name, text_keys)).filter_map(|s| s.ndx).collect();
        for (target, syms) in &m.relocs {
            if !starts.contains(target) {
                continue;
            }
            for si in syms {
                let Some(s) = m.syms.get(si) else { continue };
                let at = match s.ndx {
                    Some(ndx) => Some((mi, ndx)),
                    None => global.get(s.name.as_str()).copied(),
                };
                chosen.extend(at);
            }
        }
        for s in m.syms.values() {
            if let (Some(ndx), "OBJECT" | "NOTYPE") = (s.ndx, s.ty.as_str()) {
                if keyed(&s.name, ro_keys) {
                    chosen.insert((mi, ndx));
                }
            }
        }
    }
    let mut rename: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut bytes = 0;
    let mut sizes = Vec::new();
    for (mi, (name, m)) in members.iter().enumerate() {
        for s in m.secs.values() {
            if !read_only_data(s) {
                continue;
            }
            // objcopy renames by name: every section of that name in the member moves, so all are counted.
            let moved = s.name == SECTION || m.secs.iter().any(|(j, t)| t.name == s.name && chosen.contains(&(mi, *j)));
            if !moved {
                continue;
            }
            bytes += s.size + s.align.max(1) - 1;
            sizes.push((s.size, format!("{name}:{}", s.name)));
            if s.name != SECTION {
                rename.entry(name.clone()).or_default().insert(s.name.clone());
            }
        }
    }
    sizes.sort_by(|a, b| b.cmp(a));
    (rename, bytes, sizes)
}

/// Rename the start-up read-only data sections of `archive` to [`SECTION`] in place (`root`: the tree with
/// the key files; `work`: scratch). Fails when they could exceed [`ALIGN`] bytes. Returns (sections renamed, bytes).
pub fn place(ar: &str, archive: &Path, root: &Path, work: &Path) -> Result<(usize, u64), String> {
    let read = |p: &str| fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let text_keys = keys(&read("crates/libc-so/startup-text.keys")?, |_| true);
    let ro_keys = keys(&read("crates/libc-so/startup-data.keys")?, |k| k == "ro");
    let listing = Command::new("readelf").arg("-SrsW").arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    if !listing.status.success() {
        return Err(format!("readelf -SrsW {} failed", archive.display()));
    }
    let members = parse(&String::from_utf8_lossy(&listing.stdout));
    if members.iter().map(|(n, _)| n).collect::<BTreeSet<_>>().len() != members.len() {
        return Err(format!("{}: duplicate member names; cannot extract them safely", archive.display()));
    }
    let (rename, bytes, sizes) = plan(&members, &text_keys, &ro_keys);
    if bytes > ALIGN {
        return Err(format!("start-up read-only data: up to {bytes} bytes in {SECTION} > {ALIGN} (its alignment, xtask/src/startup_ro.rs and entry.S): it would span two 64 KiB windows in some links; largest: {}", sizes.iter().take(12).map(|(n, s)| format!("{s} {n}")).collect::<Vec<_>>().join(", ")));
    }
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let names: Vec<&String> = rename.keys().collect();
    if !names.is_empty() {
        crate::so::run(Command::new(ar).current_dir(work).arg("x").arg(archive).args(&names))?;
    }
    let objcopy = std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into());
    let mut n = 0;
    for (m, secs) in &rename {
        let mut cmd = Command::new(&objcopy);
        for s in secs {
            cmd.arg("--rename-section").arg(format!("{s}={SECTION}"));
            n += 1;
        }
        crate::so::run(cmd.arg(work.join(m)))?;
    }
    if !names.is_empty() {
        crate::so::run(Command::new(ar).current_dir(work).arg("r").arg(archive).args(&names))?;
    }
    let _ = fs::remove_dir_all(work);
    Ok((n, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "
File: libc.a(a.o)

Section Headers:
  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al
  [ 0]                   NULL            0000000000000000 000000 000000 00      0   0  0
  [ 3] .text.start       PROGBITS        0000000000000000 000040 000020 00  AX  0   0 16
  [ 4] .rela.text.start  RELA            0000000000000000 000300 000048 18   I 12   3  8
  [ 5] .text.other       PROGBITS        0000000000000000 000060 000020 00  AX  0   0 16
  [ 6] .rela.text.other  RELA            0000000000000000 000348 000018 18   I 12   5  8
  [ 7] .rodata..Lanon.1  PROGBITS        0000000000000000 000080 000010 00   A  0   0  8
  [ 8] .rodata..Lanon.2  PROGBITS        0000000000000000 000090 000010 00   A  0   0  8
  [ 9] .rodata.cst16     PROGBITS        0000000000000000 0000a0 000020 10  AM  0   0 16
  [10] .data.state       PROGBITS        0000000000000000 0000c0 000008 00  WA  0   0  8
  [11] .rodata.table     PROGBITS        0000000000000000 0000d0 000100 00   A  0   0  4

Relocation section '.rela.text.start' at offset 0x300 contains 3 entries:
    Offset             Info             Type               Symbol's Value  Symbol's Name + Addend
0000000000000003  0000000200000002 R_X86_64_PC32          0000000000000000 .rodata..Lanon.1 - 4
000000000000000a  0000000300000002 R_X86_64_PC32          0000000000000000 .rodata.cst16 - 4
0000000000000011  0000000500000002 R_X86_64_PC32          0000000000000000 .data.state - 4
0000000000000018  0000000600000004 R_X86_64_PLT32         0000000000000000 TABLE - 4

Relocation section '.rela.text.other' at offset 0x348 contains 1 entry:
    Offset             Info             Type               Symbol's Value  Symbol's Name + Addend
0000000000000003  0000000400000002 R_X86_64_PC32          0000000000000000 .rodata..Lanon.2 - 4

Symbol table '.symtab' contains 8 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
     2: 0000000000000000     0 SECTION LOCAL  DEFAULT    7 
     3: 0000000000000000     0 SECTION LOCAL  DEFAULT    9 .rodata.cst16
     4: 0000000000000000     0 SECTION LOCAL  DEFAULT    8 .rodata..Lanon.2
     5: 0000000000000000     0 SECTION LOCAL  DEFAULT   10 .data.state
     6: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  UND TABLE
     7: 0000000000000000    32 FUNC    GLOBAL DEFAULT    3 start_fn
     8: 0000000000000000    32 FUNC    GLOBAL DEFAULT    5 other_fn

File: libc.a(b.o)

Section Headers:
  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al
  [ 4] .rodata.TABLE     PROGBITS        0000000000000000 000040 000040 00   A  0   0  8
  [ 5] .rodata.CTYPE     PROGBITS        0000000000000000 000080 000300 00   A  0   0  2
  [ 6] .rodata.OTHER     PROGBITS        0000000000000000 000380 000300 00   A  0   0  2

Symbol table '.symtab' contains 3 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     1: 0000000000000000    64 OBJECT  GLOBAL HIDDEN     4 TABLE
     2: 0000000000000000   768 OBJECT  GLOBAL HIDDEN     5 CTYPE
     3: 0000000000000000   768 OBJECT  GLOBAL HIDDEN     6 OTHER
";

    #[test]
    fn startup_read_only_data_sections_follow_the_startup_functions_and_ro_keys() {
        // LAYOUT-STARTUP-RODATA (no spec clause; layout of the products): the read-only sections a start-up
        // function's relocations address, in its own member or (by a global symbol) another, and the sections of
        // the read-only keys move; a writable target, another function's constants and an unkeyed object stay.
        let members = parse(LISTING);
        assert_eq!(members.len(), 2);
        let text: BTreeSet<String> = ["start_fn".to_string()].into();
        let ro: BTreeSet<String> = ["CTYPE".to_string()].into();
        let (rename, bytes, _) = plan(&members, &text, &ro);
        let a: Vec<&str> = rename["a.o"].iter().map(String::as_str).collect();
        assert_eq!(a, [".rodata..Lanon.1", ".rodata.cst16"]);
        let b: Vec<&str> = rename["b.o"].iter().map(String::as_str).collect();
        assert_eq!(b, [".rodata.CTYPE", ".rodata.TABLE"]);
        assert_eq!(bytes, (0x10 + 7) + (0x20 + 15) + (0x40 + 7) + (0x300 + 1));
    }

    #[test]
    fn a_library_name_matches_its_weakened_key() {
        // LAYOUT-STARTUP-RODATA (no spec clause): start-up keys name the final archive's symbols
        // (`__liella_libc_<name>`); this pass runs before the weakening, on `<name>`.
        let text: BTreeSet<String> = [crate::hottext::key(&format!("{LIB_PREFIX}start_fn"))].into();
        let (rename, ..) = plan(&parse(LISTING), &text, &BTreeSet::new());
        assert!(rename["a.o"].contains(".rodata.cst16"));
    }
}
