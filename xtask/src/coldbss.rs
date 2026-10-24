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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_large_bss_sections_per_member() {
        let listing = "\nFile: libc.a(a.o)\n  [ 3] .text.f           PROGBITS        0000000000000000 000040 000400 00  AX  0   0 16\n  [ 4] .bss.big          NOBITS          0000000000000000 000440 000400 00  WA  0   0  8\n  [ 5] .bss.small        NOBITS          0000000000000000 000440 0003ff 00  WA  0   0  8\n  [ 6] .tbss.t           NOBITS          0000000000000000 000440 001000 00 WAT  0   0  8\n  [ 7] .data.d           PROGBITS        0000000000000000 000440 002000 00  WA  0   0  8\n\nFile: libc.a(b.o)\n  [12] .bss.huge         NOBITS          0000000000000000 000440 010000 00  WA  0   0 64\n";
        let got = cold_sections(listing);
        assert_eq!(got.len(), 2);
        assert_eq!(got["a.o"], vec![".bss.big".to_string()]);
        assert_eq!(got["b.o"], vec![".bss.huge".to_string()]);
    }
}
