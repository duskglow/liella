//! `cargo xtask repro [--release] [--dir <base>]`: the products must not depend on the directory
//! they were built in (BUILD-PATH-HASH, BUILD-PATH-RELEASE, ruling #713).
//!
//! The current tree's sources (everything but `target/`, `.git/` and `fuzz/target/`) are copied
//! into two directories of different names and depths under `<base>` (default
//! `<root>/target/repro`, on the role's own disk), the products are built in each with the `fast`
//! profile (`--release`: the shipped `release` profile), and every file of `target/liella` is
//! compared:
//! - byte-identical files are `SAME`;
//! - ELF files (and the ELF members of `ar` archives) that differ only in `.symtab`/`.strtab`
//!   are `SAME-LOADED`: local symbol names may change while the loaded image does not, as
//!   BUILD-PATH-HASH ruled acceptable;
//! - anything else fails, naming the first differing file, archive member and section.
//!
//! Not part of the per-story gate: it builds the products twice, from scratch. It runs as a
//! nightly job on an idle build host. Do not point `--dir` at `/dev/shm` when running under
//! `~/bin/gate` (#714: the gate gives the job a private `/dev/shm`); the role's disk or
//! `/mnt/zram` work. The two copies are removed and rebuilt from scratch on every run; a
//! directory without this tool's marker file is never removed.

use std::fs;
use std::path::{Path, PathBuf};

use crate::so::Profile;

/// The file written into each copy; only a directory holding it is ever removed by this tool.
const MARKER: &str = ".liella-repro-copy";

/// Two copy names of different lengths and depths, so length- and depth-dependent output shows too.
const COPIES: [&str; 2] = ["a", "second/tree-b"];

/// Sections whose differences do not change the loaded image (local symbol names, ruled acceptable).
const SYMBOL_ONLY: [&str; 2] = [".symtab", ".strtab"];

/// The result of comparing two products.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Byte-identical.
    Same,
    /// Different bytes, but only in the named symbol-table sections.
    SameLoaded(Vec<String>),
    /// The first difference that changes what is loaded (or that could not be classified).
    Differs(String),
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut profile = Profile::Fast;
    let mut base = root.join("target/repro");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--release" => profile = Profile::Release,
            "--dir" => base = PathBuf::from(it.next().ok_or("--dir needs a directory")?),
            other => return Err(format!("unknown argument {other} (usage: cargo xtask repro [--release] [--dir <base>])")),
        }
    }
    let base = if base.is_absolute() { base } else { root.join(base) };
    let mut built = Vec::new();
    for name in COPIES {
        let copy = base.join(name);
        fresh_copy(root, &copy)?;
        println!("repro: building the {} products in {}", profile.name(), copy.display());
        crate::so::run_so_as(&copy, profile).map_err(|e| format!("product build in {} failed: {e}", copy.display()))?;
        built.push(copy.join("target/liella"));
    }
    let (lines, first) = compare_dirs(&built[0], &built[1])?;
    for l in &lines {
        println!("repro: {l}");
    }
    match first {
        None => {
            println!("repro: {} products reproducible across {} and {}", profile.name(), base.join(COPIES[0]).display(), base.join(COPIES[1]).display());
            Ok(())
        }
        Some(d) => Err(format!("{} products depend on the build directory: first difference {d}", profile.name())),
    }
}

/// Replace `copy` with a fresh copy of the sources of `root`. An existing `copy` is removed only
/// when it carries this tool's marker.
pub fn fresh_copy(root: &Path, copy: &Path) -> Result<(), String> {
    if copy.exists() {
        if !copy.join(MARKER).is_file() {
            return Err(format!("{} exists and is not a repro copy (no {MARKER}); not removing it", copy.display()));
        }
        fs::remove_dir_all(copy).map_err(|e| format!("remove {}: {e}", copy.display()))?;
    }
    fs::create_dir_all(copy).map_err(|e| format!("create {}: {e}", copy.display()))?;
    fs::write(copy.join(MARKER), "a source copy made by cargo xtask repro; removed by its next run\n").map_err(|e| e.to_string())?;
    let canon_copy = copy.canonicalize().map_err(|e| e.to_string())?;
    copy_tree(root, root, copy, &canon_copy)
}

