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
/// `work` (scratch), and check every rename took effect. Returns the number renamed.
fn rename(ar: &str, archive: &Path, work: &Path, what: &str, renames: &BTreeMap<String, Vec<(String, String)>>) -> Result<usize, String> {
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
    rename(ar, archive, work, "coldbss", &renames)
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

/// The entries of a `startup-state.list` text: (entry, the line as written, may be absent: `fast-only`).
fn declared(text: &str) -> Result<Vec<(Declared, String, bool)>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
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
        out.push((d, f.join(" "), optional));
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
fn state_sections(members: &[(String, Vec<Shdr>)], list: &[(Declared, String, bool)]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
        let hit = list.iter().position(|(d, _, _)| match d {
            Declared::Section(n) => n == name,
            Declared::Symbol(n) => raw == Some(n.as_str()),
            Declared::Path(p) => k.as_deref().is_some_and(|k| split_stem(k).ends_with(p.as_str())),
        });
        let Some(i) = hit else { continue };
        seen[i] += 1;
        if s.size > STATE_DECLARED_MAX {
            big.push(format!("{name} ({} bytes)", s.size));
        }
        out.entry(member.clone()).or_default().push(name.to_string());
    }
    let missing: Vec<&str> = list.iter().zip(&seen).filter(|((_, _, optional), n)| !optional && **n == 0).map(|((_, l, _), _)| l.as_str()).collect();
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
    let state = state_sections(&archive_headers(archive)?, &list)?;
    let renames = state.into_iter().map(|(m, list)| (m, list.into_iter().map(|s| (s, STATE_SECTION.to_string())).collect())).collect();
    rename(ar, archive, work, "start-up state", &renames)
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
            ("a.o".to_string(), vec![sec(".bss._RNvNtCsAbC12_15liella_boundary4heap4HEAP", SHT_NOBITS, 5376), sec(".data._RNvNtCsQ9_15liella_boundary5timer5EPOLL.0.llvm.42", 1, 8), sec(".data._RNvNtCsQ9_15liella_boundary5timer5EPOLL.1", 1, 8)]),
            ("b.o".to_string(), vec![sec(".bss.__rseq_size", SHT_NOBITS, 4), sec(".bss.environ", SHT_NOBITS, 8), sec(".data._RNvNtCsQ9_15liella_boundary4heap5OTHER", 1, 8), sec(".bss._RNvNtCsQ9_15liella_boundary5xheap4HEAP", SHT_NOBITS, 8)]),
        ];
        let got = state_sections(&members, &list).unwrap();
        assert_eq!((got["a.o"].len(), got["b.o"].len()), (3, 2));
        assert!(!got["b.o"].iter().any(|n| n.contains("OTHER") || n.contains("xheap")));
        assert!(state_sections(&members[1..], &list).unwrap_err().contains("for liella_boundary::heap::HEAP, liella_boundary::timer::EPOLL ("));
        members[0].1[0].size = STATE_DECLARED_MAX + 1;
        assert!(state_sections(&members, &list).unwrap_err().contains("over 8192 bytes"));
        assert!(declared("liella_boundary::_x::Y\n").is_err() && declared("foo bar\n").is_err());
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
