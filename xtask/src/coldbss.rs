//! Cold zero-initialised data out of the startup pages (PAGEFAULTS-4, D-X.22). A static
//! program's `.bss` holds the library's small state words (heap, locale, stdio, thread start-up)
//! between large zero-initialised tables (thread keys, resolver queues, locale interning, heap
//! reserve) that most processes never or barely touch; every first write to a 4 KiB page is a
//! page fault, so the small words that start-up writes landed on ~20 different pages. The
//! sections of the large tables (`.bss.<symbol>`, at least [`MIN`] bytes) are renamed
//! `.lbss.<symbol>` in the archive: GNU ld's (and gold's) default x86-64 script places
//! `.lbss` after `.bss` in the same writable segment (an orphan section in other linkers lands
//! with the other writable zero-filled sections), so the small words pack into a few pages.
//! Only placement changes: names, symbols, sizes, alignment and the zero initial contents stay,
//! and each table keeps its own section, so `--gc-sections` still drops unused ones.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::hottext::{archive_headers, Shdr, SHT_NOBITS};
use crate::run;

/// Sections from this size on are cold tables (measured on coreutils `printf` in en_US.UTF-8:
/// 4096 saved 5 of 119 faults per process, 1024 saved 9, 256 saved 7: below 1 KiB the moved
/// sections are the small state words themselves).
pub const MIN: u64 = 1024;

/// The `.bss.*` sections of at least [`MIN`] bytes per archive member, from the members' own
/// section headers ([`crate::hottext::archive_headers`]: whole names; `readelf -SW` cuts them
/// at 256 bytes, and a cut name renames nothing).
fn cold_sections(members: &[(String, Vec<Shdr>)]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (m, headers) in members {
        for s in headers {
            if s.kind == SHT_NOBITS && s.name.starts_with(".bss.") && s.size >= MIN {
                out.entry(m.clone()).or_default().push(s.name.clone());
            }
        }
    }
    out
}

/// Rename the sections `renames` names (member -> (from, to)) in `archive`, in place, through
/// `work` (scratch), and check every rename took effect. Returns the number renamed. With `order`
/// (member -> section names), each listed member's sections are first put in that order
/// ([`order_sections`]), so the linker, which takes the input sections of one output pattern in
/// section-table order, places them in that order whatever order the compiler emitted them in.
fn rename(ar: &str, archive: &Path, work: &Path, what: &str, renames: &BTreeMap<String, Vec<(String, String)>>, order: Option<&BTreeMap<String, Vec<String>>>) -> Result<usize, String> {
    if renames.is_empty() {
        return Ok(0);
    }
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    // Members are extracted and replaced by name: a duplicated name would be ambiguous.
    let names = Command::new(ar).arg("t").arg(archive).output().map_err(|e| format!("ar: {e}"))?;
    let names = String::from_utf8_lossy(&names.stdout).into_owned();
    for m in renames.keys() {
        if names.lines().filter(|l| l == m).count() != 1 {
            return Err(format!("{what}: archive member {m} is not unique in {}", archive.display()));
        }
    }
    let members: Vec<&String> = renames.keys().collect();
    run(Command::new(ar).current_dir(work).arg("x").arg(archive).args(&members))?;
    for (m, list) in renames {
        if let Some(names) = order.and_then(|o| o.get(m)) {
            let path = work.join(m);
            let mut obj = fs::read(&path).map_err(|e| format!("{what}: {m}: {e}"))?;
            order_sections(&mut obj, names).map_err(|e| format!("{what}: {m}: {e}"))?;
            fs::write(&path, obj).map_err(|e| format!("{what}: {m}: {e}"))?;
        }
        let mut cmd = Command::new("objcopy");
        for (from, to) in list {
            cmd.arg("--rename-section").arg(format!("{from}={to}"));
        }
        run(cmd.arg(work.join(m)))?;
    }
    run(Command::new(ar).current_dir(work).arg("r").arg(archive).args(&members))?;
    let _ = fs::remove_dir_all(work);
    crate::hottext::check_renames(&archive_headers(archive)?, renames).map_err(|e| format!("{}: {what}: {e}", archive.display()))
}

/// Rename the cold `.bss.*` sections of `archive` to `.lbss.*` in place; `work` is scratch.
/// Returns the number of sections renamed (each checked in the archive afterwards).
pub fn move_cold_bss(ar: &str, archive: &Path, work: &Path) -> Result<usize, String> {
    let cold = cold_sections(&archive_headers(archive)?);
    let renames = cold.into_iter().map(|(m, list)| (m, list.into_iter().map(|s| (s.clone(), format!(".lbss{}", &s[".bss".len()..]))).collect())).collect();
    rename(ar, archive, work, "coldbss", &renames, None)
}