/// Whether `rel` (a path relative to the tree root) is left out of the copy.
fn skipped(rel: &Path) -> bool {
    rel == Path::new("target") || rel == Path::new(".git") || rel == Path::new("fuzz/target")
}

fn copy_tree(root: &Path, src: &Path, dst: &Path, canon_copy: &Path) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))?.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
        if skipped(rel) || path.canonicalize().is_ok_and(|c| canon_copy.starts_with(&c) || c.starts_with(canon_copy)) {
            continue;
        }
        let to = dst.join(e.file_name());
        let ty = e.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            let target = fs::read_link(&path).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink(&target, &to).map_err(|e| format!("symlink {}: {e}", to.display()))?;
        } else if ty.is_dir() {
            fs::create_dir(&to).map_err(|e| format!("create {}: {e}", to.display()))?;
            copy_tree(root, &path, &to, canon_copy)?;
        } else if ty.is_file() {
            fs::copy(&path, &to).map_err(|e| format!("copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Compare every regular file of two product directories (sorted by name). Returns one line per
/// file and the first difference that fails the check.
pub fn compare_dirs(a: &Path, b: &Path) -> Result<(Vec<String>, Option<String>), String> {
    let names = |d: &Path| -> Result<Vec<String>, String> {
        let mut v: Vec<String> = fs::read_dir(d)
            .map_err(|e| format!("read {}: {e}", d.display()))?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        Ok(v)
    };
    let (na, nb) = (names(a)?, names(b)?);
    let mut lines = Vec::new();
    let mut first = None;
    for n in na.iter().filter(|n| !nb.contains(n)).chain(nb.iter().filter(|n| !na.contains(n))) {
        lines.push(format!("{n} DIFFERS: built in one directory only"));
        first.get_or_insert(format!("{n}: built in one directory only"));
    }
    for n in na.iter().filter(|n| nb.contains(n)) {
        let da = fs::read(a.join(n)).map_err(|e| format!("read {n}: {e}"))?;
        let db = fs::read(b.join(n)).map_err(|e| format!("read {n}: {e}"))?;
        match compare(&da, &db) {
            Outcome::Same => lines.push(format!("{n} SAME")),
            Outcome::SameLoaded(s) => lines.push(format!("{n} SAME-LOADED (differs only in {})", s.join(", "))),
            Outcome::Differs(d) => {
                lines.push(format!("{n} DIFFERS: {d}"));
                first.get_or_insert(format!("{n}: {d}"));
            }
        }
    }
    Ok((lines, first))
}

/// Compare two products: whole files, then (ELF files, `ar` archives of ELF members) section by section.
pub fn compare(a: &[u8], b: &[u8]) -> Outcome {
    if a == b {
        return Outcome::Same;
    }
    if a.starts_with(b"!<arch>\n") && b.starts_with(b"!<arch>\n") {
        return compare_ar(a, b);
    }
    if a.starts_with(b"\x7fELF") && b.starts_with(b"\x7fELF") {
        return compare_elf(a, b);
    }
    Outcome::Differs("contents (not ELF or ar)".into())
}

/// One `ar` member: its resolved name, the header fields other than name and size, and its data.
struct Member<'a> {
    name: String,
    meta: &'a [u8],
    data: &'a [u8],
}

fn ar_members(d: &[u8]) -> Option<Vec<Member<'_>>> {
    let mut out = Vec::new();
    let mut long: &[u8] = &[];
    let mut off = 8;
    while off < d.len() {
        let h = d.get(off..off + 60)?;
        if &h[58..60] != b"`\n" {
            return None;
        }
        let size: usize = std::str::from_utf8(&h[48..58]).ok()?.trim().parse().ok()?;
        let data = d.get(off + 60..off + 60 + size)?;
        let raw = std::str::from_utf8(&h[0..16]).ok()?.trim_end();
        let name = if raw == "/" || raw == "//" {
            raw.to_string()
        } else if let Some(n) = raw.strip_prefix('/') {
            let start: usize = n.parse().ok()?;
            let rest = long.get(start..)?;
            let end = rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
            String::from_utf8_lossy(&rest[..end]).trim_end_matches('/').to_string()
        } else {
            raw.trim_end_matches('/').to_string()
        };
        if raw == "//" {
            long = data;
        }
        out.push(Member { name, meta: &h[16..48], data });
        off += 60 + size + (size & 1);
    }
    Some(out)
}

fn compare_ar(a: &[u8], b: &[u8]) -> Outcome {
    let (Some(ma), Some(mb)) = (ar_members(a), ar_members(b)) else {
        return Outcome::Differs("archive (cannot parse)".into());
    };
    if ma.len() != mb.len() {
        return Outcome::Differs(format!("archive member count {} vs {}", ma.len(), mb.len()));
    }
    // Names first: a path-dependent hash in a member name also changes the long-name table ("//").
    if let Some((i, (x, y))) = ma.iter().zip(&mb).enumerate().find(|(_, (x, y))| x.name != y.name) {
        return Outcome::Differs(format!("archive member {i}: name {} vs {}", x.name, y.name));
    }
    let mut allowed = Vec::new();
    for (x, y) in ma.iter().zip(&mb) {
        if x.meta != y.meta {
            return Outcome::Differs(format!("member {} header (date, owner or mode)", x.name));
        }
        if x.data == y.data {
            continue;
        }
        if x.name == "/" || x.name == "//" || !x.data.starts_with(b"\x7fELF") {
            let what = if x.name == "/" { "the archive symbol index".to_string() } else { format!("member {}", x.name) };
            return Outcome::Differs(what);
        }
        match compare_elf(x.data, y.data) {
            Outcome::Same => {}
            Outcome::SameLoaded(s) => allowed.extend(s.into_iter().map(|s| format!("{}:{s}", x.name))),
            Outcome::Differs(d) => return Outcome::Differs(format!("member {} {d}", x.name)),
        }
    }
    if allowed.is_empty() { Outcome::Differs("archive bytes outside its members".into()) } else { Outcome::SameLoaded(allowed) }
}

/// An ELF64 little-endian section: name, header fields compared (all but name index and file
/// offset), contents (empty for SHT_NOBITS).
struct Section<'a> {
    name: String,
    fields: Vec<u64>,
    data: &'a [u8],
}

