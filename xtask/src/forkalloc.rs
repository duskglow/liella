//! `fork-child-alloc` (static): no heap allocation in the body of a raw-fork test child.
//!
//! `selftest::in_child` / `in_child_quiet` / `spawn_child` fork without the host C library's after-fork
//! resets, and `in_library_child` forks through Liella's `fork`, which does not reset the host allocator
//! either: the child is a copy of the process with only the forking thread. When another test thread held
//! the allocator's lock at the fork, an allocation in the child waits forever and the gate hangs
//! (MEMCMP-EQ-24-32-2: 45 minutes). A child body must work on memory made before the fork (statics, stack
//! arrays, buffers the parent built).
//!
//! The check, per test source: the raw-fork helpers are the selftest ones (unless the file defines its own
//! function of that name) and every local function that passes its argument on to one (`aborts`). For each
//! call of a helper, every closure argument and every argument naming a local function is a child body; the
//! body, and transitively each local function it calls, must contain none of [`ALLOCS`]. A deliberate
//! allocation carries `// fork-alloc-ok: <why>` on its line or the line above. A token-level tripwire, like
//! the other policy checks: library calls that allocate internally are not followed.

use crate::policy::{report, rust_files, strip};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// The helpers of `liella_boundary::selftest` that run their `fn` argument in a forked child.
const HELPERS: &[&str] = &["in_child", "in_child_quiet", "spawn_child", "in_library_child"];

/// Tokens that allocate on the heap (the print macros write into libtest's capture buffer, a `Vec`).
const ALLOCS: &[&str] = &[
    "vec!", "format!", "String::", "Vec::", "VecDeque::", "Box::", "Rc::new", "Arc::new", "CString::", "OsString::", "PathBuf::", "HashMap::", "HashSet::", "BTreeMap::",
    "BTreeSet::", ".to_vec(", ".to_string(", ".to_owned(", ".into_owned(", ".to_path_buf(", ".collect", ".repeat(", ".concat(", ".join(", "print!", "println!",
    "eprint!", "eprintln!", "env::var(", "Command::new", "read_to_string(", "fs::read(", "read_dir(",
];

/// The waiver marker.
const WAIVER: &str = "fork-alloc-ok:";

/// One `fn` definition in a stripped source.
struct Def {
    name: String,
    /// Byte range of the body, braces included.
    body: (usize, usize),
    /// Byte range of the innermost enclosing function body (the whole file at top level).
    scope: (usize, usize),
}

fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The byte offset of the bracket closing the one opened at `open`.
fn close_of(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    b.len()
}

fn defs(s: &str) -> Vec<Def> {
    let b = s.as_bytes();
    let mut out: Vec<Def> = Vec::new();
    let mut from = 0;
    while let Some(p) = s[from..].find("fn ") {
        let at = from + p;
        from = at + 3;
        if at > 0 && ident(b[at - 1]) {
            continue;
        }
        let name: String = s[at + 3..].bytes().take_while(|&c| ident(c)).map(char::from).collect();
        if name.is_empty() {
            continue;
        }
        let rest = &s[at..];
        let (Some(brace), semi) = (rest.find('{'), rest.find(';')) else { continue };
        if semi.is_some_and(|x| x < brace) {
            continue;
        }
        let open = at + brace;
        out.push(Def { name, body: (open, close_of(b, open)), scope: (0, b.len()) });
    }
    let spans: Vec<(usize, usize)> = out.iter().map(|d| d.body).collect();
    for d in &mut out {
        if let Some(sc) = spans.iter().filter(|sp| sp.0 < d.body.0 && d.body.1 <= sp.1).min_by_key(|sp| sp.1 - sp.0) {
            d.scope = *sc;
        }
    }
    out
}

/// The local definitions a bare call of `name` at `pos` can mean: those in the innermost scope holding `pos`.
fn resolve(defs: &[Def], name: &str, pos: usize) -> Vec<usize> {
    let seen: Vec<usize> = (0..defs.len()).filter(|&i| defs[i].name == name && defs[i].scope.0 <= pos && pos < defs[i].scope.1).collect();
    let Some(min) = seen.iter().map(|&i| defs[i].scope.1 - defs[i].scope.0).min() else { return Vec::new() };
    seen.into_iter().filter(|&i| defs[i].scope.1 - defs[i].scope.0 == min).collect()
}