const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_REL: u32 = 9;
const SHT_GROUP: u32 = 17;
const SHT_SYMTAB_SHNDX: u32 = 18;
const SHF_INFO_LINK: u64 = 0x40;
const SHF_LINK_ORDER: u64 = 0x80;
const SHN_LORESERVE: u32 = 0xff00;
const SHN_XINDEX: u32 = 0xffff;

fn rd<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], String> {
    b.get(at..at.wrapping_add(N)).and_then(|s| s.try_into().ok()).ok_or_else(|| format!("ELF: read past the end at {at:#x}"))
}
fn u16_at(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from(u16::from_le_bytes(rd(b, at)?)))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(rd(b, at)?))
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(rd(b, at)?))
}
fn put_u16(b: &mut [u8], at: usize, v: u32) -> Result<(), String> {
    let v = u16::try_from(v).map_err(|_| format!("ELF: {v:#x} does not fit 16 bits"))?;
    b.get_mut(at..at.wrapping_add(2)).ok_or("ELF: write past the end")?.copy_from_slice(&v.to_le_bytes());
    Ok(())
}
fn put_u32(b: &mut [u8], at: usize, v: u32) -> Result<(), String> {
    b.get_mut(at..at.wrapping_add(4)).ok_or("ELF: write past the end")?.copy_from_slice(&v.to_le_bytes());
    Ok(())
}
fn usz(v: u64) -> Result<usize, String> {
    usize::try_from(v).map_err(|_| format!("ELF: {v:#x} out of range"))
}