fn u(d: &[u8], off: usize, n: usize) -> Option<u64> {
    let b = d.get(off..off + n)?;
    Some(b.iter().rev().fold(0u64, |acc, &x| (acc << 8) | u64::from(x)))
}

fn elf_sections(d: &[u8]) -> Option<Vec<Section<'_>>> {
    // ELFCLASS64, little-endian: the only kind the products are.
    if d.get(4) != Some(&2) || d.get(5) != Some(&1) {
        return None;
    }
    let shoff = usize::try_from(u(d, 0x28, 8)?).ok()?;
    let shentsize = usize::try_from(u(d, 0x3a, 2)?).ok()?;
    let shnum = usize::try_from(u(d, 0x3c, 2)?).ok()?;
    let shstrndx = usize::try_from(u(d, 0x3e, 2)?).ok()?;
    if shentsize < 64 {
        return None;
    }
    let mut raw = Vec::new();
    for i in 0..shnum {
        let h = shoff.checked_add(i.checked_mul(shentsize)?)?;
        let ty = u(d, h + 4, 4)?;
        let off = usize::try_from(u(d, h + 0x18, 8)?).ok()?;
        let size = usize::try_from(u(d, h + 0x20, 8)?).ok()?;
        let data = if ty == 8 { &d[0..0] } else { d.get(off..off.checked_add(size)?)? };
        let fields = vec![ty, u(d, h + 8, 8)?, u(d, h + 0x10, 8)?, size as u64, u(d, h + 0x28, 4)?, u(d, h + 0x2c, 4)?, u(d, h + 0x30, 8)?, u(d, h + 0x38, 8)?];
        raw.push((u(d, h, 4)?, fields, data));
    }
    let strtab = raw.get(shstrndx).map(|r| r.2).unwrap_or(&[]);
    Some(
        raw.into_iter()
            .map(|(n, fields, data)| {
                let rest = usize::try_from(n).ok().and_then(|n| strtab.get(n..)).unwrap_or(&[]);
                let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
                Section { name: String::from_utf8_lossy(&rest[..end]).into_owned(), fields, data }
            })
            .collect(),
    )
}