/// `(name, offset of the name, offset of its '(')` for each plain call in `s[lo..hi]`: not a method, not a
/// macro, not a definition; a path call keeps only its last segment and is marked by `path`.
fn calls(s: &str, lo: usize, hi: usize) -> Vec<(String, usize, usize, bool)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = lo;
    while i < hi {
        if ident(b[i]) && (i == 0 || !ident(b[i - 1])) {
            let st = i;
            while i < hi && ident(b[i]) {
                i += 1;
            }
            if b.get(i) != Some(&b'(') || b[st].is_ascii_digit() {
                continue;
            }
            let before = s[..st].trim_end();
            if before.ends_with("fn") && !before.ends_with("_fn") || before.ends_with('.') {
                continue;
            }
            out.push((s[st..i].to_string(), st, i, before.ends_with("::")));
        } else {
            i += 1;
        }
    }
    out
}

/// Top-level arguments of the call whose '(' is at `open`, as byte ranges.
fn args(b: &[u8], open: usize) -> Vec<(usize, usize)> {
    let close = close_of(b, open);
    let mut out = Vec::new();
    let (mut depth, mut st) = (0i32, open + 1);
    for (i, &c) in b.iter().enumerate().take(close).skip(open + 1) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push((st, i));
                st = i + 1;
            }
            _ => {}
        }
    }
    out.push((st, close));
    out
}

fn line_of(s: &str, pos: usize) -> usize {
    s[..pos].bytes().filter(|&c| c == b'\n').count()
}

/// The first unwaived allocation in `s[lo..hi]` or in a local function it calls: (line, token, call chain).
fn alloc_in(s: &str, orig: &[&str], defs: &[Def], lo: usize, hi: usize, seen: &mut HashSet<usize>) -> Option<(usize, &'static str, String)> {
    let b = s.as_bytes();
    let waived = |line: usize| orig.get(line).is_some_and(|l| l.contains(WAIVER)) || line > 0 && orig.get(line - 1).is_some_and(|l| l.contains(WAIVER));
    for tok in ALLOCS {
        let mut from = lo;
        while let Some(p) = s[from..hi].find(tok) {
            let at = from + p;
            from = at + tok.len();
            if ident(tok.as_bytes()[0]) && at > 0 && ident(b[at - 1]) && !tok.ends_with("String::") {
                continue;
            }
            let line = line_of(s, at);
            if !waived(line) {
                return Some((line, tok, String::new()));
            }
        }
    }
    for (name, st, _, path) in calls(s, lo, hi) {
        if path {
            continue;
        }
        for d in resolve(defs, &name, st) {
            if (defs[d].body.0 <= st && st < defs[d].body.1) || !seen.insert(d) {
                continue;
            }
            if let Some((line, tok, via)) = alloc_in(s, orig, defs, defs[d].body.0, defs[d].body.1 + 1, seen) {
                return Some((line, tok, format!(" via {name}(){via}")));
            }
        }
    }
    None
}

