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

use crate::run;

/// Sections from this size on are cold tables (measured on coreutils `printf` in en_US.UTF-8:
/// 4096 saved 5 of 119 faults per process, 1024 saved 9, 256 saved 7: below 1 KiB the moved
/// sections are the small state words themselves).
pub const MIN: u64 = 1024;

/// The `.bss.*` sections of at least [`MIN`] bytes per archive member, from `readelf -SW`.
fn cold_sections(listing: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut member = None;
    for line in listing.lines() {
        if let Some(rest) = line.strip_prefix("File: ") {
            // "File: <archive>(<member>)"
            member = rest.rfind('(').map(|i| rest[i + 1..].trim_end_matches(')').to_string());
            continue;
        }
        // "  [ n] <name> NOBITS <addr> <off> <size> ..."; the index may contain a space.
        let Some(after) = line.split_once(']').map(|(_, a)| a) else { continue };
        let f: Vec<&str> = after.split_whitespace().collect();
        let [name, "NOBITS", _, _, size, ..] = f.as_slice() else { continue };
        if !name.starts_with(".bss.") {
            continue;
        }
        let Ok(size) = u64::from_str_radix(size, 16) else { continue };
        match &member {
            Some(m) if size >= MIN => out.entry(m.clone()).or_default().push((*name).to_string()),
            _ => {}
        }
    }
    out
}

/// Rename the cold `.bss.*` sections of `archive` to `.lbss.*` in place; `work` is scratch.
/// Returns the number of sections renamed.
pub fn move_cold_bss(ar: &str, archive: &Path, work: &Path) -> Result<usize, String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let listing = Command::new("readelf").arg("-SW").arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    if !listing.status.success() {
        return Err(format!("readelf -SW {} failed", archive.display()));
    }
    let cold = cold_sections(&String::from_utf8_lossy(&listing.stdout));
    if cold.is_empty() {
        return Ok(0);
    }
    // Members are extracted and replaced by name: a duplicated name would be ambiguous.
    let names = Command::new(ar).arg("t").arg(archive).output().map_err(|e| format!("ar: {e}"))?;
    let names = String::from_utf8_lossy(&names.stdout).into_owned();
    for m in cold.keys() {
        if names.lines().filter(|l| l == m).count() != 1 {
            return Err(format!("coldbss: archive member {m} is not unique in {}", archive.display()));
        }
    }
    let members: Vec<&String> = cold.keys().collect();
    run(Command::new(ar).current_dir(work).arg("x").arg(archive).args(&members))?;
    let mut n = 0;
    for (m, sections) in &cold {
        let mut cmd = Command::new("objcopy");
        for s in sections {
            cmd.arg("--rename-section").arg(format!("{s}=.lbss{}", &s[".bss".len()..]));
            n += 1;
        }
        run(cmd.arg(work.join(m)))?;
    }
    run(Command::new(ar).current_dir(work).arg("r").arg(archive).args(&members))?;
    Ok(n)
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
const STATE_KEY_SUFFIXES: [&str; 6] = [
    "15liella_boundary7process10AUXV_WORDS",
    "15liella_boundary3sys12SYSINFO_EHDR",
    "15liella_boundary3cpu14AVX2_ERMS_FROM",
    "15liella_boundary3cpu12AVX2_ERMS_TO",
    "13liella_legacy13global_locale6GLOBAL",
    "13liella_legacy13global_locale4LOCK",
];
/// See [`STATE_KEY_SUFFIXES`].
pub const STATE_SECTION: &str = ".data1";