fn compare_elf(a: &[u8], b: &[u8]) -> Outcome {
    let (Some(sa), Some(sb)) = (elf_sections(a), elf_sections(b)) else {
        return Outcome::Differs("ELF (cannot parse the section headers)".into());
    };
    if sa.len() != sb.len() {
        return Outcome::Differs(format!("section count {} vs {}", sa.len(), sb.len()));
    }
    // The ELF header, e_shoff (moved by a longer .strtab) aside.
    if a[..0x28] != b[..0x28] || a.get(0x30..0x40) != b.get(0x30..0x40) {
        return Outcome::Differs("ELF header".into());
    }
    // The program headers (shared objects): the loaded segments' layout.
    let phdrs = |d: &[u8]| -> Option<Vec<u8>> {
        let off = usize::try_from(u(d, 0x20, 8)?).ok()?;
        let len = usize::try_from(u(d, 0x36, 2)? * u(d, 0x38, 2)?).ok()?;
        d.get(off..off.checked_add(len)?).map(<[u8]>::to_vec)
    };
    if phdrs(a) != phdrs(b) {
        return Outcome::Differs("program headers".into());
    }
    let mut allowed = Vec::new();
    for (i, (x, y)) in sa.iter().zip(&sb).enumerate() {
        if x.name != y.name {
            return Outcome::Differs(format!("section [{i}] name {} vs {}", x.name, y.name));
        }
        if x.fields == y.fields && x.data == y.data {
            continue;
        }
        if SYMBOL_ONLY.contains(&x.name.as_str()) {
            if !allowed.contains(&x.name) {
                allowed.push(x.name.clone());
            }
            continue;
        }
        let what = if x.fields != y.fields { "header" } else { "contents" };
        return Outcome::Differs(format!("section {} ({what})", x.name));
    }
    if allowed.is_empty() { Outcome::Differs("bytes outside every section".into()) } else { Outcome::SameLoaded(allowed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ELF64 LE relocatable object: the given sections (name, type, flags, contents),
    /// then `.shstrtab`, then the section header table.
    fn elf(sections: &[(&str, u32, u64, &[u8])]) -> Vec<u8> {
        let mut shstr = vec![0u8];
        let mut names = Vec::new();
        for (n, ..) in sections.iter().chain([(".shstrtab", 3, 0, &[][..])].iter()) {
            names.push(shstr.len() as u32);
            shstr.extend_from_slice(n.as_bytes());
            shstr.push(0);
        }
        let mut d = vec![0u8; 64];
        d[..6].copy_from_slice(b"\x7fELF\x02\x01");
        d[16] = 1; // ET_REL
        let mut offs = Vec::new();
        for (.., data) in sections {
            offs.push(d.len());
            d.extend_from_slice(data);
        }
        offs.push(d.len());
        d.extend_from_slice(&shstr);
        while d.len() % 8 != 0 {
            d.push(0);
        }
        let shoff = d.len();
        d.extend_from_slice(&[0u8; 64]); // the null section
        let all: Vec<(u32, u64, &[u8])> = sections.iter().map(|&(_, t, f, data)| (t, f, data)).chain([(3, 0, &shstr[..])]).collect();
        for (i, (t, f, data)) in all.iter().enumerate() {
            let mut h = [0u8; 64];
            h[0..4].copy_from_slice(&names[i].to_le_bytes());
            h[4..8].copy_from_slice(&t.to_le_bytes());
            h[8..16].copy_from_slice(&f.to_le_bytes());
            h[0x18..0x20].copy_from_slice(&(offs[i] as u64).to_le_bytes());
            h[0x20..0x28].copy_from_slice(&(data.len() as u64).to_le_bytes());
            d.extend_from_slice(&h);
        }
        d[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        d[0x34..0x36].copy_from_slice(&64u16.to_le_bytes());
        d[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        d[0x3c..0x3e].copy_from_slice(&((all.len() + 1) as u16).to_le_bytes());
        d[0x3e..0x40].copy_from_slice(&(all.len() as u16).to_le_bytes());
        d
    }

    fn obj(text: &[u8], rodata: &[u8], strtab: &[u8]) -> Vec<u8> {
        elf(&[(".text", 1, 6, text), (".rodata", 1, 2, rodata), (".symtab", 2, 0, &[0u8; 24]), (".strtab", 3, 0, strtab)])
    }

    /// A GNU `ar` archive with a long-name table when a name does not fit in 15 bytes.
    fn ar(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut long = Vec::new();
        let mut hdr_names = Vec::new();
        for (n, _) in members {
            if n.len() > 15 {
                hdr_names.push(format!("/{}", long.len()));
                long.extend_from_slice(format!("{n}/\n").as_bytes());
            } else {
                hdr_names.push(format!("{n}/"));
            }
        }
        let mut d = b"!<arch>\n".to_vec();
        let mut push = |name: &str, data: &[u8]| {
            d.extend_from_slice(format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, 644, data.len()).as_bytes());
            d.extend_from_slice(data);
            if data.len() % 2 == 1 {
                d.push(b'\n');
            }
        };
        if !long.is_empty() {
            push("//", &long);
        }
        for ((_, data), n) in members.iter().zip(&hdr_names) {
            push(n, data);
        }
        d
    }

    #[test]
    fn identical_files_are_same() {
        let o = obj(b"\x90\xc3", b"abc", b"\0x\0");
        assert_eq!(compare(&o, &o.clone()), Outcome::Same);
    }

    #[test]
    fn symbol_names_only_are_same_loaded() {
        let a = obj(b"\x90\xc3", b"abc", b"\0local.1\0");
        let b = obj(b"\x90\xc3", b"abc", b"\0local.22222\0");
        assert_eq!(compare(&a, &b), Outcome::SameLoaded(vec![".strtab".into()]));
    }

    #[test]
    fn a_path_in_rodata_differs() {
        let a = obj(b"\x90\xc3", b"/home/claude/w/a/crates/core", b"\0x\0");
        let b = obj(b"\x90\xc3", b"/home/claude/w/second/tree-b/crates/core", b"\0x\0");
        assert_eq!(compare(&a, &b), Outcome::Differs("section .rodata (header)".into()));
        let c = obj(b"\x90\xc3", b"/home/claude/w/b/crates/core", b"\0x\0");
        assert_eq!(compare(&a, &c), Outcome::Differs("section .rodata (contents)".into()));
    }

    #[test]
    fn first_loaded_difference_is_named() {
        let a = obj(b"\x90\xc3", b"abc", b"\0x\0");
        let b = obj(b"\xc3\xc3", b"abd", b"\0yy\0");
        assert_eq!(compare(&a, &b), Outcome::Differs("section .text (contents)".into()));
        let extra = elf(&[(".text", 1, 6, b"\x90\xc3"), (".rodata", 1, 2, b"abc"), (".data", 1, 3, b"z"), (".symtab", 2, 0, &[0u8; 24]), (".strtab", 3, 0, b"\0x\0")]);
        assert_eq!(compare(&a, &extra), Outcome::Differs("section count 6 vs 7".into()));
        assert_eq!(compare(b"plain text", b"plain texT"), Outcome::Differs("contents (not ELF or ar)".into()));
    }

    #[test]
    fn archive_members_are_compared_by_section() {
        let long = "liella_core-0123456789abcdef.liella_core.1-cgu.0.rcgu.o";
        let same = obj(b"\x90", b"r", b"\0x\0");
        let a = ar(&[("one.o", &same), (long, &obj(b"\xc3", b"k", b"\0s.1\0"))]);
        assert_eq!(compare(&a, &a.clone()), Outcome::Same);
        let b = ar(&[("one.o", &same), (long, &obj(b"\xc3", b"k", b"\0s.222\0"))]);
        assert_eq!(compare(&a, &b), Outcome::SameLoaded(vec![format!("{long}:.strtab")]));
        let c = ar(&[("one.o", &same), (long, &obj(b"\xc3", b"q", b"\0s.1\0"))]);
        assert_eq!(compare(&a, &c), Outcome::Differs(format!("member {long} section .rodata (contents)")));
        let renamed = ar(&[("one.o", &same), ("liella_core-fedcba9876543210.liella_core.1-cgu.0.rcgu.o", &obj(b"\xc3", b"k", b"\0s.1\0"))]);
        assert!(matches!(compare(&a, &renamed), Outcome::Differs(d) if d.contains("name")), "a renamed member (path-dependent hash) is caught");
        let fewer = ar(&[("one.o", &same)]);
        assert!(matches!(compare(&a, &fewer), Outcome::Differs(d) if d.contains("count")));
    }

    #[test]
    fn product_dirs_name_the_first_differing_file() {
        let base = std::env::temp_dir().join(format!("liella-repro-test-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        for d in [&a, &b] {
            fs::write(d.join("libc.a"), ar(&[("x.o", &obj(b"\x90", b"r", b"\0x\0"))])).unwrap();
            fs::write(d.join(".profile"), "fast\n").unwrap();
        }
        fs::write(a.join("libc.so.6"), obj(b"\x90", b"/w/a", b"\0x\0")).unwrap();
        fs::write(b.join("libc.so.6"), obj(b"\x90", b"/w/b", b"\0x\0")).unwrap();
        let (lines, first) = compare_dirs(&a, &b).unwrap();
        assert_eq!(first.as_deref(), Some("libc.so.6: section .rodata (contents)"));
        assert!(lines.iter().any(|l| l == "libc.a SAME"), "{lines:?}");
        fs::write(b.join("libc.so.6"), obj(b"\x90", b"/w/a", b"\0x\0")).unwrap();
        fs::write(b.join("liella-crt1.o"), b"x").unwrap();
        let (_, first) = compare_dirs(&a, &b).unwrap();
        assert_eq!(first.as_deref(), Some("liella-crt1.o: built in one directory only"));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn copies_skip_build_output_and_keep_foreign_directories() {
        let base = std::env::temp_dir().join(format!("liella-repro-copy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("tree");
        for d in ["crates/core/src", "target/liella", ".git", "fuzz/target", "fuzz/src"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::write(root.join("Cargo.toml"), "x").unwrap();
        fs::write(root.join("crates/core/src/lib.rs"), "y").unwrap();
        fs::write(root.join("fuzz/src/f.rs"), "z").unwrap();
        std::os::unix::fs::symlink("Cargo.toml", root.join("link")).unwrap();
        // A copy inside the tree (the default, under target/) and one beside it.
        for copy in [root.join("target/repro/a"), base.join("elsewhere/b")] {
            fresh_copy(&root, &copy).unwrap();
            assert!(copy.join("crates/core/src/lib.rs").is_file() && copy.join("fuzz/src/f.rs").is_file());
            assert_eq!(fs::read_link(copy.join("link")).unwrap(), Path::new("Cargo.toml"));
            assert!(!copy.join("target").exists() && !copy.join(".git").exists() && !copy.join("fuzz/target").exists());
            fs::write(copy.join("stale"), "s").unwrap();
            fresh_copy(&root, &copy).unwrap();
            assert!(!copy.join("stale").exists(), "a rerun starts from a fresh copy");
        }
        let foreign = base.join("foreign");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("keep"), "k").unwrap();
        assert!(fresh_copy(&root, &foreign).is_err());
        assert!(foreign.join("keep").is_file(), "a directory without the marker is never removed");
        fs::remove_dir_all(&base).unwrap();
    }
}