/// Lint one test source; violations as `file:line: ...` lines; `None` when the file names no fork helper.
fn check_file(rel: &str, src: &str) -> Option<Vec<String>> {
    let s = strip(src);
    if !HELPERS.iter().any(|h| s.contains(h)) {
        return None;
    }
    let orig: Vec<&str> = src.lines().collect();
    let defs = defs(&s);
    let b = s.as_bytes();
    // The raw-fork names: the selftest helpers this file does not shadow, then every local function whose
    // body calls a raw-fork name (a wrapper such as `aborts`), to a fixpoint.
    let mut raw: HashSet<String> = HELPERS.iter().filter(|h| !defs.iter().any(|d| d.name == **h)).map(|h| (*h).to_string()).collect();
    let is_raw = |raw: &HashSet<String>, name: &str, st: usize| raw.contains(name) || HELPERS.contains(&name) && s[..st].ends_with("selftest::");
    // A wrapper passes on a parameter: an argument that is a bare name of no local function.
    let forwards = |st: usize, open: usize| {
        args(b, open).iter().any(|&(lo, hi)| {
            let a = s[lo..hi].trim();
            !a.is_empty() && a.bytes().all(ident) && resolve(&defs, a, st).is_empty()
        })
    };
    loop {
        let more: Vec<String> = defs
            .iter()
            .filter(|d| !raw.contains(&d.name) && calls(&s, d.body.0, d.body.1).iter().any(|(n, st, open, _)| is_raw(&raw, n, *st) && forwards(*st, *open)))
            .map(|d| d.name.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        raw.extend(more);
    }
    let wrappers: Vec<(usize, usize)> = defs.iter().filter(|d| raw.contains(&d.name)).map(|d| d.body).collect();
    let mut errors = Vec::new();
    for (name, st, open, _) in calls(&s, 0, b.len()) {
        if !is_raw(&raw, &name, st) || wrappers.iter().any(|w| w.0 <= st && st < w.1) {
            continue;
        }
        for (lo, hi) in args(b, open) {
            let a = s[lo..hi].trim();
            let a0 = lo + (s[lo..hi].len() - s[lo..hi].trim_start().len());
            let body = if a.starts_with('|') || a.starts_with("move |") {
                Some(vec![(a0, hi)])
            } else if !a.is_empty() && a.bytes().all(|c| ident(c) || c == b':') {
                let last = a.rsplit("::").next().unwrap_or(a);
                Some(resolve(&defs, last, st).into_iter().map(|d| (defs[d].body.0, defs[d].body.1 + 1)).collect())
            } else {
                None
            };
            for (blo, bhi) in body.unwrap_or_default() {
                if let Some((line, tok, via)) = alloc_in(&s, &orig, &defs, blo, bhi, &mut HashSet::new()) {
                    errors.push(format!("{rel}:{}: `{tok}` allocates in a raw-fork child body ({name} at line {}{via}) - build it before the fork, or `// {WAIVER} <why>`", line + 1, line_of(&s, st) + 1));
                }
            }
        }
    }
    Some(errors)
}

pub fn fork_child_alloc(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for e in fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?.flatten() {
        rust_files(&e.path().join("tests"), &mut files);
    }
    files.sort();
    let mut errors = Vec::new();
    let mut scanned = 0;
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else { continue };
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        if let Some(e) = check_file(&rel, &src) {
            scanned += 1;
            errors.extend(e);
        }
    }
    report("fork-child-alloc", scanned, errors)
}

#[cfg(test)]
mod tests {
    use super::check_file;

    #[test]
    fn a_planted_allocation_in_a_child_body_fails() {
        let clean = "fn aborts(body: fn(&CSys) -> i32) { selftest::in_child_quiet(body).unwrap(); }\n\
                     fn t() { let v = vec![1]; aborts(|_| { let b = [0u8; 4]; s::f(b.as_ptr()); 0 }); }\n";
        assert_eq!(check_file("x.rs", clean), Some(vec![]));
        let closure = clean.replace("let b = [0u8; 4];", "let b = vec![0u8; 4];");
        assert_eq!(check_file("x.rs", &closure).unwrap().len(), 1);
        let helper = "fn mk() -> Vec<u8> { b\"ab\".to_vec() }\nfn child(_: &CSys) -> i32 { mk().len() as i32 }\nfn t() { selftest::in_child(child).unwrap(); }\n";
        let e = check_file("x.rs", helper).unwrap();
        assert!(e.len() == 1 && e[0].contains("via mk()"), "{e:?}");
        let waived = helper.replace("{ b\"ab\"", "{ // fork-alloc-ok: test\n b\"ab\"");
        assert_eq!(check_file("x.rs", &waived), Some(vec![]));
        // A file's own re-executing `in_child` is not a raw fork.
        let reexec = "fn in_child(n: &str) -> bool { Command::new(n).status().is_ok() }\nfn t() { in_child(&format!(\"x\")); }\n";
        assert_eq!(check_file("x.rs", reexec), Some(vec![]));
    }
}
