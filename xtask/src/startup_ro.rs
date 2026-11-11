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
/// The tiers of `crates/libc-so/hot-text.list` whose functions' read-only data joins the group (ruling
/// #4313): the probes whose page faults `crates/core/tests/hot_text_products.rs` bounds (start-up, the
/// setlocale + printf program and the coreutils-shaped tool) run functions of these tiers; `fork` and
/// `extra` (other traced programs) stay out.
const HOT_TIERS: [&str; 5] = ["startup", "stdio", "strings", "locale", "tool"];
/// The functions of the fault-bounded probe modes whose functions no [`HOT_TIERS`] tier holds (the
/// localtime probes: their zone functions are in `extra`), traced per mode by
/// `tools/hottext/probe_text.sh` (LAYOUT-STARTUP-RODATA-4: leaving them out split the `TZ` and `TZDIR`
/// name literals off the windows start-up had faulted in, first localtime_r 19 -> 21 faults).
const PROBE_KEYS: &str = "crates/libc-so/probe-text.keys";
/// The prefix the weakening pass gives a library name (`weaken_archive`); start-up keys name the
/// final archive's symbols, this pass runs before it.
const LIB_PREFIX: &str = "__liella_libc_";

/// A section of one archive member (`readelf -SW`).
#[derive(Clone, Debug, Default, PartialEq)]
struct Sec {
    name: String,
    size: u64,
    /// File offset (`Off`): with the name, it tells two sections apart whose names `readelf` cut alike.
    off: u64,
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
        if let Some((name, tail)) = l.strip_prefix("Relocation section '").and_then(|r| r.rsplit_once('\'')) {
            // `readelf` cuts a long name: the offset identifies the section.
            let off = tail.strip_prefix(" at offset 0x").and_then(|r| r.split_whitespace().next()).and_then(|h| u64::from_str_radix(h, 16).ok());
            rel = m.secs.iter().find(|(_, s)| s.name == name && Some(s.off) == off).map(|(i, _)| *i);
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
            let (Ok(off), Ok(size), Ok(info), Ok(align)) = (u64::from_str_radix(g[3], 16), u64::from_str_radix(g[4], 16), g[lk + 1].parse(), g[lk + 2].parse()) else { continue };
            m.secs.insert(idx, Sec { name: g[0].to_string(), size, off, align, flags: flags.to_string(), info });
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

/// The function of a function section: `.text.sorted.h<n>.<f>` (grouped hot text) or `.text.<f>` -> `<f>`.
fn text_function(section: &str) -> Option<&str> {
    let f = section.strip_prefix(".text.")?;
    Some(f.strip_prefix("sorted.h").and_then(|r| r.split_once('.')).filter(|(n, _)| n.bytes().all(|b| b.is_ascii_digit())).map_or(f, |(_, f)| f))
}

/// What [`plan`] returns: the sections to rename per member, the bytes, the (size, member:section) list.
type Plan = (BTreeMap<String, BTreeSet<String>>, u64, Vec<(u64, String)>);

/// The plan: per member, the sections (by name) to rename to [`SECTION`]; and the bytes [`SECTION`]
/// holds over the archive (sizes plus alignment slack, the renamed ones and those already there).
/// The sections of the hot functions (by symbol, and by section name as the hot-text grouping matches:
/// `.text.sorted.h<n>.<f>` after it; a symbol ThinLTO promoted carries a `.llvm.<n>` suffix).
fn starts(members: &[(String, Member)], text_keys: &BTreeSet<String>) -> Vec<(usize, usize)> {
    let mut todo = Vec::new();
    for (mi, (_, m)) in members.iter().enumerate() {
        todo.extend(m.syms.values().filter(|s| matches!(s.ty.as_str(), "FUNC" | "NOTYPE") && keyed(&s.name, text_keys)).filter_map(|s| s.ndx).map(|ndx| (mi, ndx)));
        todo.extend(m.secs.iter().filter(|(_, s)| s.flags.contains('X') && text_function(&s.name).is_some_and(|f| keyed(f, text_keys))).map(|(i, _)| (mi, *i)));
    }
    todo
}

/// The (member, section) symbol `si` of member `mi` stands for: its own section, or a global definition elsewhere.
fn target(members: &[(String, Member)], mi: usize, si: usize) -> Option<(usize, usize)> {
    let s = members[mi].1.syms.get(&si)?;
    match s.ndx {
        Some(ndx) => Some((mi, ndx)),
        None => members.iter().enumerate().find_map(|(mj, (_, m))| m.syms.values().find(|d| d.name == s.name && d.ndx.is_some() && matches!(d.bind.as_str(), "GLOBAL" | "WEAK")).map(|d| (mj, d.ndx.unwrap_or(0)))),
    }
}

fn plan(members: &[(String, Member)], text_keys: &BTreeSet<String>, ro_keys: &BTreeSet<String>) -> Plan {
    let mut chosen = reached(members, text_keys);
    for (mi, (_, m)) in members.iter().enumerate() {
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

/// The sections the start-up functions' relocations address (`text_keys`: [`starts`]), and
/// transitively what a reached relocation-read-only table or read-only constant addresses.
fn reached(members: &[(String, Member)], text_keys: &BTreeSet<String>) -> BTreeSet<(usize, usize)> {
    // Defined global symbols -> (member, section index), for a relocation against another member's object.
    let mut global: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (mi, (_, m)) in members.iter().enumerate() {
        for s in m.syms.values() {
            if let (Some(ndx), "GLOBAL" | "WEAK") = (s.ndx, s.bind.as_str()) {
                global.entry(&s.name).or_insert((mi, ndx));
            }
        }
    }
    // Every section a hot function's relocations address, and (transitively) what a relocation-read-only
    // table (`.data.rel.ro*`: statics holding `&str`s and slices) or a chosen constant addresses: the
    // constants such a function reaches through a static are read with it.
    let mut chosen: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut todo = starts(members, text_keys);
    let mut seen: BTreeSet<(usize, usize)> = todo.iter().copied().collect();
    while let Some((mi, from)) = todo.pop() {
        let m = &members[mi].1;
        for (_, syms) in m.relocs.iter().filter(|(target, _)| *target == from) {
            for si in syms {
                let Some(s) = m.syms.get(si) else { continue };
                let at = match s.ndx {
                    Some(ndx) => Some((mi, ndx)),
                    None => global.get(s.name.as_str()).copied(),
                };
                let Some(at) = at else { continue };
                chosen.insert(at);
                let follow = members[at.0].1.secs.get(&at.1).is_some_and(|t| t.name.starts_with(".data.rel.ro") || read_only_data(t));
                if follow && seen.insert(at) {
                    todo.push(at);
                }
            }
        }
    }
    chosen
}

/// LAYOUT-DATA-CLUSTERS (D-X.64): the head of `.data.rel.ro`. GNU ld's default script places
/// `*(.data.rel.ro.local* ...)` before `*(.data.rel.ro .data.rel.ro.* ...)`, so a relro section renamed
/// `.data.rel.ro.local.<rest>` joins the start object's empty, aligned `.data.rel.ro.local.liella_cluster`
/// at the head of the output section: the start-up-read relro objects take their own window(s), which no
/// longer move with the size of the rest of `.data.rel.ro` or of the segments before it.
pub const RELRO_HEAD: &str = ".data.rel.ro.local";

/// The relocation-read-only sections to move to [`RELRO_HEAD`], per member: each `.data.rel.ro*`
/// section the start-up functions reach ([`reached`]) or that holds a writable-kind key of
/// `startup-data.keys` (`rw_keys`: the page-fault trace saw it read at start-up). Sections already at
/// the head are left as they are.
fn relro_plan(members: &[(String, Member)], text_keys: &BTreeSet<String>, rw_keys: &BTreeSet<String>) -> BTreeMap<String, BTreeSet<String>> {
    let mut chosen = reached(members, text_keys);
    for (mi, (_, m)) in members.iter().enumerate() {
        chosen.extend(m.syms.values().filter(|s| s.ty == "OBJECT" && keyed(&s.name, rw_keys)).filter_map(|s| s.ndx).map(|ndx| (mi, ndx)));
    }
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (mi, (name, m)) in members.iter().enumerate() {
        for (i, s) in &m.secs {
            if s.name.starts_with(".data.rel.ro") && !s.name.starts_with(RELRO_HEAD) && s.flags.contains('W') && chosen.contains(&(mi, *i)) {
                out.entry(name.clone()).or_default().insert(s.name.clone());
            }
        }
    }
    out
}

/// The relro head name of `section` (`.data.rel.ro.X` -> `.data.rel.ro.local.X`).
fn relro_head_name(section: &str) -> String {
    format!("{RELRO_HEAD}{}", &section[".data.rel.ro".len()..])
}

/// The start alignment of each start-up data cluster (D-X.64), as the preprocessor names entry.S
/// reads: the smallest power of two not below the cluster's bytes in `archive` (each input section's
/// size plus its alignment less one: an upper bound for any link), at least one 4 KiB page and at most
/// one 64 KiB fault-around window. A cluster no larger than its alignment never straddles a window
/// boundary (they are multiples of it), and the written cluster takes ceil(size / 4 KiB) pages.
pub fn cluster_aligns(archive: &Path) -> Result<Vec<(&'static str, u64)>, String> {
    let out = Command::new("readelf").arg("-SW").arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf -SW {} failed", archive.display()));
    }
    Ok(cluster_aligns_of(&String::from_utf8_lossy(&out.stdout)))
}

fn cluster_aligns_of(listing: &str) -> Vec<(&'static str, u64)> {
    let mut bytes = [0u64; 3];
    for l in listing.lines() {
        let Some((_, rest)) = l.trim_start().strip_prefix('[').and_then(|r| r.split_once(']')) else { continue };
        let g: Vec<&str> = rest.split_whitespace().collect();
        let (Some(name), Some(size), Some(align)) = (g.first(), g.get(4).and_then(|h| u64::from_str_radix(h, 16).ok()), g.last().and_then(|a| a.parse::<u64>().ok())) else { continue };
        let k = match *name {
            SECTION => 0,
            n if n.starts_with(RELRO_HEAD) => 1,
            crate::coldbss::STATE_SECTION => 2,
            _ => continue,
        };
        bytes[k] += size + align.max(1) - 1;
    }
    let pow2 = |b: u64| b.max(4096).next_power_of_two().min(65536);
    vec![("LIELLA_RODATA1_ALIGN", pow2(bytes[0])), ("LIELLA_RELRO_ALIGN", pow2(bytes[1])), ("LIELLA_DATA1_ALIGN", pow2(bytes[2]))]
}

/// The text keys, read-only keys and parsed members of `archive` (`root`: the tree with the key files).
/// What [`inputs`] returns: text keys, read-only keys, members.
type Inputs = (BTreeSet<String>, BTreeSet<String>, Vec<(String, Member)>);

fn inputs(archive: &Path, root: &Path) -> Result<Inputs, String> {
    let read = |p: &str| fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let mut text_keys = keys(&read("crates/libc-so/startup-text.keys")?, |_| true);
    text_keys.extend(crate::hottext::parse_list(&read("crates/libc-so/hot-text.list")?)?.into_iter().filter(|e| HOT_TIERS.contains(&e.tier)).map(|e| e.key));
    // The fault-bounded probe modes no tier covers (the first localtime_r, + mktime): traced on their own.
    text_keys.extend(keys(&read(PROBE_KEYS)?, |_| true));
    let ro_keys = keys(&read("crates/libc-so/startup-data.keys")?, |k| k == "ro");
    let listing = Command::new("readelf").arg("-SrsW").arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    if !listing.status.success() {
        return Err(format!("readelf -SrsW {} failed", archive.display()));
    }
    let mut members = parse(&String::from_utf8_lossy(&listing.stdout));
    if members.iter().map(|(n, _)| n).collect::<BTreeSet<_>>().len() != members.len() {
        return Err(format!("{}: duplicate member names; cannot extract them safely", archive.display()));
    }
    let full = section_names(&fs::read(archive).map_err(|e| format!("{}: {e}", archive.display()))?).map_err(|e| format!("{}: {e}", archive.display()))?;
    whole_names(&mut members, &full)?;
    Ok((text_keys, ro_keys, members))
}

/// Replace each section name `readelf -SW` printed by the whole name from the object (`full`: member ->
/// names by section index). `readelf` cuts a name at 256 bytes, and `objcopy --rename-section` matches
/// whole names only: a cut name renamed nothing (LAYOUT-STARTUP-RODATA-3: a hot switch table stayed out).
fn whole_names(members: &mut [(String, Member)], full: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    for (name, m) in members.iter_mut() {
        let names = full.get(name).ok_or_else(|| format!("{name}: no section names read from the archive"))?;
        for (i, s) in m.secs.iter_mut() {
            match names.get(*i) {
                Some(f) if f.starts_with(&s.name) => s.name.clone_from(f),
                f => return Err(format!("{name}: section [{i}] is {:?} in the object, `{}` in readelf", f, s.name)),
            }
        }
    }
    Ok(())
}

/// The section names (by index) of every ELF64 little-endian member of the `ar` archive `data`, by
/// member name (GNU long names resolved); other members are left out.
fn section_names(data: &[u8]) -> Result<BTreeMap<String, Vec<String>>, String> {
    if data.get(..8) != Some(b"!<arch>\n".as_slice()) {
        return Err("not an ar archive".into());
    }
    let (mut at, mut long, mut out): (usize, &[u8], _) = (8, &[], BTreeMap::new());
    while at + 60 <= data.len() {
        let h = &data[at..at + 60];
        let size: usize = std::str::from_utf8(&h[48..58]).ok().and_then(|s| s.trim().parse().ok()).ok_or(format!("member header at {at}: bad size"))?;
        let body = data.get(at + 60..at + 60 + size).ok_or(format!("member at {at} runs past the end"))?;
        let raw = String::from_utf8_lossy(&h[..16]);
        let raw = raw.trim_end();
        if raw == "//" {
            long = body;
        } else if raw != "/" && raw != "/SYM64/" {
            let name = match raw.strip_prefix('/').map(str::parse::<usize>) {
                Some(Ok(off)) => {
                    let rest = long.get(off..).ok_or(format!("long name offset {off} past the table"))?;
                    let end = rest.windows(2).position(|w| w == b"/\n").ok_or(format!("long name at {off} not terminated"))?;
                    String::from_utf8_lossy(&rest[..end]).into_owned()
                }
                Some(Err(_)) => return Err(format!("member name `{raw}`")),
                None => raw.strip_suffix('/').unwrap_or(raw).to_string(),
            };
            if let Some(names) = elf_section_names(body) {
                out.insert(name, names);
            }
        }
        at += 60 + size + size % 2;
    }
    Ok(out)
}

/// The section names of an ELF64 little-endian object, by index (`None`: not one, or malformed).
fn elf_section_names(d: &[u8]) -> Option<Vec<String>> {
    let u = |off: usize, n: usize| d.get(off..off.checked_add(n)?).map(|b| b.iter().rev().fold(0u64, |a, &x| (a << 8) | u64::from(x)));
    if d.get(..6) != Some(b"\x7fELF\x02\x01".as_slice()) {
        return None;
    }
    let shoff = usize::try_from(u(0x28, 8)?).ok()?;
    let ent = usize::try_from(u(0x3a, 2)?).ok()?;
    let sh = |i: usize, off: usize, n: usize| u(shoff.checked_add(i.checked_mul(ent)?)?.checked_add(off)?, n).and_then(|v| usize::try_from(v).ok());
    // Over 0xff00 sections: the count is section 0's size and the string table index its link.
    let num = match u(0x3c, 2)? {
        0 if shoff != 0 => sh(0, 0x20, 8)?,
        n => usize::try_from(n).ok()?,
    };
    let strndx = match u(0x3e, 2)? {
        0xffff => sh(0, 0x28, 4)?,
        n => usize::try_from(n).ok()?,
    };
    let (str_off, str_size) = (sh(strndx, 0x18, 8)?, sh(strndx, 0x20, 8)?);
    let strtab = d.get(str_off..str_off.checked_add(str_size)?)?;
    (0..num)
        .map(|i| {
            let rest = strtab.get(sh(i, 0, 4)?..)?;
            Some(String::from_utf8_lossy(&rest[..rest.iter().position(|&c| c == 0)?]).into_owned())
        })
        .collect()
}

/// `cargo xtask startup-ro-plan <archive> [<filter>]`: what [`place`] would do to `archive`, without changing
/// it: the bytes and the largest sections; with `<filter>`, every section whose name contains it, whether it
/// is a start, and its relocation targets (member section, name, chosen or not). A diagnosis aid for the
/// selection (LAYOUT-STARTUP-RODATA-3); on a built `libc.a` the moved sections already read `.rodata1`.
pub fn dump(root: &Path, args: &[String]) -> Result<(), String> {
    let archive = args.first().ok_or("usage: cargo xtask startup-ro-plan <archive> [<section-name filter>]")?;
    let (text_keys, ro_keys, members) = inputs(Path::new(archive), root)?;
    let (rename, bytes, sizes) = plan(&members, &text_keys, &ro_keys);
    println!("startup-ro-plan: {} sections to rename in {} members, at most {bytes} bytes", rename.values().map(BTreeSet::len).sum::<usize>(), rename.len());
    for (n, s) in sizes.iter().take(12) {
        println!("  {n:7} {s}");
    }
    let Some(filter) = args.get(1) else { return Ok(()) };
    let starts: BTreeSet<(usize, usize)> = starts(&members, &text_keys).into_iter().collect();
    for (mi, (name, m)) in members.iter().enumerate() {
        for (i, s) in m.secs.iter().filter(|(_, s)| s.name.contains(filter.as_str())) {
            println!("{name}: [{i}] {} start={} renamed={}", s.name, starts.contains(&(mi, *i)), rename.get(name).is_some_and(|r| r.contains(&s.name)));
            for (_, syms) in m.relocs.iter().filter(|(t, _)| t == i) {
                for at in syms.iter().filter_map(|si| target(&members, mi, *si)) {
                    let t = members[at.0].1.secs.get(&at.1);
                    println!("  -> {}:[{}] {} renamed={}", members[at.0].0, at.1, t.map_or("?", |t| t.name.as_str()), t.is_some_and(|t| t.name == SECTION || rename.get(&members[at.0].0).is_some_and(|r| r.contains(&t.name))));
                }
            }
        }
    }
    Ok(())
}

/// Rename the start-up read-only data sections of `archive` to [`SECTION`] in place (`root`: the tree with
/// the key files; `work`: scratch). Fails when they could exceed [`ALIGN`] bytes. Returns (sections renamed, bytes).
pub fn place(ar: &str, archive: &Path, root: &Path, work: &Path) -> Result<(usize, u64), String> {
    let (text_keys, ro_keys, members) = inputs(archive, root)?;
    let (mut rename, bytes, sizes) = plan(&members, &text_keys, &ro_keys);
    let rw_keys = keys(&fs::read_to_string(root.join("crates/libc-so/startup-data.keys")).map_err(|e| format!("startup-data.keys: {e}"))?, |k| k == "rw");
    let relro = relro_plan(&members, &text_keys, &rw_keys);
    for (m, secs) in &relro {
        rename.entry(m.clone()).or_default().extend(secs.iter().cloned());
    }
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
            let to = if s.starts_with(".data.rel.ro") { relro_head_name(s) } else { SECTION.to_string() };
            cmd.arg("--rename-section").arg(format!("{s}={to}"));
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

    #[test]
    fn cluster_alignment_is_the_smallest_power_of_two_window_safe_and_page_sized() {
        // LAYOUT-DATA-CLUSTERS (D-X.64; no spec clause, layout of the products): each cluster's bytes
        // (size + alignment - 1 per input section) rounded up to a power of two, at least 4 KiB, at most
        // 64 KiB; the relro head counts every `.data.rel.ro.local*` section, `.data1` only itself.
        let l = "  [ 3] .rodata1 PROGBITS 0000000000000000 000040 004e20 00 A 0 0 32\n  [ 4] .rodata1 PROGBITS 0000000000000000 000040 000100 00 A 0 0 1\n  [ 5] .data.rel.ro.local._RNvX PROGBITS 0000000000000000 000040 000010 00 WA 0 0 8\n  [ 6] .data.rel.ro.x PROGBITS 0000000000000000 000040 090000 00 WA 0 0 8\n  [ 7] .data1 PROGBITS 0000000000000000 000040 001001 00 WA 0 0 1\n  [ 8] .data1x PROGBITS 0000000000000000 000040 040000 00 WA 0 0 1\n";
        assert_eq!(cluster_aligns_of(l), vec![("LIELLA_RODATA1_ALIGN", 32768), ("LIELLA_RELRO_ALIGN", 4096), ("LIELLA_DATA1_ALIGN", 8192)]);
        assert_eq!(cluster_aligns_of("  [ 1] .data1 NOBITS 0 0 90000 00 WA 0 0 8\n")[2], ("LIELLA_DATA1_ALIGN", 65536));
        assert_eq!(relro_head_name(".data.rel.ro._RNvFOO"), ".data.rel.ro.local._RNvFOO");
        assert_eq!(relro_head_name(".data.rel.ro"), ".data.rel.ro.local");
    }

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
    fn a_function_section_names_its_function_grouped_or_not() {
        // LAYOUT-STARTUP-RODATA-2 (no spec clause): hot functions are found by section name as the
        // hot-text grouping names them, so a ThinLTO-promoted symbol's `.llvm.<n>` suffix does not hide them.
        assert_eq!(text_function(".text.sorted.h00180.f.llvm.12"), Some("f.llvm.12"));
        assert_eq!(text_function(".text.f"), Some("f"));
        assert_eq!(text_function(".text.sorted.hx.f"), Some("sorted.hx.f"));
        assert_eq!(text_function(".rodata.f"), None);
    }

    /// An ELF64 little-endian object with the given section names (section 1 is the name table).
    fn elf(names: &[&str]) -> Vec<u8> {
        let mut strtab = vec![0u8];
        let mut at = Vec::new();
        for n in names {
            at.push(strtab.len() as u64);
            strtab.extend_from_slice(n.as_bytes());
            strtab.push(0);
        }
        let shoff = (64 + strtab.len()).next_multiple_of(8);
        let mut d = vec![0u8; shoff];
        d[..6].copy_from_slice(b"\x7fELF\x02\x01");
        d[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        d[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        d[0x3c..0x3e].copy_from_slice(&(names.len() as u16).to_le_bytes());
        d[0x3e..0x40].copy_from_slice(&1u16.to_le_bytes());
        d[64..64 + strtab.len()].copy_from_slice(&strtab);
        for (i, a) in at.iter().enumerate() {
            let mut h = [0u8; 64];
            h[..4].copy_from_slice(&(*a as u32).to_le_bytes());
            if i == 1 {
                h[0x18..0x20].copy_from_slice(&64u64.to_le_bytes());
                h[0x20..0x28].copy_from_slice(&(strtab.len() as u64).to_le_bytes());
            }
            d.extend_from_slice(&h);
        }
        d
    }

    #[test]
    fn section_names_are_read_whole_from_the_archive() {
        // LAYOUT-STARTUP-RODATA-3 (no spec clause; layout of the products): `readelf -SW` cuts section names at
        // 256 bytes and objcopy renames whole names only, so the names come from the objects: a 300-byte name
        // whole, a member under a GNU long name, an odd-sized member's padding skipped.
        let long = format!(".rodata..Lswitch.table.{}", "x".repeat(300));
        let (a, b) = (elf(&["", ".shstrtab", &long]), elf(&["", ".shstrtab", ".text.f"]));
        let member = "c-0123456789abcdef.rcgu.o";
        let names = format!("{member}/\n");
        let mut ar = b"!<arch>\n".to_vec();
        for (name, body) in [("//", names.as_bytes()), ("/0", &a), ("b.o/", &[1u8; 3][..]), ("c.o/", &b)] {
            ar.extend_from_slice(format!("{name:<16}{:<32}{:<10}`\n", "", body.len()).as_bytes());
            ar.extend_from_slice(body);
            if body.len() % 2 == 1 {
                ar.push(b'\n');
            }
        }
        let got = section_names(&ar).unwrap();
        assert_eq!(got.keys().map(String::as_str).collect::<Vec<_>>(), ["c-0123456789abcdef.rcgu.o", "c.o"]);
        assert_eq!(got[member][2], long);
        assert_eq!(got["c.o"], ["", ".shstrtab", ".text.f"]);
        // readelf's cut name becomes the whole name; a name that is not a prefix of the object's is an error.
        let mut members = parse(&format!("File: x.a({member})\n  [ 2] {} PROGBITS 0 000040 000010 00 A 0 0 8\n", &long[..256]));
        whole_names(&mut members, &got).unwrap();
        assert_eq!(members[0].1.secs[&2].name, long);
        let mut wrong = parse(&format!("File: x.a({member})\n  [ 2] .rodata.y PROGBITS 0 000040 000010 00 A 0 0 8\n"));
        assert!(whole_names(&mut wrong, &got).is_err());
    }

    #[test]
    fn relocation_sections_with_names_cut_alike_are_told_apart_by_offset() {
        // LAYOUT-STARTUP-RODATA-3 (no spec clause): two relocation sections readelf shows under one cut name
        // belong to their own functions, so only the hot function's constant is chosen.
        let listing = "
File: libc.a(a.o)
  [ 1] .text.cut         PROGBITS        0000000000000000 000040 000020 00  AX  0   0 16
  [ 2] .rela.text.cut    RELA            0000000000000000 000300 000018 18   I  9   1  8
  [ 3] .text.cut         PROGBITS        0000000000000000 000060 000020 00  AX  0   0 16
  [ 4] .rela.text.cut    RELA            0000000000000000 000318 000018 18   I  9   3  8
  [ 5] .rodata.hot       PROGBITS        0000000000000000 000080 000010 00   A  0   0  8
  [ 6] .rodata.cold      PROGBITS        0000000000000000 000090 000010 00   A  0   0  8
Relocation section '.rela.text.cut' at offset 0x318 contains 1 entry:
0000000000000003  0000000200000002 R_X86_64_PC32          0000000000000000 .rodata.cold - 4
Relocation section '.rela.text.cut' at offset 0x300 contains 1 entry:
0000000000000003  0000000100000002 R_X86_64_PC32          0000000000000000 .rodata.hot - 4
Symbol table '.symtab' contains 4 entries:
     1: 0000000000000000     0 SECTION LOCAL  DEFAULT    5 .rodata.hot
     2: 0000000000000000     0 SECTION LOCAL  DEFAULT    6 .rodata.cold
     3: 0000000000000000    32 FUNC    GLOBAL DEFAULT    1 hot_fn
     4: 0000000000000000    32 FUNC    GLOBAL DEFAULT    3 cold_fn
";
        let text: BTreeSet<String> = ["hot_fn".to_string()].into();
        let (rename, ..) = plan(&parse(listing), &text, &BTreeSet::new());
        assert_eq!(rename["a.o"].iter().map(String::as_str).collect::<Vec<_>>(), [".rodata.hot"]);
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