/// The start-up state sections per archive member ([`STATE_KEY_SUFFIXES`]), from `readelf -SW`.
/// Each suffix must name exactly one section: a renamed or removed static is an error, not a
/// silent return to its own page.
fn state_sections(listing: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen = [0usize; STATE_KEY_SUFFIXES.len()];
    let mut member = String::new();
    for l in listing.lines() {
        if let Some(m) = l.strip_prefix("File: ").and_then(|f| f.rsplit_once('(')).and_then(|(_, m)| m.strip_suffix(')')) {
            member = m.to_string();
            continue;
        }
        let Some(name) = l.split_once("] ").and_then(|(_, r)| r.split_whitespace().next()) else { continue };
        let Some(sym) = name.strip_prefix(".data.").or_else(|| name.strip_prefix(".bss.")) else { continue };
        // LLVM may split a static and number the part (`.data.<symbol>.0`).
        let sym = match sym.rsplit_once('.') {
            Some((s, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !s.ends_with(".llvm") => s,
            _ => sym,
        };
        let k = crate::hottext::key(sym);
        if let Some(i) = STATE_KEY_SUFFIXES.iter().position(|s| k.ends_with(s)) {
            seen[i] += 1;
            out.entry(member.clone()).or_default().push(name.to_string());
        }
    }
    let bad: Vec<String> = STATE_KEY_SUFFIXES.iter().zip(seen).filter(|(_, n)| *n != 1).map(|(s, n)| format!("{s} ({n} sections)")).collect();
    if !bad.is_empty() {
        return Err(format!("start-up state: want one .data/.bss section each, got: {} (renamed? update STATE_KEY_SUFFIXES in xtask/src/coldbss.rs)", bad.join(", ")));
    }
    Ok(out)
}

/// Rename the start-up state sections of `archive` ([`STATE_KEY_SUFFIXES`]) to
/// [`STATE_SECTION`] in place; `work` is scratch. Returns the number renamed.
pub fn move_startup_state(ar: &str, archive: &Path, work: &Path) -> Result<usize, String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let listing = Command::new("readelf").arg("-SW").arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    if !listing.status.success() {
        return Err(format!("readelf -SW {} failed", archive.display()));
    }
    let state = state_sections(&String::from_utf8_lossy(&listing.stdout))?;
    let names = Command::new(ar).arg("t").arg(archive).output().map_err(|e| format!("ar: {e}"))?;
    let names = String::from_utf8_lossy(&names.stdout).into_owned();
    for m in state.keys() {
        if names.lines().filter(|l| l == m).count() != 1 {
            return Err(format!("start-up state: archive member {m} is not unique in {}", archive.display()));
        }
    }
    let members: Vec<&String> = state.keys().collect();
    run(Command::new(ar).current_dir(work).arg("x").arg(archive).args(&members))?;
    let mut n = 0;
    for (m, sections) in &state {
        let mut cmd = Command::new("objcopy");
        for s in sections {
            cmd.arg("--rename-section").arg(format!("{s}={STATE_SECTION}"));
            n += 1;
        }
        run(cmd.arg(work.join(m)))?;
    }
    run(Command::new(ar).current_dir(work).arg("r").arg(archive).args(&members))?;
    let _ = fs::remove_dir_all(work);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_state_sections_are_found_once_each() {
        // STARTUP-FAULTS-RESTORE (no spec clause; layout of the products): each listed static's
        // section is picked whatever its crate hash and ThinLTO suffix; a missing one is an error.
        let mut listing = String::from("\nFile: libc.a(a.o)\n");
        for (i, s) in STATE_KEY_SUFFIXES.iter().enumerate() {
            let (kind, tail) = if i % 2 == 0 { (".bss", ".llvm.42") } else { (".data", ".0") };
            listing.push_str(&format!("  [{i}] {kind}._RNvNtCsAbC12_{s}{tail}  NOBITS 0 0 8 00 WA 0 0 8\n"));
            if i == 2 {
                listing.push_str("\nFile: libc.a(b.o)\n  [9] .data._RNvNtCsX_15liella_boundary3cpu8NOT_USED PROGBITS 0 0 8 00 WA 0 0 8\n");
            }
        }
        let got = state_sections(&listing).unwrap();
        assert_eq!((got["a.o"].len(), got["b.o"].len()), (3, STATE_KEY_SUFFIXES.len() - 3));
        assert!(got["a.o"][0].starts_with(".bss._RNvNtCsAbC12_15liella_boundary7process10AUXV_WORDS"));
        let short = listing.replace("6GLOBAL", "6GLOBAX");
        assert!(state_sections(&short).unwrap_err().contains("13liella_legacy13global_locale6GLOBAL (0 sections)"));
    }

    #[test]
    fn picks_large_bss_sections_per_member() {
        let listing = "\nFile: libc.a(a.o)\n  [ 3] .text.f           PROGBITS        0000000000000000 000040 000400 00  AX  0   0 16\n  [ 4] .bss.big          NOBITS          0000000000000000 000440 000400 00  WA  0   0  8\n  [ 5] .bss.small        NOBITS          0000000000000000 000440 0003ff 00  WA  0   0  8\n  [ 6] .tbss.t           NOBITS          0000000000000000 000440 001000 00 WAT  0   0  8\n  [ 7] .data.d           PROGBITS        0000000000000000 000440 002000 00  WA  0   0  8\n\nFile: libc.a(b.o)\n  [12] .bss.huge         NOBITS          0000000000000000 000440 010000 00  WA  0   0 64\n";
        let got = cold_sections(listing);
        assert_eq!(got.len(), 2);
        assert_eq!(got["a.o"], vec![".bss.big".to_string()]);
        assert_eq!(got["b.o"], vec![".bss.huge".to_string()]);
    }
}
