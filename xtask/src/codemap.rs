//! `cargo xtask codemap-asm [--check]`: keeps two things in the code map (`docs/codemap/`) true.
//!
//! 1. A symbol exported through `export! { asm "V"; ... }` is defined by a global label in an assembly source under
//!    `csys/src/machine/`, not by the Rust stub inside the macro (the stub only gives Rust callers a typed
//!    declaration). Its "defined at" column in `symbols.tsv` names the `.S` file and the label's line. An asm export
//!    whose label no `.S` file defines is an error, never a guess.
//! 2. No path the code map names (the "defined at" column of `symbols.tsv`, repository paths in `README.md`) points
//!    at a file that no longer exists.
//!
//! Without `--check` the "defined at" column of the asm-export rows is rewritten in place (nothing else in the file
//! changes); with `--check` (run by `cargo xtask static`) a row that still points at the Rust stub is an error. Run it
//! after regenerating the code map.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const TSV: &str = "docs/codemap/symbols.tsv";
const README: &str = "docs/codemap/README.md";
const MACHINE: &str = "csys/src/machine";
/// Repository roots whose paths in `README.md` are checked for existence.
const PATH_ROOTS: [&str; 9] = ["crates/", "csys/", "xtask/", "plan/", "docs/", "spec/", "tests/", "benchmarks/", "validation/"];

/// `(name, "file:line")` of each `export! { asm ... fn name` site in one Rust source.
fn asm_exports(file: &str, text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(rest) = l.trim_start().strip_prefix("export!").map(str::trim_start) else { continue };
        let Some(rest) = rest.strip_prefix('{').map(str::trim_start) else { continue };
        let Some(after) = rest.strip_prefix("asm") else { continue };
        if !(after.is_empty() || after.starts_with([' ', '\t', '"'])) {
            continue;
        }
        // The stub's `fn` follows within the macro's attribute lines.
        let name = lines[i..].iter().take(8).find_map(|l| {
            let at = l.find("fn ")?;
            let id: String = l[at + 3..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            (!id.is_empty()).then_some(id)
        });
        if let Some(name) = name {
            out.push((name, format!("{file}:{}", i + 1)));
        }
    }
    out
}

/// `name -> "file:line"` for each label one assembly source both declares global (`.globl`/`.global`) and defines
/// (`name:` at the start of a line).
fn asm_labels(file: &str, text: &str) -> BTreeMap<String, String> {
    let global: BTreeSet<&str> = text
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            t.strip_prefix(".globl").or_else(|| t.strip_prefix(".global")).filter(|r| r.starts_with([' ', '\t']))
        })
        .flat_map(|r| r.split([',', ' ', '\t']).filter(|s| !s.is_empty()))
        .collect();
    let mut out = BTreeMap::new();
    for (i, l) in text.lines().enumerate() {
        if let Some((name, _)) = l.split_once(':') {
            if global.contains(name) {
                out.entry(name.to_string()).or_insert_with(|| format!("{file}:{}", i + 1));
            }
        }
    }
    out
}

/// The rewritten `symbols.tsv` text and the errors: an asm export without a label.
fn resolve(tsv: &str, exports: &[(String, String)], labels: &BTreeMap<String, String>) -> (String, Vec<String>) {
    let mut errors = Vec::new();
    let mut moved: BTreeMap<&str, &str> = BTreeMap::new();
    for (name, site) in exports {
        match labels.get(name) {
            Some(at) => {
                moved.insert(name, at);
            }
            None => errors.push(format!("{site}: `export! {{ asm` {name}: no global label `{name}` in {MACHINE}/*.S")),
        }
    }
    let mut out = String::with_capacity(tsv.len());
    for (i, line) in tsv.lines().enumerate() {
        let mut cols: Vec<&str> = line.split('\t').collect();
        if i > 0 && cols.len() >= 3 {
            if let Some(at) = moved.get(cols[0]) {
                cols[2] = at;
            }
        }
        out.push_str(&cols.join("\t"));
        out.push('\n');
    }
    (out, errors)
}