/// LAYOUT-STARTUP-STATE-LOCALE (ruling #4923): put the sections named `want` (each name exactly once
/// in the object) in `want`'s order, in place, in the ELF64 little-endian relocatable object `obj`.
/// They and their relocation sections (each right after its target) swap places among the
/// section-table slots they already hold, so every other section keeps
/// its index; the data stays where it is (each header carries its offset). What refers to a moved
/// section by index follows it: symbols (`st_shndx`, or their `SHT_SYMTAB_SHNDX` entry), the target
/// of relocation sections (`sh_info`), group members and `SHF_LINK_ORDER` links. Any other
/// `sh_link` naming a moved section is refused. Returns the number of sections whose slot changed.
pub(crate) fn order_sections(obj: &mut [u8], want: &[String]) -> Result<usize, String> {
    if obj.get(..7) != Some(&[0x7f, b'E', b'L', b'F', 2, 1, 1][..]) || u16_at(obj, 16)? != 1 {
        return Err("not an ELF64 little-endian relocatable object".into());
    }
    let (shoff, entsize) = (usz(u64_at(obj, 0x28)?)?, u16_at(obj, 0x3a)?);
    if entsize != 64 {
        return Err(format!("section header size {entsize}, want 64"));
    }
    let hdr = |i: usize| shoff + i * 64;
    let mut shnum = usz(u64::from(u16_at(obj, 0x3c)?))?;
    if shnum == 0 {
        shnum = usz(u64_at(obj, hdr(0) + 32)?)?;
    }
    let mut shstrndx = usz(u64::from(u16_at(obj, 0x3e)?))?;
    if shstrndx == SHN_XINDEX as usize {
        shstrndx = usz(u64::from(u32_at(obj, hdr(0) + 40)?))?;
    }
    if shstrndx >= shnum {
        return Err(format!("section name table {shstrndx} of {shnum}"));
    }
    let strtab = usz(u64_at(obj, hdr(shstrndx) + 24)?)?;
    let mut names = Vec::with_capacity(shnum);
    for i in 0..shnum {
        let at = strtab + usz(u64::from(u32_at(obj, hdr(i))?))?;
        let tail = obj.get(at..).ok_or("ELF: section name past the end")?;
        names.push(&tail[..tail.iter().position(|&c| c == 0).ok_or("ELF: unterminated section name")?]);
    }
    // A wanted section's relocation sections move with it, right after it. BFD (objcopy, ld) makes a
    // relocation section's target when it reads the relocation section, so a target left in a later
    // slot than its relocations is taken in the relocations' place, the emission order (the `ld -r`
    // join and the C member kept `stdout` and `__tzname` there).
    let mut relocs: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..shnum {
        let kind = u32_at(obj, hdr(i) + 4)?;
        if kind == SHT_REL || kind == SHT_RELA {
            relocs.entry(usz(u64::from(u32_at(obj, hdr(i) + 44)?))?).or_default().push(i);
        }
    }
    let mut from = Vec::with_capacity(want.len());
    for w in want {
        let hits: Vec<usize> = (0..shnum).filter(|&i| names[i] == w.as_bytes()).collect();
        match hits[..] {
            [i] => {
                from.push(i);
                from.extend(relocs.get(&i).into_iter().flatten());
            }
            _ => return Err(format!("{} sections named {w}, want exactly one", hits.len())),
        }
    }
    let mut slots = from.clone();
    slots.sort_unstable();
    if slots.windows(2).any(|p| p[0] == p[1]) {
        return Err("a section is named twice in the order".into());
    }
    // map[old index] = new index; identity outside the moved set.
    let mut map: Vec<u32> = (0..shnum).map(|i| u32::try_from(i).map_err(|_| "ELF: too many sections".to_string())).collect::<Result<_, _>>()?;
    for (k, &old) in from.iter().enumerate() {
        map[old] = u32::try_from(slots[k]).map_err(|_| "ELF: too many sections")?;
    }
    let moved = from.iter().zip(&slots).filter(|(a, b)| a != b).count();
    if moved == 0 {
        return Ok(0);
    }
    let remap = |i: u32| map.get(i as usize).copied().unwrap_or(i);
    let is_moved = |i: u32| (i as usize) < shnum && map[i as usize] as usize != i as usize;
    // Every header's references, then the symbol and group contents, then the headers themselves.
    let mut xindex: BTreeMap<usize, usize> = BTreeMap::new();
    for i in 0..shnum {
        let (kind, flags, link) = (u32_at(obj, hdr(i) + 4)?, u64_at(obj, hdr(i) + 8)?, u32_at(obj, hdr(i) + 40)?);
        if kind == SHT_SYMTAB_SHNDX {
            xindex.insert(link as usize, i);
        }
        if is_moved(link) {
            if flags & SHF_LINK_ORDER == 0 {
                return Err(format!("section {i} (type {kind:#x}) links to a reordered section"));
            }
            put_u32(obj, hdr(i) + 40, remap(link))?;
        }
        if kind == SHT_REL || kind == SHT_RELA || flags & SHF_INFO_LINK != 0 {
            let info = u32_at(obj, hdr(i) + 44)?;
            put_u32(obj, hdr(i) + 44, remap(info))?;
        }
        if kind == SHT_GROUP {
            let (off, size) = (usz(u64_at(obj, hdr(i) + 24)?)?, usz(u64_at(obj, hdr(i) + 32)?)?);
            for at in (off + 4..off + size).step_by(4) {
                let v = u32_at(obj, at)?;
                put_u32(obj, at, remap(v))?;
            }
        }
    }
    for i in 0..shnum {
        if u32_at(obj, hdr(i) + 4)? != SHT_SYMTAB {
            continue;
        }
        let (off, size) = (usz(u64_at(obj, hdr(i) + 24)?)?, usz(u64_at(obj, hdr(i) + 32)?)?);
        let ext = match xindex.get(&i) {
            Some(&x) => Some(usz(u64_at(obj, hdr(x) + 24)?)?),
            None => None,
        };
        for n in 0..size / 24 {
            let at = off + n * 24 + 6;
            let raw = u16_at(obj, at)?;
            let (old, via_ext) = match (raw, ext) {
                (SHN_XINDEX, Some(e)) => (u32_at(obj, e + n * 4)?, true),
                (SHN_XINDEX, None) => return Err(format!("symbol {n}: SHN_XINDEX without a SHT_SYMTAB_SHNDX section")),
                (r, _) if r == 0 || r >= SHN_LORESERVE => continue,
                (r, _) => (r, false),
            };
            if !is_moved(old) {
                continue;
            }
            let new = remap(old);
            match ext {
                Some(e) if new >= SHN_LORESERVE || via_ext => {
                    put_u16(obj, at, SHN_XINDEX)?;
                    put_u32(obj, e + n * 4, new)?;
                }
                _ if new < SHN_LORESERVE => put_u16(obj, at, new)?,
                _ => return Err(format!("symbol {n}: section {new} needs SHN_XINDEX and there is no SHT_SYMTAB_SHNDX section")),
            }
        }
    }
    let old_headers: Vec<[u8; 64]> = from.iter().map(|&i| rd(obj, hdr(i))).collect::<Result<_, _>>()?;
    for (k, h) in old_headers.iter().enumerate() {
        obj.get_mut(hdr(slots[k])..hdr(slots[k]) + 64).ok_or("ELF: header past the end")?.copy_from_slice(h);
    }
    Ok(moved)
}

