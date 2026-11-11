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

use std::collections::{BTreeMap, BTreeSet};
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
/// writable start-up key of `crates/libc-so/startup-data.keys` joins too ([`STATE_KEYS_FILE`]), and the
/// start object aligns the cluster to the smallest power of two not below its size
/// (`startup_ro::cluster_aligns`), so it takes the fewest pages wherever `.data` ends.
/// LAYOUT-DATA-CLUSTERS-2 (D-X.64): `heap::HEAP` (5376 bytes, above [`STATE_KEYED_MAX`]), whose head and
/// tail every process writes (its first allocation): in `.lbss` it took two pages whose place rustc's
/// emission order decided (beside `RESERVE_MEM`, never written at start-up); here it costs 5376 file bytes.
/// The rename runs before [`move_cold_bss`], so a keyed object of 1 KiB to 4 KiB (`process::EXIT_HANDLERS`,
/// 1048 bytes) joins too instead of `.lbss`.
/// LAYOUT-DATA-CLUSTERS-3 (D-X.67): `time::zone::NEXT_ID` (4 bytes) and `__tzname` ([`STATE_C_SECTIONS`]),
/// written by the first zone load (localtime_r, tzset): next's `.data` ended with `__tzname`, which shared
/// the cluster's first page; with the cluster aligned it sat alone on a `.data` page (first localtime_r
/// 20 -> 21 faults, fast, perf page-fault map). Here they cost no page of their own.
const STATE_KEY_SUFFIXES: [&str; 10] = [
    "15liella_boundary5timer5EPOLL",
    "15liella_boundary7process10AUXV_WORDS",
    "15liella_boundary3sys12SYSINFO_EHDR",
    "15liella_boundary3cpu14AVX2_ERMS_FROM",
    "15liella_boundary3cpu12AVX2_ERMS_TO",
    "13liella_legacy13global_locale6GLOBAL",
    "13liella_legacy13global_locale4LOCK",
    "13liella_legacy7nscache7NSCACHE",
    "15liella_boundary4heap4HEAP",
    "6liella4time4zone7NEXT_ID",
];
/// Start-up state that C defines, by whole section name (no Rust symbol to key): `__environ`/`environ`
/// (8 bytes, `.bss.environ`), written by every start. Zero-initialised: it becomes 8 zero file bytes.
/// `__tzname`/`tzname` (16 bytes, `.data.rel.local.__tzname`): see [`STATE_KEY_SUFFIXES`] (D-X.67).
const STATE_C_SECTIONS: [&str; 2] = [".bss.environ", ".data.rel.local.__tzname"];
/// See [`STATE_KEY_SUFFIXES`].
pub const STATE_SECTION: &str = ".data1";
/// The trace-derived written start-up objects (kind `rw`; regenerated by `tools/hottext/regen.sh`). A key
/// the archive does not define (another profile's split, an object renamed since the trace) is skipped.
pub const STATE_KEYS_FILE: &str = "crates/libc-so/startup-data.keys";
/// A keyed section larger than this stays where it is: zero-initialised members of the cluster become
/// file bytes, and a large table would spread the cluster over pages start-up does not write.
pub const STATE_KEYED_MAX: u64 = 4096;

/// The writable-kind keys of a `startup-data.keys` text, numeric split suffixes dropped.
pub fn state_keys(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() == 3 && f[1] == "rw").then(|| split_stem(f[0]).to_string())
        })
        .collect()
}

