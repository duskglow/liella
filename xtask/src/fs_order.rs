//! `fs-order` (static): test code must not depend on directory order.
//!
//! A directory listing comes back in an order that belongs to the file system (ext4 hashes, xfs and btrfs
//! differ, tmpfs is newest-first), so a test that records or compares one without ordering it passes on one
//! machine and fails on another (owner rule "no file-system dependence", 2026-10-21).
//!
//! The check, per test source (`crates/*/tests`): each listing source in code (comments and string literals
//! are stripped first) is one of [`SOURCES`]: `read_dir(`, `readdir(`, `readdir64(`, `scandir(`, `scandir64(`,
//! `fts_read(`, `ftw(`, `nftw(` and the flag `GLOB_NOSORT`. A definition or declaration (`fn readdir(`) is not
//! a use. A use is clean when the function that contains it also orders the listing or reduces it to
//! something order-free: one of [`ORDERED`] (a sort, `alphasort`/`versionsort`, a `BTreeSet`/`BTreeMap`/
//! `HashSet`, a plain `.count()`). A deliberate case carries `// FS-ORDER: <reason>` on its line or on one of
//! the two lines above it or above the enclosing function's `fn` line (which covers the whole function). A token-level tripwire, like the other policy checks: a sort in a helper the function
//! calls is not followed (put the marker there).

use crate::policy::{report, rust_files, strip};
use std::fs;
use std::path::Path;

/// Tokens that produce a directory-order listing.
const SOURCES: &[&str] = &["read_dir(", "readdir(", "readdir64(", "scandir(", "scandir64(", "fts_read(", "nftw(", "nftw64(", "ftw(", "ftw64(", "GLOB_NOSORT"];

/// Tokens that order a listing or make its order irrelevant.
const ORDERED: &[&str] = &[".sort", "sort_by", "sort_unstable", "alphasort", "versionsort", "BTreeSet", "BTreeMap", "HashSet", ".count()", ".is_empty()"];

/// The waiver marker.
const WAIVER: &str = "FS-ORDER:";

fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// For each `fn` item with a body in a stripped source: where `fn` stands and the byte range of the body.
fn fn_bodies(s: &str) -> Vec<(usize, usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = s[i..].find("fn ") {
        let at = i + p;
        i = at + 3;
        if at > 0 && ident(b[at - 1]) {
            continue;
        }
        let mut depth = 0i32;
        let mut j = i;
        while j < b.len() {
            match b[j] {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                b';' if depth <= 0 => break,
                b'{' if depth <= 0 => {
                    let mut d = 0i32;
                    let mut k = j;
                    while k < b.len() {
                        match b[k] {
                            b'{' => d += 1,
                            b'}' => {
                                d -= 1;
                                if d == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    out.push((at, j, k.min(b.len() - 1) + 1));
                    break;
                }
                _ => {}
            }
            j += 1;
        }
    }
    out
}

/// The violations in one source (`rel` names it in the messages).
pub(crate) fn check_file(rel: &str, src: &str) -> Vec<String> {
    let s = strip(src);
    let orig: Vec<&str> = src.lines().collect();
    let bodies = fn_bodies(&s);
    let b = s.as_bytes();
    let mut errors = Vec::new();
    for tok in SOURCES {
        let mut from = 0;
        while let Some(p) = s[from..].find(tok) {
            let at = from + p;
            from = at + tok.len();
            if at > 0 && ident(b[at - 1]) {
                continue;
            }
            if s[..at].trim_end().ends_with("fn") {
                continue;
            }
            // A call whose result is only tested for NULL (`readdir(d).is_null()`) records no order.
            if let Some(open) = s[at..].find('(').map(|o| at + o).filter(|_| tok.ends_with('(')) {
                let mut d = 0i32;
                let close = (open..b.len()).find(|&i| {
                    d += match b[i] {
                        b'(' => 1,
                        b')' => -1,
                        _ => 0,
                    };
                    d == 0
                });
                if close.is_some_and(|c| s[c + 1..].starts_with(".is_null()")) {
                    continue;
                }
            }
            let line = s[..at].bytes().filter(|&c| c == b'\n').count();
            let scope = bodies.iter().filter(|r| r.1 <= at && at < r.2).min_by_key(|r| r.2 - r.1);
            let fn_line = scope.map_or(line, |r| s[..r.0].bytes().filter(|&c| c == b'\n').count());
            let waived = |l: usize| (l.saturating_sub(2)..=l).any(|l| orig.get(l).is_some_and(|t| t.contains(WAIVER)));
            if waived(line) || waived(fn_line) {
                continue;
            }
            let text = scope.map_or("", |r| &s[r.1..r.2]);
            if ORDERED.iter().any(|o| text.contains(o)) {
                continue;
            }
            errors.push(format!(
                "{rel}:{}: `{}` lists a directory in file-system order and the function (line {}) neither sorts it nor compares it as a set - sort it, or `// {WAIVER} <why>` on the call or above the function",
                line + 1,
                tok.trim_end_matches('('),
                fn_line + 1
            ));
        }
    }
    errors.sort();
    errors
}

pub fn fs_order(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for e in fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?.flatten() {
        rust_files(&e.path().join("tests"), &mut files);
    }
    files.sort();
    let mut errors = Vec::new();
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else { continue };
        errors.extend(check_file(&f.strip_prefix(root).unwrap_or(f).display().to_string(), &src));
    }
    report("fs-order", files.len(), errors)
}

#[cfg(test)]
mod tests {
    use super::check_file;

    #[test]
    fn an_unsorted_listing_fails() {
        let bad = "fn names() -> Vec<String> { std::fs::read_dir(\".\").unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect() }\n";
        let e = check_file("x.rs", bad);
        assert!(e.len() == 1 && e[0].contains("x.rs:1:") && e[0].contains("read_dir"), "{e:?}");
        let glob = "fn t() { let r = glob(p, GLOB_NOSORT, None, &mut g); assert_eq!(names(&g), [\"a\", \"b\"]); }\n";
        assert_eq!(check_file("x.rs", glob).len(), 1);
    }

    #[test]
    fn sorted_set_waived_and_mentioned_listings_pass() {
        let sorted = "fn names() -> Vec<String> { let mut v: Vec<String> = std::fs::read_dir(\".\").unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect(); v.sort(); v }\n";
        assert!(check_file("x.rs", sorted).is_empty());
        let set = "fn t() { let s: BTreeSet<_> = std::fs::read_dir(\".\").unwrap().collect(); }\n";
        assert!(check_file("x.rs", set).is_empty());
        let count = "fn n() -> usize { std::fs::read_dir(\"/proc/self/fd\").unwrap().count() }\n";
        assert!(check_file("x.rs", count).is_empty());
        let waived = "fn t() {\n    // FS-ORDER: the test wants directory order itself\n    let r = glob(p, GLOB_NOSORT, None, &mut g);\n}\n";
        assert!(check_file("x.rs", waived).is_empty());
        let fn_waived = "// FS-ORDER: counts entries only\nfn t() {\n    let a = 1;\n    while !x::readdir(d).is_null() {}\n    let e = x::readdir(d);\n}\n";
        assert!(check_file("x.rs", fn_waived).is_empty());
        let null_only = "fn t() { assert!(x::readdir(d).is_null()); }\nfn u() { let e = x::readdir(d); }\n";
        assert_eq!(check_file("x.rs", null_only).len(), 1);
        // Comments, string literals and declarations are not uses.
        let text = "// read_dir( is mentioned\nextern \"C\" { fn readdir(d: *mut D) -> *mut E; }\nfn t() { let s = \"scandir(\"; }\n";
        assert!(check_file("x.rs", text).is_empty());
    }
}