/// STARTUP-FAULTS-RESTORE: the small statics every static process writes (or reads, then writes)
/// before `main` and in its first `setlocale`, by key suffix (crate hashes dropped,
/// [`crate::hottext::key`]). Their `.data.<symbol>` / `.bss.<symbol>` sections are renamed
/// [`STATE_SECTION`]: GNU ld's default script places `*(.data1)` right after `.data`, next to the
/// start of `.bss`, so they share one or two pages instead of one page each among the rest of
/// `.data` and `.bss` (printf: `SYSINFO_EHDR` and `AVX2_ERMS_FROM` dirtied a `.data` page each,
/// `global_locale::{GLOBAL, LOCK}` a `.bss` page of their own, read first: two faults).
/// `liella_machine_scan_level` (scan.S) is placed there by its own `.section` directive.
/// Measured (one build host, kit printf, `setarch -R`): these six and scan_level 62 -> 61; adding
/// `cpu::{COPY_MIN, FILL_MIN, ERMS_LARGE_FROM}`, fold.S's `cmp_sse` words and
/// `global_locale::MAPS` gained nothing more (61: the freed pages got other first writers), so
/// they stay where they were.
/// STARTUP-FAULTS-RESTORE-3: `nscache::NSCACHE` (608 bytes, written by `nscache::start` in every
/// process) joined: after a merge with next reordered `.data` it sat on a `.data` page of its own,
/// startup only 17 -> 18 (Haswell-EP build host, perf page-fault map of both builds).
/// LAYOUT-DATA-CLUSTERS (D-X.64): `timer::EPOLL`, which the fork child reads (`fork::child_reset`):
/// on a page the parent's start-up never touched, it cost the child a fault of its own. Every other
/// writable start-up key of `crates/libc-so/startup-data.keys` joined too (now [`STATE_LIST_FILE`]), and the
/// start object aligns the cluster to the smallest power of two not below its size
/// (`startup_ro::cluster_aligns`), so it takes the fewest pages wherever `.data` ends.
/// LAYOUT-DATA-CLUSTERS-2 (D-X.64): `heap::HEAP` (5376 bytes, above the 4 KiB keyed limit of the time), whose head and
/// tail every process writes (its first allocation): in `.lbss` it took two pages whose place rustc's
/// emission order decided (beside `RESERVE_MEM`, never written at start-up); here it costs 5376 file bytes.
/// The rename runs before [`move_cold_bss`], so a keyed object of 1 KiB to 4 KiB (`process::EXIT_HANDLERS`,
/// 1048 bytes) joins too instead of `.lbss`.
/// LAYOUT-DATA-CLUSTERS-3 (D-X.67): `time::zone::NEXT_ID` (4 bytes) and `__tzname` (a `section` entry),
/// written by the first zone load (localtime_r, tzset): next's `.data` ended with `__tzname`, which shared
/// the cluster's first page; with the cluster aligned it sat alone on a `.data` page (first localtime_r
/// 20 -> 21 faults, fast, perf page-fault map). Here they cost no page of their own.
/// LAYOUT-DATA-CLUSTERS-6: the cluster's members are DECLARED in one hand-kept list, never taken from a
/// trace. The trace-derived keys (`startup-data.keys`) could not converge: an object already moved into
/// `.data1` dropped out of the next trace, so keys flipped between regenerations, and one profile's pass
/// dropped the other's keys. The list names each static once for every profile (LLVM's split parts and
/// ThinLTO suffixes match their static), and a trace only checks it (hot_text_products).
pub const STATE_LIST_FILE: &str = "crates/libc-so/startup-state.list";
/// See [`STATE_LIST_FILE`].
pub const STATE_SECTION: &str = ".data1";
/// A declared section larger than this is refused: zero-initialised members of the cluster become file
/// bytes, and a large table would spread the cluster over pages start-up does not write.
pub const STATE_DECLARED_MAX: u64 = 8192;

/// One entry of [`STATE_LIST_FILE`].
#[derive(Clone, Debug, PartialEq)]
enum Declared {
    /// A Rust static by path, as the tail of its v0 symbol (`C15liella_boundary4heap4HEAP`).
    Path(String),
    /// An object in `.data.<name>` or `.bss.<name>`.
    Symbol(String),
    /// A whole section name.
    Section(String),
}

/// The line of [`STATE_LIST_FILE`] after which every entry is first-call state ([`FIRST_CALL_SECTION`]).
pub const FIRST_CALL_LINE: &str = "cluster first-call";

/// The entries of a `startup-state.list` text: (entry, the line as written, may be absent: `fast-only`,
/// first-call state: after [`FIRST_CALL_LINE`]).
fn declared(text: &str) -> Result<Vec<(Declared, String, bool, bool)>, String> {
    let (mut out, mut first) = (Vec::new(), false);
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == FIRST_CALL_LINE {
            if first {
                return Err(format!("{STATE_LIST_FILE}:{}: a second `{FIRST_CALL_LINE}`", n + 1));
            }
            first = true;
            continue;
        }
        let mut f: Vec<&str> = line.split_whitespace().collect();
        let optional = f.last() == Some(&"fast-only");
        if optional {
            f.pop();
        }
        let ident = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && !s.starts_with(|c: char| c.is_ascii_digit() || c == '_');
        let d = match f.as_slice() {
            ["symbol", name] => Declared::Symbol((*name).to_string()),
            ["section", name] => Declared::Section((*name).to_string()),
            [path] if path.contains("::") && path.split("::").all(ident) => {
                Declared::Path(std::iter::once("C".to_string()).chain(path.split("::").map(|s| format!("{}{s}", s.len()))).collect())
            }
            _ => return Err(format!("{STATE_LIST_FILE}:{}: not an entry: {line}", n + 1)),
        };
        out.push((d, f.join(" "), optional, first));
    }
    Ok(out)
}