/// `name` without an LLVM split number (`.0`) or ThinLTO suffix (`.llvm.<n>`).
fn split_stem(name: &str) -> &str {
    let name = name.rsplit_once(".llvm.").filter(|(_, n)| n.bytes().all(|b| b.is_ascii_digit())).map_or(name, |(s, _)| s);
    match name.rsplit_once('.') {
        Some((s, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => s,
        _ => name,
    }
}

/// The start-up state sections per archive member ([`STATE_KEY_SUFFIXES`]), from the members'
/// section headers (whole names). Each suffix must name exactly one section: a renamed or removed
/// static is an error, not a silent return to its own page.
fn state_sections(members: &[(String, Vec<Shdr>)], keys: &BTreeSet<String>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen = [0usize; STATE_KEY_SUFFIXES.len()];
    let mut seen_c = [0usize; STATE_C_SECTIONS.len()];
    for (member, name) in members.iter().flat_map(|(m, h)| h.iter().map(move |s| (m, s.name.as_str()))) {
        if let Some(i) = STATE_C_SECTIONS.iter().position(|s| *s == name) {
            seen_c[i] += 1;
            out.entry(member.clone()).or_default().push(name.to_string());
            continue;
        }
        let Some(sym) = name.strip_prefix(".data.").or_else(|| name.strip_prefix(".bss.")) else { continue };
        // LLVM may split a static and number the part (`.data.<symbol>.0`).
        let sym = match sym.rsplit_once('.') {
            Some((s, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !s.ends_with(".llvm") => s,
            _ => sym,
        };
        let k = crate::hottext::key(sym);
        // A split static under ThinLTO is `<symbol>.0.llvm.<n>`: both suffixes go (timer::EPOLL).
        if let Some(i) = STATE_KEY_SUFFIXES.iter().position(|s| split_stem(&k).ends_with(s)) {
            seen[i] += 1;
            out.entry(member.clone()).or_default().push(name.to_string());
        } else if keys.contains(split_stem(&k)) && size_of(members, member, name) <= STATE_KEYED_MAX {
            out.entry(member.clone()).or_default().push(name.to_string());
        }
    }
    let bad: Vec<String> = STATE_KEY_SUFFIXES.iter().zip(seen).chain(STATE_C_SECTIONS.iter().zip(seen_c)).filter(|(_, n)| *n != 1).map(|(s, n)| format!("{s} ({n} sections)")).collect();
    if !bad.is_empty() {
        return Err(format!("start-up state: want one .data/.bss section each, got: {} (renamed? update STATE_KEY_SUFFIXES in xtask/src/coldbss.rs)", bad.join(", ")));
    }
    Ok(out)
}

fn size_of(members: &[(String, Vec<Shdr>)], member: &str, section: &str) -> u64 {
    members.iter().filter(|(m, _)| m == member).flat_map(|(_, h)| h.iter()).filter(|s| s.name == section).map(|s| s.size).sum()
}

/// Rename the start-up state sections of `archive` ([`STATE_KEY_SUFFIXES`] and the written keys of
/// [`STATE_KEYS_FILE`] under `root`) to [`STATE_SECTION`] in place; `work` is scratch. Returns the number renamed.
pub fn move_startup_state(ar: &str, archive: &Path, root: &Path, work: &Path) -> Result<usize, String> {
    let keys = state_keys(&fs::read_to_string(root.join(STATE_KEYS_FILE)).map_err(|e| format!("{STATE_KEYS_FILE}: {e}"))?);
    let state = state_sections(&archive_headers(archive)?, &keys)?;
    let renames = state.into_iter().map(|(m, list)| (m, list.into_iter().map(|s| (s, STATE_SECTION.to_string())).collect())).collect();
    rename(ar, archive, work, "start-up state", &renames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_state_sections_are_found_once_each() {
        // STARTUP-FAULTS-RESTORE (no spec clause; layout of the products): each listed static's
        // section is picked whatever its crate hash and ThinLTO suffix; a missing one is an error.
        let mut members = vec![("a.o".to_string(), Vec::new())];
        for (i, s) in STATE_KEY_SUFFIXES.iter().enumerate() {
            let (kind, tail) = if i % 2 == 0 { (".bss", ".0.llvm.42") } else { (".data", ".0") };
            members.last_mut().unwrap().1.push(sec(&format!("{kind}._RNvNtCsAbC12_{s}{tail}"), SHT_NOBITS, 8));
            if i == 2 {
                members.push(("b.o".to_string(), vec![sec(".data._RNvNtCsX_15liella_boundary3cpu8NOT_USED", 1, 8)]));
            }
        }
        // LAYOUT-DATA-CLUSTERS-2: the C start-up state is picked by its whole section name.
        for c in STATE_C_SECTIONS {
            members.last_mut().unwrap().1.push(sec(c, SHT_NOBITS, 8));
        }
        assert!(state_sections(&members[..1], &BTreeSet::new()).is_err(), "a missing listed section is an error");
        let got = state_sections(&members, &BTreeSet::new()).unwrap();
        assert_eq!((got["a.o"].len(), got["b.o"].len()), (3, STATE_KEY_SUFFIXES.len() - 3 + STATE_C_SECTIONS.len()));
        assert!(got["b.o"].iter().any(|s| s == ".bss.environ"));
        assert!(got["a.o"][0].starts_with(".bss._RNvNtCsAbC12_15liella_boundary5timer5EPOLL"));
        assert!(got["a.o"][1].starts_with(".data._RNvNtCsAbC12_15liella_boundary7process10AUXV_WORDS"));
        for (_, h) in &mut members {
            for s in h.iter_mut() {
                s.name = s.name.replace("6GLOBAL", "6GLOBAX");
            }
        }
        assert!(state_sections(&members, &BTreeSet::new()).unwrap_err().contains("13liella_legacy13global_locale6GLOBAL (0 sections)"));
    }

    #[test]
    fn written_startup_keys_join_the_state_cluster_up_to_a_page() {
        // LAYOUT-DATA-CLUSTERS (D-X.64; no spec clause, layout of the products): a writable key of
        // startup-data.keys joins whatever its split number, crate hash or ThinLTO suffix; a keyed
        // section over STATE_KEYED_MAX, an `ro` key and an unkeyed static stay where they are.
        let keys = state_keys("# c\n_RNvNtC15liella_boundary4heap4KEYD.0\trw\tfast\n_RNvNtC15liella_boundary4heap3BIG\trw\tfast\n_RNvNtC15liella_boundary4heap4ROKY\tro\tfast\n");
        assert_eq!(keys.len(), 2);
        let mut h: Vec<Shdr> = STATE_KEY_SUFFIXES.iter().map(|s| sec(&format!(".data._RNvNtCs1_{s}"), 1, 8)).collect();
        h.push(sec(".bss._RNvNtCsQ9_15liella_boundary4heap4KEYD.llvm.77", SHT_NOBITS, 64));
        h.extend(STATE_C_SECTIONS.iter().map(|c| sec(c, SHT_NOBITS, 8)));
        h.push(sec(".data._RNvNtCsQ9_15liella_boundary4heap3BIG", 1, STATE_KEYED_MAX + 1));
        h.push(sec(".data._RNvNtCsQ9_15liella_boundary4heap4ROKY", 1, 8));
        h.push(sec(".data._RNvNtCsQ9_15liella_boundary4heap5OTHER", 1, 8));
        let got = state_sections(&[("h.o".to_string(), h)], &keys).unwrap();
        assert_eq!(got["h.o"].len(), STATE_KEY_SUFFIXES.len() + STATE_C_SECTIONS.len() + 1);
        assert!(got["h.o"].iter().any(|n| n.contains("4heap4KEYD")));
        assert!(!got["h.o"].iter().any(|n| n.contains("3BIG") || n.contains("ROKY") || n.contains("OTHER")));
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