/// Paths the code map names that do not exist under `root`.
fn dead_paths(root: &Path, tsv: &str, readme: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, line) in tsv.lines().enumerate().skip(1) {
        let Some(at) = line.split('\t').nth(2) else { continue };
        let path = at.rsplit_once(':').filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit())).map_or(at, |(p, _)| p);
        if !path.is_empty() && !root.join(path).exists() {
            errors.push(format!("{TSV}:{}: {path} does not exist", i + 1));
        }
    }
    for (i, line) in readme.lines().enumerate() {
        for tok in line.split(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c))) {
            let tok = tok.trim_end_matches('.');
            if PATH_ROOTS.iter().any(|r| tok.starts_with(r)) && !root.join(tok).exists() {
                errors.push(format!("{README}:{}: {tok} does not exist", i + 1));
            }
        }
    }
    errors
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                walk(&p, ext, out);
            }
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let mut rs = Vec::new();
    walk(&root.join("crates"), "rs", &mut rs);
    rs.sort();
    let mut exports = Vec::new();
    for p in &rs {
        let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if text.contains("export!") {
            exports.extend(asm_exports(&rel(root, p), &text));
        }
    }
    let mut ss = Vec::new();
    walk(&root.join(MACHINE), "S", &mut ss);
    ss.sort();
    let mut labels = BTreeMap::new();
    for p in &ss {
        let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        for (k, v) in asm_labels(&rel(root, p), &text) {
            labels.entry(k).or_insert(v);
        }
    }
    let tsv = fs::read_to_string(root.join(TSV)).map_err(|e| format!("{TSV}: {e}"))?;
    let readme = fs::read_to_string(root.join(README)).unwrap_or_default();
    let (fixed, mut errors) = resolve(&tsv, &exports, &labels);
    let stale = tsv.lines().zip(fixed.lines()).filter(|(a, b)| a != b).count();
    if stale > 0 {
        if check {
            errors.push(format!("{TSV}: {stale} asm-export row(s) do not name their .S label; run `cargo xtask codemap-asm`"));
        } else {
            fs::write(root.join(TSV), &fixed).map_err(|e| format!("{TSV}: {e}"))?;
            println!("codemap-asm: {stale} row(s) of {TSV} now name their .S label");
        }
    }
    errors.extend(dead_paths(root, if check { &tsv } else { &fixed }, &readme));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{} problem(s):\n  {}", errors.len(), errors.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    // no spec clause: code map tooling (CODEMAP-ASM-EXPORTS).
    use super::*;

    const RS: &str = "export! { asm \"GLIBC_2.2.5\";\npub extern \"C\" fn asm_fn(x: usize) -> usize {\n    x\n}}\n\n\
                      export! { \"GLIBC_2.2.5\";\npub extern \"C\" fn rust_fn() {}}\n\n\
                      export! { asm \"GLIBC_2.34\";\n#[allow(unused)]\npub extern \"C\" fn lost_fn() {}}\n";
    const S: &str = "\t.text\n\t.globl asm_fn\n\t.type asm_fn, @function\nasm_fn:\n\tret\nlocal_fn:\n\tret\n";
    const TSV_IN: &str = "symbol\tversion\tdefined at\tfirst implementation call\n\
                          asm_fn\tGLIBC_2.2.5\tcrates/x/src/a.rs:2\tcore::f\n\
                          rust_fn\tGLIBC_2.2.5\tcrates/x/src/a.rs:7\t\n";

    fn fixture(tag: &str, rs: &str, readme: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-codemap-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (p, t) in [("crates/x/src/a.rs", rs), ("csys/src/machine/a.S", S), (TSV, TSV_IN), (README, readme)] {
            fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
            fs::write(root.join(p), t).unwrap();
        }
        root
    }

    #[test]
    fn asm_export_moves_to_its_label_and_rust_export_stays() {
        let rs = RS.replace("export! { asm \"GLIBC_2.34\";", "export! { \"GLIBC_2.34\";");
        let root = fixture("ok", &rs, "| x | crates/x/src/a.rs:1 |\n");
        let err = run(&root, true).unwrap_err();
        assert!(err.contains("1 asm-export row(s)"), "{err}");
        run(&root, false).unwrap();
        let tsv = fs::read_to_string(root.join(TSV)).unwrap();
        assert!(tsv.contains("asm_fn\tGLIBC_2.2.5\tcsys/src/machine/a.S:4\tcore::f\n"), "{tsv}");
        assert!(tsv.contains("rust_fn\tGLIBC_2.2.5\tcrates/x/src/a.rs:7\t\n"), "{tsv}");
        run(&root, true).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn asm_export_without_label_is_an_error() {
        let root = fixture("lost", RS, "");
        let err = run(&root, false).unwrap_err();
        assert!(err.contains("crates/x/src/a.rs:9: `export! { asm` lost_fn: no global label"), "{err}");
        assert!(!err.contains("asm_fn"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dead_paths_are_errors() {
        let root = fixture("dead", RS, "| `/etc/hosts.equiv` | crates/x/src/gone.rs:541 |\n| ok | crates/x/src/a.rs |\n");
        let tsv = format!("{TSV_IN}gone\tGLIBC_2.2.5\tcrates/x/src/gone2.rs:3\t\n");
        let errs = dead_paths(&root, &tsv, &fs::read_to_string(root.join(README)).unwrap());
        assert_eq!(errs, [format!("{TSV}:4: crates/x/src/gone2.rs does not exist"), format!("{README}:1: crates/x/src/gone.rs does not exist")]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn labels_need_both_globl_and_definition() {
        let l = asm_labels("m.S", ".globl a, b\n.global c\na:\nc:\nd:\n\t.globl e\n");
        assert_eq!(l.into_iter().collect::<Vec<_>>(), [("a".into(), "m.S:3".into()), ("c".into(), "m.S:4".into())]);
    }
}