/// `name` without an LLVM split number (`.0`) or ThinLTO suffix (`.llvm.<n>`).
fn split_stem(name: &str) -> &str {
    let name = name.rsplit_once(".llvm.").filter(|(_, n)| n.bytes().all(|b| b.is_ascii_digit())).map_or(name, |(s, _)| s);
    match name.rsplit_once('.') {
        Some((s, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => s,
        _ => name,
    }
}

/// The declared start-up state sections per archive member ([`STATE_LIST_FILE`]), from the members'
/// section headers (whole names). Every entry not marked `fast-only` must name at least one section: a
/// renamed or removed static is an error, not a silent return to a page of its own.
/// Each section comes with the position of its entry in the list.
fn state_sections(members: &[(String, Vec<Shdr>)], list: &[(Declared, String, bool, bool)]) -> Result<BTreeMap<String, Vec<(String, usize)>>, String> {
    let mut out: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    let (mut seen, mut big) = (vec![0usize; list.len()], Vec::new());
    for (member, s) in members.iter().flat_map(|(m, h)| h.iter().map(move |s| (m, s))) {
        let name = s.name.as_str();
        let raw = name.strip_prefix(".data.").or_else(|| name.strip_prefix(".bss."));
        // LLVM may split a static and number the part (`.data.<symbol>.0`); under ThinLTO a split part is
        // `<symbol>.0.llvm.<n>`: both suffixes go (timer::EPOLL).
        let k = raw.map(|sym| match sym.rsplit_once('.') {
            Some((s, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !s.ends_with(".llvm") => crate::hottext::key(s),
            _ => crate::hottext::key(sym),
        });
        let hit = list.iter().position(|(d, _, _, _)| match d {
            Declared::Section(n) => n == name,
            Declared::Symbol(n) => raw == Some(n.as_str()),
            Declared::Path(p) => k.as_deref().is_some_and(|k| split_stem(k).ends_with(p.as_str())),
        });
        let Some(i) = hit else { continue };
        seen[i] += 1;
        if s.size > STATE_DECLARED_MAX {
            big.push(format!("{name} ({} bytes)", s.size));
        }
        out.entry(member.clone()).or_default().push((name.to_string(), i));
    }
    let missing: Vec<&str> = list.iter().zip(&seen).filter(|((_, _, optional, _), n)| !optional && **n == 0).map(|((_, l, _, _), _)| l.as_str()).collect();
    if !missing.is_empty() {
        return Err(format!("start-up state: no .data/.bss section for {} (renamed or removed? update {STATE_LIST_FILE})", missing.join(", ")));
    }
    if !big.is_empty() {
        return Err(format!("start-up state: declared sections over {STATE_DECLARED_MAX} bytes: {} (declare only small state in {STATE_LIST_FILE})", big.join(", ")));
    }
    Ok(out)
}

/// Rename the declared start-up state sections of `archive` ([`STATE_LIST_FILE`] under `root`) to
/// [`STATE_SECTION`] in place; `work` is scratch. Returns the number renamed.
pub fn move_startup_state(ar: &str, archive: &Path, root: &Path, work: &Path) -> Result<usize, String> {
    let list = declared(&fs::read_to_string(root.join(STATE_LIST_FILE)).map_err(|e| format!("{STATE_LIST_FILE}: {e}"))?)?;
    let members = archive_headers(archive)?;
    let plan = state_plan(&members, &list)?;
    if let Some((m, first)) = &plan.lead {
        // The cluster's first section starts a page, before the renames make the names ambiguous.
        let _ = fs::remove_dir_all(work);
        fs::create_dir_all(work).map_err(|e| e.to_string())?;
        run(Command::new(ar).current_dir(work).arg("x").arg(archive).arg(m))?;
        run(Command::new("objcopy").arg(format!("--set-section-alignment={first}={FIRST_CALL_ALIGN}")).arg(work.join(m)))?;
        run(Command::new(ar).current_dir(work).arg("r").arg(archive).arg(m))?;
        let _ = fs::remove_dir_all(work);
    }
    let n = rename(ar, archive, work, "start-up state", &plan.renames, Some(&plan.order))?;
    if let Some((m, _)) = &plan.lead {
        let after = archive_headers(archive)?;
        let lead = after.iter().filter(|(n, _)| n == m).flat_map(|(_, h)| h).filter(|s| s.name == FIRST_CALL_SECTION && s.align >= FIRST_CALL_ALIGN).count();
        if lead != 1 {
            return Err(format!("start-up state: {m}: {lead} {FIRST_CALL_SECTION} sections aligned to {FIRST_CALL_ALIGN}, want 1 (objcopy --set-section-alignment)"));
        }
    }
    Ok(n)
}

/// LAYOUT-STARTUP-STATE-LOCALE-3 (ruling #4942): the zero-filled first-call state (entries after
/// [`FIRST_CALL_LINE`]: what the first setlocale, setenv, localtime_r, mktime and catalog lookups touch,
/// never a process that makes none of those calls) goes here, after `.bss` in GNU ld's default script, not
/// into [`STATE_SECTION`]: there it lay between the every-process tier and the C members' start-up words
/// (`__libc_stack_end`, `environ`, linked after the Rust object), and the `true` probe touched one page
/// more (intl::RESOLVED alone is 4 KiB). In `.lbss` it stays in the one writable segment a static PIE
/// needs (`.ldata` would start a second). Initialised first-call objects cannot be zero-filled and stay in
/// [`STATE_SECTION`], after the every-process tier (list order).
pub const FIRST_CALL_SECTION: &str = ".lbss";
/// The first-call cluster starts a page, so its page split depends only on the list and the objects'
/// sizes (anonymous zero-filled pages have no fault-around window to fit).
pub const FIRST_CALL_ALIGN: u64 = 4096;

/// What [`move_startup_state`] does to the archive.
#[derive(Debug, Default)]
struct StatePlan {
    /// member -> (section, new name).
    renames: BTreeMap<String, Vec<(String, String)>>,
    /// member -> section-table order: the every-process tier, the first-call cluster, the member's
    /// cold tables (also `.lbss` once renamed: after the cluster, never inside it).
    order: BTreeMap<String, Vec<String>>,
    /// The member holding the first-call cluster and the cluster's first section.
    lead: Option<(String, String)>,
}

/// The renames and order of the declared start-up state. The first-call cluster is one member's: the one
/// with the most first-call zero-filled bytes (the library's Rust object). Another member's first-call
/// state stays in [`STATE_SECTION`]: a second piece in `.lbss` would land among the cold tables, on a page
/// of its own wherever they put it.
fn state_plan(members: &[(String, Vec<Shdr>)], list: &[(Declared, String, bool, bool)]) -> Result<StatePlan, String> {
    let state = state_sections(members, list)?;
    let order = state_order(&state);
    // member -> its zero-filled first-call sections (list entries after FIRST_CALL_LINE) and their bytes.
    let mut zero: BTreeMap<String, (Vec<String>, u64)> = BTreeMap::new();
    for (m, secs) in &state {
        for (s, i) in secs {
            let nobits = members.iter().filter(|(n, _)| n == m).flat_map(|(_, h)| h).find(|h| &h.name == s && h.kind == SHT_NOBITS);
            if let Some(h) = nobits.filter(|_| list[*i].3) {
                let e = zero.entry(m.clone()).or_default();
                e.0.push(s.clone());
                e.1 += h.size;
            }
        }
    }
    let lead = zero.iter().max_by_key(|(m, (_, b))| (*b, std::cmp::Reverse((*m).clone()))).map(|(m, _)| m.clone());
    let cold = cold_sections(members);
    let mut plan = StatePlan::default();
    for (m, secs) in order {
        let cluster: &[String] = match (&lead, zero.get(&m)) {
            (Some(l), Some((z, _))) if *l == m => z,
            _ => &[],
        };
        let (first, base): (Vec<String>, Vec<String>) = secs.into_iter().partition(|s| cluster.contains(s));
        let renames = base.iter().map(|s| (s.clone(), STATE_SECTION.to_string())).chain(first.iter().map(|s| (s.clone(), FIRST_CALL_SECTION.to_string()))).collect();
        plan.renames.insert(m.clone(), renames);
        let mut o: Vec<String> = base.into_iter().chain(first.iter().cloned()).collect();
        if let Some(f) = first.first() {
            let tables: Vec<String> = cold.get(&m).into_iter().flatten().filter(|c| !o.contains(c)).cloned().collect();
            o.extend(tables);
            plan.lead = Some((m.clone(), f.clone()));
        }
        plan.order.insert(m, o);
    }
    Ok(plan)
}

/// LAYOUT-STARTUP-STATE-LOCALE (ruling #4923): the order of each member's start-up state in
/// [`STATE_SECTION`]: list order, then section name (the split parts of one static). The whole
/// library is one LTO object, whose compiler emitted the same objects in an order that changed with
/// any code change anywhere; the `.data1` pages then held other objects and the start-up probes'
/// counts moved with unrelated changes (tool 54 -> 55 at 9 of the pads measured). In list order the
/// cluster's layout depends only on the list and the objects' sizes.
fn state_order(state: &BTreeMap<String, Vec<(String, usize)>>) -> BTreeMap<String, Vec<String>> {
    state
        .iter()
        .map(|(m, secs)| {
            let mut v: Vec<&(String, usize)> = secs.iter().collect();
            v.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            (m.clone(), v.into_iter().map(|(s, _)| s.clone()).collect())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_startup_state_is_found_whatever_its_symbol_suffixes() {
        // LAYOUT-DATA-CLUSTERS-6 (D-X.64; no spec clause, layout of the products): a declared Rust static is
        // picked whatever its crate hash, split number or ThinLTO suffix, a `symbol` and a `section` entry by
        // name; an undeclared static (also one whose path only ends like a declared one) stays; a missing
        // entry is an error unless it is `fast-only`; a declared section over STATE_DECLARED_MAX is refused.
        let list = declared("# c\nliella_boundary::heap::HEAP\nliella_boundary::timer::EPOLL # why\nsymbol __rseq_size\nsection .bss.environ\nliella_boundary::dispatch::FMA  fast-only\n").unwrap();
        assert_eq!(list.len(), 5);
        assert_eq!(list[0].0, Declared::Path("C15liella_boundary4heap4HEAP".into()));
        let mut members = vec![
            ("a.o".to_string(), vec![sec(".data._RNvNtCsQ9_15liella_boundary5timer5EPOLL.1", 1, 8), sec(".bss._RNvNtCsAbC12_15liella_boundary4heap4HEAP", SHT_NOBITS, 5376), sec(".data._RNvNtCsQ9_15liella_boundary5timer5EPOLL.0.llvm.42", 1, 8)]),
            ("b.o".to_string(), vec![sec(".bss.__rseq_size", SHT_NOBITS, 4), sec(".bss.environ", SHT_NOBITS, 8), sec(".data._RNvNtCsQ9_15liella_boundary4heap5OTHER", 1, 8), sec(".bss._RNvNtCsQ9_15liella_boundary5xheap4HEAP", SHT_NOBITS, 8)]),
        ];
        let got = state_sections(&members, &list).unwrap();
        assert_eq!((got["a.o"].len(), got["b.o"].len()), (3, 2));
        assert!(!got["b.o"].iter().any(|(n, _)| n.contains("OTHER") || n.contains("xheap")));
        // LAYOUT-STARTUP-STATE-LOCALE (#4923): list order, then name, whatever the headers' order.
        let order = state_order(&got);
        assert_eq!(order["a.o"], [".bss._RNvNtCsAbC12_15liella_boundary4heap4HEAP", ".data._RNvNtCsQ9_15liella_boundary5timer5EPOLL.0.llvm.42", ".data._RNvNtCsQ9_15liella_boundary5timer5EPOLL.1"]);
        assert_eq!(order["b.o"], [".bss.__rseq_size", ".bss.environ"]);
        // LAYOUT-STARTUP-STATE-LOCALE-3 (ruling #4942): entries after the cluster line are first-call state.
        // The zero-filled ones of the member with the most such bytes go to the page-aligned `.lbss` cluster,
        // in list order, ahead of that member's cold tables; an initialised one, and another member's, stay
        // in `.data1` after the every-process tier.
        let list = declared("liella_boundary::heap::HEAP\ncluster first-call\nliella_legacy::intl::RESOLVED\nsymbol stdout\nsymbol __daylight\nliella_legacy::intl::LOCK\n").unwrap();
        assert_eq!(list.iter().map(|e| e.3).collect::<Vec<_>>(), [false, true, true, true, true]);
        assert!(declared("cluster first-call\ncluster first-call\n").unwrap_err().contains("a second"));
        let members = vec![
            ("r.o".to_string(), vec![sec(".bss.big", SHT_NOBITS, 2048), sec(".bss._RNvNtCs1_13liella_legacy4intl4LOCK", SHT_NOBITS, 4), sec(".data.stdout", 1, 8), sec(".bss._RNvNtCs1_13liella_legacy4intl8RESOLVED", SHT_NOBITS, 4096), sec(".bss._RNvNtCs1_15liella_boundary4heap4HEAP", SHT_NOBITS, 5376)]),
            ("c.o".to_string(), vec![sec(".bss.__daylight", SHT_NOBITS, 4)]),
        ];
        let plan = state_plan(&members, &list).unwrap();
        let to = |m: &str, s: &str, want: &str| plan.renames[m].iter().any(|(f, t)| f.ends_with(s) && t == want);
        assert!(to("r.o", "4HEAP", ".data1") && to("r.o", "RESOLVED", ".lbss") && to("r.o", "4LOCK", ".lbss") && to("r.o", "stdout", ".data1") && to("c.o", "__daylight", ".data1"), "{:?}", plan.renames);
        assert_eq!(plan.order["r.o"], [".bss._RNvNtCs1_15liella_boundary4heap4HEAP", ".data.stdout", ".bss._RNvNtCs1_13liella_legacy4intl8RESOLVED", ".bss._RNvNtCs1_13liella_legacy4intl4LOCK", ".bss.big"]);
        assert_eq!(plan.lead, Some(("r.o".to_string(), ".bss._RNvNtCs1_13liella_legacy4intl8RESOLVED".to_string())));
        assert!(state_sections(&members[1..], &list).unwrap_err().contains("for liella_boundary::heap::HEAP, liella_boundary::timer::EPOLL ("));
        members[0].1[0].size = STATE_DECLARED_MAX + 1;
        assert!(state_sections(&members, &list).unwrap_err().contains("over 8192 bytes"));
        assert!(declared("liella_boundary::_x::Y\n").is_err() && declared("foo bar\n").is_err());
    }

    #[test]
    fn reordered_sections_keep_their_symbols_and_relocations() {
        // LAYOUT-STARTUP-STATE-LOCALE (#4923; no spec clause, layout of the products): the sections named
        // are linked in the order given, and every symbol and relocation still reaches its own data: the
        // program checks the values through pointers (relocations in moved sections and against moved
        // sections, a local and a global symbol) and the addresses follow the order. As in the build, the
        // sections are then renamed to one name (objcopy) and joined (`ld -r`) before the link: both read the
        // object through BFD, which takes a section where its relocations are. The pointers sit at both ends
        // of the emitted order and the order is reversed, so a relocated section always lands after its
        // relocation section's slot unless that moves with it.
        let dir = std::env::temp_dir().join(format!("liella-order-sections-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (src, obj, joined, exe) = (dir.join("o.c"), dir.join("o.o"), dir.join("j.o"), dir.join("o"));
        let c = "extern int a; static int s; int *pa = &a; int a = 1; int b = 2; static int s = 4; int c = 3; int *ps = &s;\n\
                 int main(void) { return *pa == 1 && b == 2 && c == 3 && *ps == 4 && s == 4 && pa == &a ? 0 : 1; }\n";
        fs::write(&src, c).unwrap();
        assert!(Command::new("cc").args(["-c", "-O0", "-fdata-sections", "-fno-pie", "-o"]).arg(&obj).arg(&src).status().unwrap().success());
        let headers = Command::new("readelf").arg("-SW").arg(&obj).output().unwrap();
        let mut want: Vec<String> = String::from_utf8_lossy(&headers.stdout).lines().filter_map(|l| l.split_once("] ").and_then(|(_, r)| r.split_whitespace().next())).filter(|n| n.starts_with(".data.")).map(str::to_string).collect();
        assert_eq!(want.len(), 6, "emitted .data sections: {want:?}");
        want.reverse();
        let mut bytes = fs::read(&obj).unwrap();
        assert!(order_sections(&mut bytes, &want).unwrap() > 0);
        assert_eq!(order_sections(&mut bytes.clone(), &want).unwrap(), 0, "a second pass moves nothing");
        assert!(order_sections(&mut bytes.clone(), &[".data.nope".to_string()]).unwrap_err().contains("0 sections named .data.nope"));
        fs::write(&obj, &bytes).unwrap();
        let mut objcopy = Command::new("objcopy");
        for w in &want {
            objcopy.arg("--rename-section").arg(format!("{w}=.data1"));
        }
        assert!(objcopy.arg(&obj).status().unwrap().success());
        assert!(Command::new("ld").arg("-r").arg("-o").arg(&joined).arg(&obj).status().unwrap().success());
        assert!(Command::new("cc").args(["-no-pie", "-o"]).arg(&exe).arg(&joined).status().unwrap().success());
        assert_eq!(Command::new(&exe).status().unwrap().code(), Some(0), "a reordered symbol or relocation lost its data");
        let nm = Command::new("nm").arg("-n").arg(&exe).output().unwrap();
        let names: Vec<String> = want.iter().map(|w| w[".data.".len()..].to_string()).collect();
        let ours: Vec<String> = String::from_utf8_lossy(&nm.stdout).lines().filter_map(|l| l.split_whitespace().nth(2)).filter(|n| names.iter().any(|w| w == n)).map(str::to_string).collect();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(ours, names, "linked order");
    }

    fn sec(name: &str, kind: u32, size: u64) -> Shdr {
        Shdr { name: name.to_string(), kind, size, align: 8 }
    }

    #[test]
    fn picks_large_bss_sections_per_member() {
        // PAGEFAULTS-4 (no spec clause; layout of the products): zero-filled `.bss.*` of at least
        // MIN bytes, by whole name, whatever its length (readelf -SW cut names at 256 bytes).
        let long = format!(".bss.{}", "x".repeat(300));
        let members = vec![
            ("a.o".to_string(), vec![sec(".text.f", 1, 0x400), sec(".bss.big", SHT_NOBITS, 0x400), sec(".bss.small", SHT_NOBITS, 0x3ff), sec(".tbss.t", SHT_NOBITS, 0x1000), sec(".data.d", 1, 0x2000)]),
            ("b.o".to_string(), vec![sec(".bss.huge", SHT_NOBITS, 0x10000), sec(&long, SHT_NOBITS, 0x800), sec(".bss.notnobits", 1, 0x800)]),
        ];
        let got = cold_sections(&members);
        assert_eq!(got.len(), 2);
        assert_eq!(got["a.o"], vec![".bss.big".to_string()]);
        assert_eq!(got["b.o"], vec![".bss.huge".to_string(), long]);
    }
}
