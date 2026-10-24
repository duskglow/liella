//! `cargo xtask symgap`: the names glibc's static archive defines that Liella's `libc.a` does
//! not (owner suggestion 2026-10-05, D-X.14). The reference is the symbol table of the installed
//! `/usr/lib/x86_64-linux-gnu/libc.a` (interface data, OD-1), restricted to the exported ABI
//! (`spec/symbols.tsv`); nothing else of it is read. A missing name is reported as a gap, or as
//! later work when a later phase owns it: §27 (dynamic linking, phase 14), §29 (legacy, phase
//! 15), §28 (profiling, phase 15), the libio internals of §12.13 (the vtables and their
//! operations, phase 15, F32: [`F32_LIBIO`]), or a name `libc.so.6` has only as a compatibility
//! version (phase 15). The command reports and never fails: the gaps are a work list, not a
//! regression.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::symcov::F32_LIBIO;

const REFERENCE: &str = "/usr/lib/x86_64-linux-gnu/libc.a";


/// Defined global and weak names of an archive, by `nm`.
fn defined(path: &Path) -> Result<BTreeSet<String>, String> {
    let out = Command::new("nm").args(["--defined-only", "-P"]).arg(path).output().map_err(|e| format!("nm: {e}"))?;
    if !out.status.success() {
        return Err(format!("nm {} failed", path.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (name, ty) = (f.next()?, f.next()?);
            // Upper case: global; W, V: weak; i: indirect function.
            (ty.chars().next().is_some_and(|c| c.is_ascii_uppercase()) || ty == "i").then(|| name.to_string())
        })
        .collect())
}

pub fn run(root: &Path) -> Result<(), String> {
    let reference = Path::new(REFERENCE);
    if !reference.exists() {
        println!("symgap: skipped ({REFERENCE} is not installed)");
        return Ok(());
    }
    let ours = root.join("target/liella/libc.a");
    if !ours.exists() {
        return Err("target/liella/libc.a is missing: run `cargo xtask so`".into());
    }
    let table = fs::read_to_string(root.join("spec/symbols.tsv")).map_err(|e| format!("symbols.tsv: {e}"))?;
    let sections: BTreeMap<String, u32> = table.lines().skip(1).filter_map(|l| {
        let mut f = l.split('\t');
        let (name, sec) = (f.next()?, f.next()?);
        Some((name.to_string(), sec.parse().ok()?))
    }).collect();
    let versions = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let default: BTreeSet<String> = versions.lines().skip(1).filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        (f.first() == Some(&"libc.so.6") && f.get(3) == Some(&"default")).then(|| f.get(1).map(|s| s.to_string())).flatten()
    }).collect();
    let (theirs, mine) = (defined(reference)?, defined(&ours)?);
    let mut gaps: BTreeMap<u32, Vec<&String>> = BTreeMap::new();
    let f32: BTreeSet<&str> = F32_LIBIO.into_iter().collect();
    let mut later: BTreeMap<&str, usize> = BTreeMap::new();
    for name in theirs.iter().filter(|n| !mine.contains(*n)) {
        let Some(&sec) = sections.get(name) else { continue };
        let owner = match sec {
            27 => Some("§27"),
            29 => Some("§29"),
            28 => Some("§28"),
            12 if f32.contains(name.as_str()) => Some("§12.13 F32"),
            _ if !default.contains(name) => Some("compat"),
            _ => None,
        };
        if let Some(o) = owner {
            *later.entry(o).or_default() += 1;
        } else {
            gaps.entry(sec).or_default().push(name);
        }
    }
    let total: usize = gaps.values().map(Vec::len).sum();
    let later_total: usize = later.values().sum();
    let parts: Vec<String> = later.iter().map(|(o, n)| format!("{o} {n}")).collect();
    println!("symgap: {total} names glibc's libc.a defines and Liella's does not (in the ABI, owned by phases up to 13); {later_total} more owned by phases 14 and 15 ({})", parts.join(", "));
    for (sec, names) in &gaps {
        let shown: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        println!("  §{sec:<2} {:>4}: {}", names.len(), shown.join(" "));
    }
    Ok(())
}
