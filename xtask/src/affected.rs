//! `cargo xtask affected [--base <ref>] [--files <path>...] [--only <names>] [--list | --run]`: run only the tests a
//! change can affect (PR-4; owner item: token cost).
//!
//! Compiling the workspace is cheap; the nine thousand tests and the conformance programs are
//! the cost. This command maps the changed files to the tests that can reach them, CONSERVATIVELY:
//! when it cannot tell, it selects more. A missed affected test is a bug; an extra test is only
//! cost. The full two-machine gate (`cargo xtask test`) still runs once at merge.
//!
//! Changed files are names only: `git status --porcelain` (untracked files too) plus
//! `git diff --name-only $(git merge-base HEAD <base>)` (default base `origin/main`, else
//! `main`; the env `AFFECTED_BASE` sets the default for a branch stacked on an unmerged parent). In a tree without git history (the VM role trees: `sync_tree.sh` copies files only) the
//! list comes from `.changed`, which the sync writes from the local checkout. `--files` replaces the
//! git query with an explicit list (used by the unit tests and to
//! ask "what would a change in X select?" without making it).
//!
//! # Rules, in order of precedence
//!
//! 1. ESCALATE TO FULL (everything: the workspace clippy and `cargo xtask test`): `csys/` (but
//!    the assembly sources, rule 6), `crates/abi`, build scripts, `Cargo.toml`/`Cargo.lock`, the
//!    toolchain file, `xtask/` (but this module and the audit modules, rule 7), the generated tables and their inputs (`spec/clauses.tsv`, `symbols.tsv`,
//!    `primitives.tsv`, `spec/abi`, `spec/unicode`, `*_gen.rs`, `tables.rs`, `builtin_items.rs`,
//!    `include/`, the math fits and coefficients), the product crates, linker and export lists,
//!    any non-Rust file under a crate's `src/`, and any path this table does not know.
//! 2. DOCS ONLY (`docs/`, `plan/`, `handoff/`, `*.md`, `spec/behaviour`, `spec/schedule.tsv`, and the
//!    operator tools under the top-level `tools/`, e.g. the probe service, which no test reaches):
//!    `stubs` (which runs `trace`) and the policy checks, no tests.
//! 3. A conformance program (`conformance/c/*.c`): that program, and the tests that name it.
//!    A header `conformance/c/*.h`: the programs that `#include` it (none: every program) and
//!    the tests that name it. `conformance/run.sh` and `conformance/tools`: every conformance program.
//! 4. Rust source of a crate:
//!    * `core`, `boundary`, `legacy`, `math`: NARROWED through a module graph. The changed file
//!      belongs to a top-level module (a file, a directory, or one file of `boundary/src/posix`).
//!      The set of affected modules is its reverse closure over textual references (`m::`, or any
//!      word in a `use` statement; `mod` declarations do not count) across all crates that may use
//!      it. The spec sections of the affected `posix/sNN_*` modules (plus section 0, the
//!      conventions) are the affected sections. A test target, or conformance program, is
//!      selected when it carries one of those sections (file name `sNN_`, `§N`, a clause ID
//!      `sN_...`, a plan row `D-N.`), or references an affected module, or carries no section at
//!      all (then nothing says it is unrelated). A root file (`lib.rs`, `main.rs`) or a module
//!      re-exported with `pub use` is not narrowed.
//!    * every other crate (`kernel`, `rt`, `mem`, `ld`, `native`, `thread-db`, ...), a root file:
//!      WIDE: every test target of every crate that depends on it (dev-dependencies included),
//!      directly or not, and every conformance program.
//! 5. A test file: that target, and the targets of the crate that name the file (`#[path]`,
//!    `include!`); a file in a support directory of `tests/`: every target that names the
//!    directory. Examples, benches and other crate files: clippy only.
//! 6. An assembly source of csys (`csys/src/**/*.S`): its symbols (`.globl`/`.weak` names and
//!    `liella_` labels) are followed to the Rust files and conformance programs that use them
//!    (declarations in `extern` blocks and C prototypes do not count), and each user is selected
//!    as if it had changed (rules 3 to 5), plus the tests that name the file and clippy of the
//!    crate whose build script compiles csys. Computed on every run from the tree, so it cannot
//!    go stale. FULL when unsure: a new, deleted or generated file, a C source, a macro,
//!    `#include` or `.set` (which can define symbols unseen), a symbol not prefixed `liella_`
//!    (may be exported C ABI), a symbol named by another csys source, a link list, a script or
//!    xtask, or no user at all.
//! 7. An xtask audit/report module (`XTASK_AUDITS`: policy, symgap, status, schedule): the
//!    xtask unit tests, clippy, the policy checks and the module's own command.
//!
//! TIERS (GATE-TIER, coordinator ruling 10-11): `--tier direct` (the DEFAULT, per-fix gates) applies rule 4 to EVERY
//! crate's non-root module through `Selection::narrow_direct`: the changed module, its one-hop users, the targets
//! naming either (not through hubs or test-support modules), and the spec sections of those modules only; no reverse
//! closure, no section 0, no "carries no section". Rules 1-3 and 5-7 (and every escalation to FULL) are unchanged.
//! `--tier wide` is the rule 4 above; `--tier full` runs everything, as the landing gate (`cargo xtask test`) does.
//! `--replay <records>` checks that the failures of past gates fall inside a tier (`replay`).
//!
//! `--only <names>` (red/green runs): exactly the named test targets, `conformance:<program>`,
//! `lib:<crate>` or `xtask`, without stubs, policy checks or clippy; labelled NOT A GATE in the
//! plan and the summary.
//!
//! Always: `stubs`, the policy checks (`purity`, `unsafe-audit`, `csys-audit`), and clippy
//! (`--all-targets --all-features -D warnings`) on every changed crate.
//!
//! Known limit: the module graph follows names, not types. A trait method called without naming
//! its module is not seen; the closure-wide reasons of the narrowed crates are written so that
//! this is rare, but it is the reason the full gate runs once at merge.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Crates whose changes are narrowed through the module graph.
const NARROW: &[&str] = &["liella", "liella-boundary", "liella-legacy", "liella-math"];

// ---------------------------------------------------------------------------------------------
// The repository model

struct Crate {
    name: String,
    dir: String,
    deps: BTreeSet<String>,
}

struct Node {
    krate: String,
    name: String,
    /// `lib.rs`, `main.rs`: the crate root.
    root: bool,
    section: Option<u32>,
    /// GATE-TIER-3: the spec sections the module carries: its `posix/sNN` file name, else its header (`sections_of`).
    hdr: BTreeSet<u32>,
    refs: BTreeSet<Ref>,
    /// The text of the root file (for its `pub use` statements).
    reexports: Vec<BTreeSet<String>>,
    /// The module's source (all its files).
    text: String,
}

struct Target {
    krate: String,
    name: String,
    sections: BTreeSet<u32>,
    refs: BTreeSet<Ref>,
    /// Support directories of `tests/` the target names.
    support: BTreeSet<String>,
    /// Needs the products (`cargo xtask so`).
    products: bool,
    /// Whole text, lower-cased names of conformance programs it mentions are found by `mentions`.
    text: String,
    /// GATE-TIER-3: `text[..own]` is the target's own source (its file, or its directory), without the support files.
    own: usize,
}

struct Conf {
    name: String,
    sections: BTreeSet<u32>,
    /// The quoted `#include "x"` names of the program.
    includes: BTreeSet<String>,
    /// GATE-TIER-2: the C entry points of the workspace it names (keys of `Repo::entries`).
    calls: BTreeSet<String>,
}

/// GATE-TIER-2: one Rust definition of a C entry point (`extern "C" fn name`): the module that defines it and the
/// modules its body names. A conformance program is C, so naming-based reach never finds it; it is reached through
/// the entry points it calls instead.
struct Entry {
    node: usize,
    refs: BTreeSet<Ref>,
}

/// A source file of the C layer (`csys/src`).
struct CsysFile {
    /// `@generated` (by `cargo xtask csys-gen`): its check runs only in the full gate.
    generated: bool,
    /// The symbols it defines (see `csys_symbols`); `None`: unmappable.
    symbols: Option<Vec<String>>,
}

/// One occurrence of a csys symbol outside comments: the file (relative, `/`) and whether the
/// line only declares it (an `extern` block entry or a C prototype).
struct SymRef {
    file: String,
    decl: bool,
}

pub struct Repo {
    crates: Vec<Crate>,
    /// GATE-TIER-3: the files of the `tests/` support directories some target names (path -> text).
    support_texts: BTreeMap<String, String>,
    /// Transitive dependencies per crate name (dev-dependencies included).
    closure: BTreeMap<String, BTreeSet<String>>,
    nodes: Vec<Node>,
    targets: Vec<Target>,
    conf: Vec<Conf>,
    /// Modules named by at least a sixth of all modules (indexes into `nodes`).
    hubs: BTreeSet<usize>,
    /// Modules declared only under `#[cfg(feature = "selftest")]` / `#[cfg(test)]` in their crate root, `(crate, module)`:
    /// never in a product (derived from the source by `test_only_modules`).
    test_only: BTreeSet<(String, String)>,
    /// `csys/src/...` path -> its symbols.
    csys: BTreeMap<String, CsysFile>,
    /// GATE-TIER-2: C symbol -> its Rust definitions (`entries_of`), plus the csys-defined symbols that
    /// `docs/codemap/symbols.tsv` maps to a Rust entry (`printf` -> `liella_vfprintf4`).
    entries: BTreeMap<String, Vec<Entry>>,
    /// GATE-TIER-2: modules named by the bodies of at least a sixth of all entries (`stdio`, `conv`, `ret`, ...): like
    /// `hubs`, an entry reaches a change through them only when they changed themselves.
    entry_hubs: BTreeSet<usize>,
    /// csys symbol -> its occurrences in the repository's sources and link inputs.
    csys_refs: BTreeMap<String, Vec<SymRef>>,
    /// TOOLS-7 (3): `git diff -U0` of `csys/include/` against the base, which the sync writes to `.changed.hdr` (a VM
    /// tree has no history). `None`: not available, so a changed header is not mapped (FULL).
    hdr_diff: Option<String>,
}

/// The symbols an assembly source of csys defines: the `.globl`/`.global`/`.weak` names and every
/// label at column 0 that starts with `liella_`. `None` when the file can define symbols this
/// reading does not see (a macro, `#define`, `#include`, `.set`/`.equ`): unmappable. A one-line `.symver a, name@VER` alias is mapped (it defines `a` and `name`).
/// C sources are not read (definitions behind macros and multi-line declarators need the
/// compiler to be sure): `None`.
fn csys_symbols(path: &str, text: &str) -> Option<Vec<String>> {
    if !path.ends_with(".S") {
        return None;
    }
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim_start();
        let first = t.split([' ', '\t']).next().unwrap_or("");
        if matches!(first, ".macro" | "#define" | "#include" | ".set" | ".equ" | ".equiv" | ".include" | ".irp" | ".rept") {
            return None;
        }
        if first == ".symver" {
            // TOOLS-9: a one-line alias `.symver <local>, <name>@<VERSION>` (also `@@`) defines `<name>` (the exported
            // symbol, the one tests and users refer to) and `<local>`. Any other shape stays unmappable.
            let args = t[first.len()..].trim();
            let (local, versioned) = args.split_once(',')?;
            let (local, versioned) = (local.trim(), versioned.trim());
            let name = versioned.split('@').next().unwrap_or("");
            if local.is_empty() || name.is_empty() || !versioned.contains('@') || versioned.contains([' ', '\t', ',']) || local.contains([' ', '\t']) {
                return None;
            }
            out.push(local.to_string());
            out.push(name.to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix(".globl").or_else(|| t.strip_prefix(".global")).or_else(|| t.strip_prefix(".weak")) {
            out.extend(rest.split([' ', '\t', ',']).filter(|w| !w.is_empty()).map(String::from));
        } else if l.starts_with("liella_") {
            if let Some((label, _)) = l.split_once(':') {
                out.push(label.trim().to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    Some(out)
}

/// The occurrences of `symbols` in `text` (file `rel`), skipping comment lines.
fn sym_refs(rel: &str, text: &str, symbols: &BTreeSet<String>, out: &mut BTreeMap<String, Vec<SymRef>>) {
    let code = rel.ends_with(".rs") || rel.ends_with(".c") || rel.ends_with(".h") || rel.ends_with(".S");
    for l in text.lines() {
        let t = l.trim();
        if code && (t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') || (rel.ends_with(".S") && t.starts_with('#') && !t.starts_with("#include"))) {
            continue;
        }
        for w in t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| symbols.contains(*w)) {
            // `fn name(...);` in an extern block, or a C prototype: a declaration, not a use.
            let decl = code && t.ends_with(';') && !t.contains('=') && !t.contains('{') && (t.contains(&format!("fn {w}(")) || (!rel.ends_with(".rs") && t.contains(&format!("{w}("))) || t.contains(&format!("static {w}:")) || t.contains(&format!("static mut {w}:")));
            out.entry(w.to_string()).or_default().push(SymRef { file: rel.to_string(), decl });
        }
    }
}

fn read(path: &Path) -> String {
    fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

fn sorted_dir(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn files_under(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    for p in sorted_dir(dir) {
        if p.is_dir() {
            files_under(&p, exts, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)) {
            out.push(p);
        }
    }
}

fn parse_manifest(text: &str) -> (String, BTreeSet<String>) {
    let mut name = String::new();
    let mut deps = BTreeSet::new();
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(h) = l.strip_prefix('[') {
            section = h.trim_end_matches(']').trim_matches('[').to_string();
            // `[dependencies.foo]`
            for prefix in ["dependencies.", "dev-dependencies.", "build-dependencies."] {
                if let Some(d) = section.strip_prefix(prefix) {
                    deps.insert(d.to_string());
                }
            }
            continue;
        }
        if section == "package" {
            if let Some(rest) = l.strip_prefix("name") {
                if let Some(v) = rest.trim_start().strip_prefix('=') {
                    name = v.trim().trim_matches('\u{22}').to_string();
                }
            }
        } else if section.ends_with("dependencies") {
            let Some((key, value)) = l.split_once('=') else { continue };
            let key = key.trim();
            if key.is_empty() || key.starts_with('#') || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                continue;
            }
            match value.split_once("package") {
                Some((_, p)) if p.trim_start().starts_with('=') => {
                    deps.insert(p.split('\u{22}').nth(1).unwrap_or(key).to_string());
                }
                _ => {
                    deps.insert(key.to_string());
                }
            }
        }
    }
    (name, deps)
}

/// The leading comment block of a Rust or C file (its declared subject), at most 60 lines.
fn header_of(text: &str) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in text.lines().take(60) {
        let t = line.trim_start();
        let comment = in_block || t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') || t.starts_with("#!") || t.starts_with("#[");
        if !comment {
            break;
        }
        if t.starts_with("/*") && !t.contains("*/") {
            in_block = true;
        }
        if t.contains("*/") {
            in_block = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Spec sections of a test or program: its `sNN_` file-name prefix if it has one; otherwise the
/// ones its header names (`§N`, clause IDs `sN_<digit>`, plan rows `D-N.`). Mentions further down
/// a file (a comment about another section) do not make it a test of that section.
fn sections_of(file_name: &str, text: &str) -> BTreeSet<u32> {
    let by_name = sections_of_text(file_name, "");
    if by_name.is_empty() { sections_of_text("", &header_of(text)) } else { by_name }
}

/// Spec sections a file name or text carries: `sNN_` prefix, `§N`, clause IDs `sN_<digit>`, plan
/// rows `D-N.`.
fn sections_of_text(file_name: &str, text: &str) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let num = |s: &str| -> Option<u32> {
        let d: String = s.chars().take_while(char::is_ascii_digit).collect();
        d.parse().ok()
    };
    if let Some(r) = file_name.strip_prefix('s') {
        let digits = r.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && r[digits..].starts_with('_') {
            if let Ok(n) = r[..digits].parse() {
                out.insert(n);
            }
        }
    }
    for part in text.split('§').skip(1) {
        if let Some(n) = num(part) {
            out.insert(n);
        }
    }
    let b = text.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        let boundary = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if !boundary {
            continue;
        }
        if c == b's' {
            let rest = &text[i + 1..];
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && rest.as_bytes().get(digits) == Some(&b'_') && rest.as_bytes().get(digits + 1).is_some_and(u8::is_ascii_digit) {
                if let Ok(n) = rest[..digits].parse() {
                    out.insert(n);
                }
            }
        } else if c == b'D' && text[i..].starts_with("D-") {
            let rest = &text[i + 2..];
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && rest.as_bytes().get(digits) == Some(&b'.') {
                if let Ok(n) = rest[..digits].parse() {
                    out.insert(n);
                }
            }
        }
    }
    out
}

/// A reference to a module: the crate it names (`None`: the crate the text is in, or not
/// determined) and the module (the path segment after the crate).
type Ref = (Option<String>, String);

#[derive(Debug, PartialEq)]
enum Tok {
    Id(String),
    Sep,
    Open,
    Close,
    Comma,
    Semi,
    Star,
}

fn tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_alphabetic() || c == b'_' {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(Tok::Id(line[s..i].to_string()));
                continue;
            }
            match c {
                b':' if b.get(i + 1) == Some(&b':') => {
                    out.push(Tok::Sep);
                    i += 1;
                }
                b'{' => out.push(Tok::Open),
                b'}' => out.push(Tok::Close),
                b',' => out.push(Tok::Comma),
                b';' => out.push(Tok::Semi),
                b'*' => out.push(Tok::Star),
                _ => {}
            }
            i += 1;
        }
    }
    out
}

/// Module references of a Rust text. A path rooted at a crate's library name names that crate's
/// module (its second segment); `crate`, `self`, `super` and a bare root name a module of the
/// crate the text is in; paths into `std`, `core` and `alloc` are not ours. In a `use` tree every
/// path is resolved the same way (`use liella::{printf, fs::dir}` names `printf` and `fs`). A
/// `mod` declaration is not a reference. `aliases` maps library names to package names.
fn refs_of(text: &str, aliases: &BTreeMap<String, String>) -> BTreeSet<Ref> {
    let toks = tokens(text);
    let mut out = BTreeSet::new();
    let mut record = |path: &[String]| {
        let Some(root) = path.first() else { return };
        if matches!(root.as_str(), "std" | "core" | "alloc" | "*") {
            return;
        }
        if let Some(krate) = aliases.get(root) {
            if let Some(m) = path.get(1) {
                if m != "*" {
                    out.insert((Some(krate.clone()), m.clone()));
                }
            }
        } else if matches!(root.as_str(), "crate" | "self" | "super") {
            if let Some(m) = path.get(1) {
                if m != "*" {
                    out.insert((None, m.clone()));
                }
            }
        } else {
            out.insert((None, root.clone()));
        }
    };
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Id(id) if id == "use" => {
                // The use tree up to its `;`.
                let mut prefixes: Vec<Vec<String>> = vec![Vec::new()];
                let mut cur: Vec<String> = Vec::new();
                i += 1;
                while i < toks.len() {
                    match &toks[i] {
                        Tok::Id(w) if w == "as" => {
                            i += 1; // the alias
                        }
                        Tok::Id(w) => cur.push(w.clone()),
                        Tok::Star => cur.push("*".to_string()),
                        Tok::Sep => {}
                        Tok::Open => {
                            let mut p = prefixes.last().cloned().unwrap_or_default();
                            p.append(&mut cur);
                            prefixes.push(p);
                        }
                        Tok::Comma | Tok::Close | Tok::Semi => {
                            if !cur.is_empty() {
                                let mut full = prefixes.last().cloned().unwrap_or_default();
                                full.append(&mut cur);
                                record(&full);
                            }
                            if toks[i] == Tok::Close && prefixes.len() > 1 {
                                prefixes.pop();
                            }
                            if toks[i] == Tok::Semi {
                                break;
                            }
                        }
                    }
                    i += 1;
                }
            }
            Tok::Id(id) if id == "mod" => {
                i += 2; // the name
            }
            Tok::Id(_) if i > 0 && toks[i - 1] == Tok::Sep => {
                i += 1; // not the start of a path
            }
            Tok::Id(_) => {
                let mut path = Vec::new();
                let mut j = i;
                while let Some(Tok::Id(w)) = toks.get(j) {
                    path.push(w.clone());
                    if toks.get(j + 1) == Some(&Tok::Sep) {
                        j += 2;
                    } else {
                        break;
                    }
                }
                if path.len() > 1 {
                    record(&path);
                }
                i = j.max(i) + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// The identifiers of each `pub use` statement.
fn reexports_of(text: &str) -> Vec<BTreeSet<String>> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if cur.is_none() && (t.starts_with("pub use ") || t.starts_with("pub(crate) use ")) {
            cur = Some(String::new());
        }
        if let Some(s) = cur.as_mut() {
            s.push_str(t);
            s.push(' ');
            if t.contains(';') {
                let words: BTreeSet<String> = s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '*')).filter(|w| !w.is_empty()).map(String::from).collect();
                out.push(words);
                cur = None;
            }
        }
    }
    out
}

/// GATE-TIER-2: the C entry points a module's text defines (`extern "C" fn name`, `extern "C-unwind" fn name`) and the
/// references of each body (the `fn` line to the first `}` line at its indentation; a one-line body is that line).
fn entries_of(text: &str, aliases: &BTreeMap<String, String>) -> Vec<(String, BTreeSet<Ref>)> {
    let lines: Vec<&str> = text.lines().collect();
    let indent = |l: &str| l.len() - l.trim_start().len();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let Some(k) = t.find("extern \"C").and_then(|k| t[k..].find(" fn ").map(|f| k + f + 4)) else { continue };
        let name: String = t[k..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if name.is_empty() || t.trim_end().ends_with(';') {
            continue;
        }
        let mut end = i;
        if !(t.trim_end().ends_with('}') && t.contains('{')) {
            end = lines.iter().enumerate().skip(i + 1).find(|(_, m)| !m.trim().is_empty() && indent(m) <= indent(l) && m.trim_start().starts_with('}')).map_or(lines.len() - 1, |(e, _)| e);
        }
        out.push((name, refs_of(&lines[i..=end].join("\n"), aliases)));
    }
    out
}

/// GATE-TIER-2: the identifiers of a C text (comments are not removed: a name in a comment over-selects, never misses).
fn c_words(text: &str) -> BTreeSet<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| w.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')).collect()
}

/// The modules a crate root declares only under `#[cfg(feature = "selftest")]` or `#[cfg(test)]` (the cfg line, then
/// `[pub] mod name;` or `mod name {`).
fn test_only_modules(nodes: &[Node]) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for n in nodes.iter().filter(|n| n.root) {
        let mut cfg = false;
        for l in n.text.lines().map(str::trim) {
            if l.starts_with("#[cfg(") {
                cfg = l.contains("feature = \"selftest\"") || l == "#[cfg(test)]";
                continue;
            }
            if l.starts_with("#[") || l.is_empty() {
                continue;
            }
            if cfg {
                let rest = l.strip_prefix("pub ").unwrap_or(l);
                if let Some(m) = rest.strip_prefix("mod ") {
                    out.insert((n.krate.clone(), m.trim_end_matches([';', '{', ' ']).to_string()));
                }
            }
            cfg = false;
        }
    }
    out
}

impl Repo {
    /// Is `krate::module` declared only under a test cfg?
    #[cfg(test)]
    pub fn test_only_has(&self, krate: &str, module: &str) -> bool {
        self.test_only.contains(&(krate.to_string(), module.to_string()))
    }

    pub fn load(root: &Path) -> Repo {
        let top = read(&root.join("Cargo.toml"));
        let mut members = Vec::new();
        if let Some(i) = top.find("members") {
            if let Some(open) = top[i..].find('[') {
                if let Some(close) = top[i + open..].find(']') {
                    members = top[i + open + 1..i + open + close].split('\u{22}').skip(1).step_by(2).map(String::from).collect();
                }
            }
        }
        let mut crates = Vec::new();
        for dir in &members {
            let (name, deps) = parse_manifest(&read(&root.join(dir).join("Cargo.toml")));
            if !name.is_empty() {
                crates.push(Crate { name, dir: dir.clone(), deps });
            }
        }
        let names: BTreeSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let direct: BTreeMap<String, BTreeSet<String>> = crates.iter().map(|c| (c.name.clone(), c.deps.iter().filter(|d| names.contains(*d)).cloned().collect())).collect();
        let mut closure = BTreeMap::new();
        for c in &crates {
            let mut seen = BTreeSet::new();
            let mut stack: Vec<String> = direct[&c.name].iter().cloned().collect();
            while let Some(d) = stack.pop() {
                if seen.insert(d.clone()) {
                    stack.extend(direct[&d].iter().cloned());
                }
            }
            closure.insert(c.name.clone(), seen);
        }
        let aliases: BTreeMap<String, String> = crates.iter().map(|c| (c.name.replace('-', "_"), c.name.clone())).collect();
        let mut nodes = Vec::new();
        let mut targets = Vec::new();
        let mut support_texts = BTreeMap::new();
        for c in crates.iter().filter(|c| c.dir != "xtask") {
            let cdir = root.join(&c.dir);
            // Nodes: top-level modules of src (one per file of boundary's `posix/`).
            let src = cdir.join("src");
            for p in sorted_dir(&src) {
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
                let mut files = Vec::new();
                if p.is_dir() {
                    files_under(&p, &["rs"], &mut files);
                } else if p.extension().is_some_and(|e| e == "rs") {
                    files.push(p.clone());
                } else {
                    continue;
                }
                if p.is_dir() && stem == "posix" && c.name == "liella-boundary" {
                    for f in files {
                        let n = f.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
                        let text = read(&f);
                        nodes.push(Node { krate: c.name.clone(), section: sections_of(&n, "").into_iter().next(), hdr: sections_of(&n, &text), name: n, root: false, refs: refs_of(&text, &aliases), reexports: Vec::new(), text });
                    }
                    continue;
                }
                let text: String = files.iter().map(|f| read(f)).collect::<Vec<_>>().join("\n");
                let root_file = !p.is_dir() && (stem == "lib" || stem == "main");
                nodes.push(Node { krate: c.name.clone(), name: stem, root: root_file, section: None, hdr: sections_of("", &text), refs: refs_of(&text, &aliases), reexports: if root_file { reexports_of(&text) } else { Vec::new() }, text });
            }
            // Test targets: tests/*.rs and tests/*/main.rs, with the support directories they name.
            let tdir = cdir.join("tests");
            let entries = sorted_dir(&tdir);
            let subdirs: Vec<(String, PathBuf)> = entries.iter().filter(|p| p.is_dir()).map(|p| (p.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string(), p.clone())).collect();
            for p in &entries {
                let (name, file) = if p.is_dir() {
                    if !p.join("main.rs").exists() {
                        continue;
                    }
                    (p.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string(), p.join("main.rs"))
                } else if p.extension().is_some_and(|e| e == "rs") {
                    (p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string(), p.clone())
                } else {
                    continue;
                };
                let mut text = read(&file);
                if p.is_dir() {
                    let mut all = Vec::new();
                    files_under(p, &["rs", "c", "h"], &mut all);
                    text = all.iter().map(|f| read(f)).collect::<Vec<_>>().join("\n");
                }
                let own = text.len();
                let mut support = BTreeSet::new();
                for (d, dp) in &subdirs {
                    if *dp != *p && text.contains(d.as_str()) {
                        support.insert(d.clone());
                        let mut all = Vec::new();
                        files_under(dp, &["rs", "c", "h"], &mut all);
                        for f in all {
                            let t = read(&f);
                            text.push('\n');
                            text.push_str(&t);
                            support_texts.insert(f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/"), t);
                        }
                    }
                }
                let products = text.contains("target/liella") || text.contains("libc.a") || text.contains("liella-crt1") || text.contains("conformance");
                targets.push(Target { krate: c.name.clone(), sections: sections_of(&name, &text), refs: refs_of(&text, &aliases), name, support, products, text, own });
            }
        }
        // GATE-TIER-2: the C entry points and the modules each one's body names.
        let mut entries: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
        for (x, n) in nodes.iter().enumerate() {
            for (name, refs) in entries_of(&n.text, &aliases) {
                entries.entry(name).or_default().push(Entry { node: x, refs });
            }
        }
        // A symbol csys defines and hands to a Rust entry (`printf` -> `liella_vfprintf4`): the codemap's
        // "first implementation call" column. A stale codemap only loses these extra names.
        for l in read(&root.join("docs/codemap/symbols.tsv")).lines().skip(1) {
            let f: Vec<&str> = l.split('\t').collect();
            if let (Some(sym), Some(imp)) = (f.first(), f.get(3)) {
                if !entries.contains_key(*sym) && entries.contains_key(*imp) {
                    let to: Vec<Entry> = entries[*imp].iter().map(|e| Entry { node: e.node, refs: e.refs.clone() }).collect();
                    entries.insert(sym.to_string(), to);
                }
            }
        }
        let mut named_by: BTreeMap<usize, usize> = BTreeMap::new();
        let all_entries: Vec<&Entry> = entries.values().flatten().collect();
        for e in &all_entries {
            for (x, n) in nodes.iter().enumerate() {
                if e.refs.contains(&(Some(n.krate.clone()), n.name.clone())) || (n.krate == nodes[e.node].krate && e.refs.contains(&(None, n.name.clone()))) {
                    *named_by.entry(x).or_default() += 1;
                }
            }
        }
        let entry_hubs: BTreeSet<usize> = named_by.into_iter().filter(|&(_, k)| k * 6 >= all_entries.len()).map(|(x, _)| x).collect();
        let mut conf = Vec::new();
        for p in sorted_dir(&root.join("conformance/c")) {
            if p.extension().is_some_and(|e| e == "c") {
                let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
                let text = read(&p);
                let includes = text.lines().filter_map(|l| l.trim().strip_prefix("#include")).filter_map(|r| r.trim().strip_prefix('\u{22}')).filter_map(|r| r.split('\u{22}').next()).map(String::from).collect();
                let calls = c_words(&text).into_iter().filter(|w| entries.contains_key(*w)).map(String::from).collect();
                conf.push(Conf { sections: sections_of(&name, &text), name, includes, calls });
            }
        }
        // The C layer: the symbols of each source and where the repository names them.
        let mut csys = BTreeMap::new();
        let mut all_csys = Vec::new();
        files_under(&root.join("csys/src"), &["S", "c"], &mut all_csys);
        let rel = |p: &Path| p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
        for p in &all_csys {
            let (r, text) = (rel(p), read(p));
            csys.insert(r.clone(), CsysFile { generated: text.contains("@generated"), symbols: csys_symbols(&r, &text) });
        }
        let symbols: BTreeSet<String> = csys.values().filter_map(|c| c.symbols.as_ref()).flatten().cloned().collect();
        let mut csys_refs = BTreeMap::new();
        let mut scan = Vec::new();
        for (dir, exts) in [
            ("crates", &["rs", "c", "h", "S", "map", "ld", "lds", "version", "tsv", "txt", "toml", "sh", "py"][..]),
            ("conformance", &["c", "h", "sh"][..]),
            ("csys", &["c", "h", "S", "tsv"][..]),
            ("include", &["h"][..]),
            ("spec", &["tsv"][..]),
            ("scripts", &["sh", "py", "txt", "map"][..]),
            ("xtask/src", &["rs"][..]),
        ] {
            files_under(&root.join(dir), exts, &mut scan);
        }
        // This command's own source names symbols in its unit tests.
        scan.retain(|p| !p.ends_with("xtask/src/affected.rs"));
        for p in &scan {
            sym_refs(&rel(p), &read(p), &symbols, &mut csys_refs);
        }
        let names = |x: &Node, y: &Node| x.refs.contains(&(Some(y.krate.clone()), y.name.clone())) || (x.krate == y.krate && x.refs.contains(&(None, y.name.clone())));
        let hubs = (0..nodes.len()).filter(|&y| nodes.iter().filter(|x| names(x, &nodes[y])).count() * 6 >= nodes.len()).collect();
        let test_only = test_only_modules(&nodes);
        Repo { crates, support_texts, closure, nodes, targets, conf, hubs, test_only, csys, entries, entry_hubs, csys_refs, hdr_diff: fs::read_to_string(root.join(".changed.hdr")).ok() }
    }

    /// The module of the graph a source path belongs to, `(crate, module)`, as `classify_path` names it.
    pub(crate) fn module_of(&self, path: &str) -> Option<(String, String)> {
        let c = self.crate_of(path)?;
        let inner = path[c.dir.len() + 1..].strip_prefix("src/")?;
        let parts: Vec<&str> = inner.split('/').collect();
        let stem = |s: &str| s.split('.').next().unwrap_or(s).to_string();
        let name = if c.name == "liella-boundary" && parts.len() >= 2 && parts[0] == "posix" { stem(parts[1]) } else { stem(parts[0]) };
        Some((c.name.clone(), name))
    }

    fn crate_of(&self, path: &str) -> Option<&Crate> {
        self.crates.iter().find(|c| path.starts_with(&format!("{}/", c.dir)))
    }

    /// `c` and every crate that depends on it.
    fn dependents(&self, c: &str) -> BTreeSet<String> {
        self.crates.iter().filter(|x| x.name == c || self.closure.get(&x.name).is_some_and(|d| d.contains(c))).map(|x| x.name.clone()).collect()
    }

    fn may_use(&self, user: &str, used: &str) -> bool {
        user == used || self.closure.get(user).is_some_and(|d| d.contains(used))
    }
}

// ---------------------------------------------------------------------------------------------
// The selection

#[derive(Default, Debug)]
pub struct Selection {
    pub full: Vec<String>,
    pub clippy: BTreeMap<String, String>,
    pub libs: BTreeMap<String, String>,
    /// (crate, target) -> reason
    pub tests: BTreeMap<(String, String), String>,
    pub conf: BTreeMap<String, String>,
    pub xtask_tests: bool,
    /// xtask commands to run (`cargo xtask <c>`), for a changed audit/report module.
    pub xtask_cmds: BTreeSet<String>,
    /// `--only`: the named binaries and nothing else (no checks, no clippy): NOT a gate.
    pub only: bool,
    /// TOOLS-7 (3b): the ffi.rs <-> liella_csys.h agreement check (K5) runs: ffi.rs, `csys/include/` or
    /// `csys/src/machine/` changed. Cheap, and the check is otherwise part of the FULL `xtask test` only.
    pub k5: bool,
    pub notes: Vec<String>,
    /// The modules a narrowed change reaches, `(crate, module)` (the reverse closure; `bench-rows` maps them to
    /// the exported symbols and the bench rows that call them).
    pub modules: BTreeSet<(String, String)>,
    /// The crates selected WIDE (every dependent): `bench-rows` cannot narrow them.
    pub wide_crates: BTreeSet<String>,
    /// Product view (`bench-rows`): the closure does not pass through test-only modules (`Repo::test_only`).
    pub products: bool,
    /// GATE-TIER: the DIRECT tier (`Tier::Direct`, see `narrow_direct`); false = the wide rules.
    pub direct: bool,
}

/// GATE-TIER (coordinator ruling 10-11, owner intent "only run the tests that are impacted by a fix for gate runs and
/// then run the whole test suite for the RC merge"): how far a per-change selection reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// The default of `xtask affected` (per-fix gates): the changed modules and their one-hop users only.
    Direct,
    /// The earlier rules: the reverse closure, every section it spans, targets without a section.
    Wide,
    /// Everything (`cargo xtask test`): what a landing gate runs.
    Full,
}

impl Tier {
    fn parse(s: &str) -> Result<Tier, String> {
        match s {
            "direct" => Ok(Tier::Direct),
            "wide" => Ok(Tier::Wide),
            "full" => Ok(Tier::Full),
            _ => Err(format!("--tier: `{s}` is not direct, wide or full")),
        }
    }
}

fn add<K: Ord>(m: &mut BTreeMap<K, String>, k: K, why: &str) {
    m.entry(k).or_insert_with(|| why.to_string());
}

impl Selection {
    fn wide(&mut self, repo: &Repo, krate: &str, why: &str) {
        let deps = repo.dependents(krate);
        self.wide_crates.insert(krate.to_string());
        add(&mut self.clippy, krate.to_string(), why);
        for d in &deps {
            add(&mut self.libs, d.clone(), why);
        }
        for t in repo.targets.iter().filter(|t| deps.contains(&t.krate)) {
            add(&mut self.tests, (t.krate.clone(), t.name.clone()), why);
        }
        for c in &repo.conf {
            add(&mut self.conf, c.name.clone(), why);
        }
    }

    /// AFF-OWNER (TOOL-CLEAN, 2026-10-08): the unit tests of the crate that OWNS a changed source file always run,
    /// whatever the module graph reached. `narrow` can hand a re-exported module to `wide` of ANOTHER crate (the
    /// one whose module was reached), whose dependents do not include the owner (a dependency of it), and a
    /// `--files` run of a core file then ran conformance programs and no cargo suite, which read as a pass.
    fn own_suite(&mut self, repo: &Repo, f: &str) {
        let Some(c) = repo.crate_of(f) else { return };
        if f[c.dir.len() + 1..].starts_with("src/") && f.ends_with(".rs") {
            add(&mut self.libs, c.name.clone(), &format!("{f}: the unit tests of the crate that owns the file"));
        }
    }

    fn all_conf(&mut self, repo: &Repo, why: &str) {
        for c in &repo.conf {
            add(&mut self.conf, c.name.clone(), why);
        }
    }

    /// A change in module `name` of crate `krate`.
    fn narrow(&mut self, repo: &Repo, krate: &str, name: &str, file: &str) {
        let Some(start) = repo.nodes.iter().position(|n| n.krate == krate && n.name == name) else {
            return self.wide(repo, krate, &format!("{file}: module `{name}` not found in the module graph"));
        };
        // Reverse closure over references.
        let mut affected: BTreeSet<usize> = BTreeSet::from([start]);
        let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
        let mut frontier: BTreeSet<usize> = BTreeSet::new();
        let mut depth: BTreeMap<usize, u32> = BTreeMap::new();
        let mut stack = vec![start];
        while let Some(y) = stack.pop() {
            // A hub (`sys`, `conv`, `error`, ...: named by a sixth of all modules) is affected when
            // it names an affected module, but its users are not followed: at module granularity
            // everything reaches a hub and the hub reaches everything, which would select the
            // whole workspace for any change. A change IN a hub follows its users.
            if y != start && repo.hubs.contains(&y) {
                continue;
            }
            for (x, nx) in repo.nodes.iter().enumerate() {
                let ny = &repo.nodes[y];
                // The translation layer is entered from another crate at the modules that name the
                // changed one, and followed through one more helper module, then into the `posix/sNN` files only:
                // the helper modules name each other in a cycle, so following them all would reach
                // every section.
                if frontier.contains(&y) && nx.section.is_none() {
                    continue;
                }
                let names_it = nx.refs.contains(&(Some(ny.krate.clone()), ny.name.clone())) || (nx.krate == ny.krate && nx.refs.contains(&(None, ny.name.clone())));
                if self.products && repo.test_only.contains(&(nx.krate.clone(), nx.name.clone())) {
                    continue;
                }
                if !affected.contains(&x) && names_it && repo.may_use(&nx.krate, &ny.krate) {
                    affected.insert(x);
                    if nx.krate == "liella-boundary" && nx.section.is_none() {
                        // Depth in helper modules since the entry from another crate.
                        let d = if ny.krate != nx.krate { 1 } else { depth.get(&y).copied().unwrap_or(0) + 1 };
                        depth.insert(x, d);
                        if d >= 2 {
                            frontier.insert(x);
                        }
                    }
                    parent.insert(x, y);
                    stack.push(x);
                }
            }
        }
        self.modules.extend(affected.iter().map(|&i| (repo.nodes[i].krate.clone(), repo.nodes[i].name.clone())));
        let label = |i: usize| format!("{}::{}", repo.nodes[i].krate, repo.nodes[i].name);
        let chain: Vec<String> = affected.iter().filter(|&&i| i != start).map(|&i| format!("{} (via {})", label(i), label(parent[&i]))).collect();
        let hubs: Vec<String> = affected.iter().filter(|&&i| repo.hubs.contains(&i)).map(|&i| label(i)).collect();
        self.notes.push(format!("{file}: module {} reaches {} modules: {}; hubs reached but not followed: {}", label(start), chain.len(), chain.join(", "), if hubs.is_empty() { "none".to_string() } else { hubs.join(", ") }));
        let idents: BTreeSet<&str> = affected.iter().map(|&i| repo.nodes[i].name.as_str()).collect();
        // A re-exported module cannot be followed by name.
        for &i in &affected {
            let n = &repo.nodes[i];
            if n.root {
                continue;
            }
            for root in repo.nodes.iter().filter(|r| r.root && r.krate == n.krate) {
                if root.reexports.iter().any(|stmt| stmt.contains(&n.name) || (stmt.contains("*") && stmt.iter().any(|w| idents.contains(w.as_str())))) {
                    add(&mut self.clippy, krate.to_string(), &format!("{file} changed"));
                    return self.wide(repo, &n.krate, &format!("{file}: module `{}` of `{}` is re-exported at its crate root", n.name, n.krate));
                }
            }
        }
        let mut sections: BTreeSet<u32> = BTreeSet::from([0]);
        sections.extend(affected.iter().filter_map(|&i| repo.nodes[i].section));
        let secs: Vec<String> = sections.iter().map(u32::to_string).collect();
        let via = |t: &str| format!("{file}: {t}");
        add(&mut self.clippy, krate.to_string(), &format!("{file} changed"));
        let mut crates_in: BTreeSet<&str> = affected.iter().map(|&i| repo.nodes[i].krate.as_str()).collect();
        crates_in.insert(krate);
        for c in crates_in {
            add(&mut self.libs, c.to_string(), &via(&format!("unit tests of a crate with an affected module ({} affected modules)", affected.len())));
        }
        let eligible = repo.dependents(krate);
        for t in repo.targets.iter().filter(|t| eligible.contains(&t.krate)) {
            let why = if let Some((_, r)) = t.refs.iter().find(|(c, m)| affected.iter().any(|&i| repo.nodes[i].name == *m && c.as_ref().is_none_or(|c| *c == repo.nodes[i].krate))) {
                via(&format!("references affected module `{r}`"))
            } else if let Some(s) = t.sections.intersection(&sections).next() {
                via(&format!("spec section {s} (affected sections {})", secs.join(",")))
            } else if t.sections.is_empty() {
                via("carries no spec section (cannot be shown unrelated)")
            } else {
                continue;
            };
            add(&mut self.tests, (t.krate.clone(), t.name.clone()), &why);
        }
        for c in &repo.conf {
            let why = if let Some(s) = c.sections.intersection(&sections).next() {
                via(&format!("spec section {s} (affected sections {})", secs.join(",")))
            } else if c.sections.is_empty() {
                via("carries no spec section (cannot be shown unrelated)")
            } else {
                continue;
            };
            add(&mut self.conf, c.name.clone(), &why);
        }
    }

    /// GATE-TIER, the DIRECT tier: a change in module `name` of crate `krate` selects
    /// * the unit tests of the crates holding the module and its one-hop users (modules that name it; not
    ///   transitive, hubs and test-support modules included as users but see below),
    /// * the test targets that name the module or one of those users (a user that is a hub or a test-only module,
    ///   e.g. `selftest`, does not select its own namers: that would reach most of the workspace again),
    /// * the targets and conformance programs carrying a spec section of the module or of a one-hop `posix/sNN`
    ///   user (never section 0 by default, never "carries no section").
    ///
    /// A module re-exported at its crate root cannot be followed by name: wide, as before. Escalations to FULL are
    /// decided before this (`classify`). Data (GATE-TIME, 100 gates): every real failure was in a target the change
    /// names directly; the reverse closure caught only flakes. The landing gate runs everything.
    fn narrow_direct(&mut self, repo: &Repo, krate: &str, name: &str, file: &str) {
        let Some(start) = repo.nodes.iter().position(|n| n.krate == krate && n.name == name) else {
            return self.wide(repo, krate, &format!("{file}: module `{name}` not found in the module graph"));
        };
        let ns = &repo.nodes[start];
        // GATE-TIER-2: a module re-exported at its crate root (`pub use errno::{Errno, KResult};`) is used through the
        // re-exported names too: a module naming `liella_kernel::Errno` (or `crate::Errno` inside the crate) is a user.
        // Only a glob re-export (`pub use m::*;`) cannot be followed by name: wide, as before.
        let mut items: BTreeSet<String> = BTreeSet::new();
        if !ns.root {
            for root in repo.nodes.iter().filter(|r| r.root && r.krate == ns.krate) {
                for stmt in root.reexports.iter().filter(|stmt| stmt.contains(&ns.name)) {
                    if stmt.contains("*") {
                        add(&mut self.clippy, krate.to_string(), &format!("{file} changed"));
                        return self.wide(repo, krate, &format!("{file}: module `{name}` of `{krate}` is glob re-exported at its crate root"));
                    }
                    items.extend(stmt.iter().filter(|w| !matches!(w.as_str(), "pub" | "use" | "crate" | "self" | "super" | "as" | "in") && **w != ns.name).cloned());
                }
            }
        }
        if !items.is_empty() {
            self.notes.push(format!("{file}: module `{name}` is re-exported at its crate root as {}: their namers are users", items.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        let names_item = |nx: &Node| items.iter().any(|it| nx.refs.contains(&(Some(krate.to_string()), it.clone())) || (nx.krate == krate && nx.refs.contains(&(None, it.clone()))));
        let users: Vec<usize> = repo
            .nodes
            .iter()
            .enumerate()
            .filter(|&(x, nx)| {
                x != start
                    && repo.may_use(&nx.krate, krate)
                    && !(self.products && repo.test_only.contains(&(nx.krate.clone(), nx.name.clone())))
                    &&(nx.refs.contains(&(Some(krate.to_string()), name.to_string())) || (nx.krate == krate && nx.refs.contains(&(None, name.to_string()))) || names_item(nx))
            })
            .map(|(x, _)| x)
            .collect();
        let label = |i: usize| format!("{}::{}", repo.nodes[i].krate, repo.nodes[i].name);
        self.modules.insert((krate.to_string(), name.to_string()));
        self.modules.extend(users.iter().map(|&i| (repo.nodes[i].krate.clone(), repo.nodes[i].name.clone())));
        self.notes.push(format!("{file}: DIRECT tier: module {} and its {} one-hop users: {}", label(start), users.len(), users.iter().map(|&i| label(i)).collect::<Vec<_>>().join(", ")));
        // Modules whose namers are selected: the changed one, and users that are neither hubs nor test support.
        let mut named: Vec<usize> = vec![start];
        named.extend(users.iter().copied().filter(|&i| !repo.hubs.contains(&i) && !repo.test_only.contains(&(repo.nodes[i].krate.clone(), repo.nodes[i].name.clone()))));
        let mut sections: BTreeSet<u32> = BTreeSet::new();
        sections.extend(ns.section);
        sections.extend(users.iter().filter_map(|&i| repo.nodes[i].section));
        let secs: Vec<String> = sections.iter().map(u32::to_string).collect();
        let via = |t: &str| format!("{file}: {t}");
        add(&mut self.clippy, krate.to_string(), &format!("{file} changed"));
        let mut crates_in: BTreeSet<&str> = users.iter().map(|&i| repo.nodes[i].krate.as_str()).collect();
        crates_in.insert(krate);
        for c in crates_in {
            add(&mut self.libs, c.to_string(), &via("unit tests of the crate of the module or of a one-hop user"));
        }
        let eligible = repo.dependents(krate);
        for t in repo.targets.iter().filter(|t| eligible.contains(&t.krate)) {
            let why = if let Some((_, r)) = t.refs.iter().find(|(c, m)| named.iter().any(|&i| repo.nodes[i].name == *m && c.as_ref().is_none_or(|c| *c == repo.nodes[i].krate)) || (c.as_deref() == Some(krate) && items.contains(m))) {
                via(&format!("names `{r}` (the module, a name it re-exports or a one-hop user)"))
            } else if let Some(s) = t.sections.intersection(&sections).next() {
                via(&format!("spec section {s} (sections of the module and its one-hop users: {})", secs.join(",")))
            } else {
                continue;
            };
            add(&mut self.tests, (t.krate.clone(), t.name.clone()), &why);
        }
        // GATE-TIER-3b: a test that compiles C support files it names (`oracle/aio_diff.c`) and diffs their printed
        // output runs the changed module when such a file calls an output entry (`ORACLE_OUTPUT`) defined in the module
        // or whose body names it (not an entry hub): a printf change broke aio_products. One-hop users are not followed
        // (they took the replay to 4507 targets); a core-only printf change reaches the oracle diffs only in FULL, as on next. GATE-TIER-3
        // narrowed an oracle file's change to the tests naming it, which had hidden this reach behind the directory rule.
        // Other entries such a file calls (`memcpy`, `fork`) are helpers, left to the wide tier as for conformance programs.
        let calls_in: BTreeSet<&str> = repo
            .entries
            .iter()
            .filter(|(sym, _)| ORACLE_OUTPUT.contains(&sym.as_str()))
            .filter(|(_, es)| es.iter().any(|e| e.node == start || (!repo.entry_hubs.contains(&start) && e.refs.iter().any(|(m_c, m)| *m == ns.name && m_c.as_deref().unwrap_or(repo.nodes[e.node].krate.as_str()) == krate))))
            .map(|(s, _)| s.as_str())
            .collect();
        if !calls_in.is_empty() {
            for t in repo.targets.iter().filter(|t| eligible.contains(&t.krate) && !t.support.is_empty()) {
                if let Some((p, sym)) = support_c_call(repo, t, &calls_in) {
                    add(&mut self.tests, (t.krate.clone(), t.name.clone()), &via(&format!("its support file {p} calls `{sym}`, whose entry is in the module or names it")));
                }
            }
        }
        // GATE-TIER-2: a conformance program calling a C entry point that is defined in the changed module or whose
        // body names the changed module or a one-hop user in another crate (a hub or test-support user excluded, as for
        // the targets; a module most entry bodies name is followed only when it changed). Same-crate users are left to
        // the spec sections: following them selected nearly every program for a stdio change. The entry's module must
        // also share a spec section with the program: a program of another section (or of none: native_v1, x_overrides)
        // that only uses the entry as a helper (`fclose` in s00_errno) is left to the wide and full tiers, as before.
        // GATE-TIER-3: a cross-crate user is followed for a program only when the user carries one of the program's
        // sections, or carries none while the changed module carries one: a sectionless core module (`pattern`) reached
        // sectionless boundary helpers (`process`, `locale`) whose entries (`_exit`, `freelocale`) pulled in s01/s07/s23.
        // Only the user's `posix/sNN` file section counts (a header naming §1 in passing, as `process` does, does not);
        // a user without one is followed when the changed module's own sections meet the program's.
        let follow = |i: usize, c: &Conf| match repo.nodes[i].section {
            Some(s) => c.sections.contains(&s),
            None => !ns.hdr.is_disjoint(&c.sections),
        };
        let reached = |e: &Entry, c: &Conf| e.node == start || e.refs.iter().any(|(m_c, m)| named.iter().any(|&i| (i == start || (repo.nodes[i].krate != krate && !repo.entry_hubs.contains(&i) && follow(i, c))) && repo.nodes[i].name == *m && repo.nodes[i].krate == *m_c.as_deref().unwrap_or(repo.nodes[e.node].krate.as_str())));
        for c in &repo.conf {
            if let Some(s) = c.sections.intersection(&sections).next() {
                add(&mut self.conf, c.name.clone(), &via(&format!("spec section {s} (sections of the module and its one-hop users: {})", secs.join(","))));
            } else if let Some(sym) = c.calls.iter().find(|s| repo.entries[*s].iter().any(|e| reached(e, c) && repo.nodes[e.node].section.is_some_and(|x| c.sections.contains(&x)))) {
                add(&mut self.conf, c.name.clone(), &via(&format!("calls `{sym}`, whose entry is in the module or names it or a one-hop user")));
            }
        }
    }
}

enum Class {
    Full(&'static str),
    Docs,
    Nothing(&'static str),
    AllConf,
    XtaskOwn,
    /// bench/ and benchmarks/: data the xtask unit tests read (limits, rows, sentinels), so `-p xtask` runs.
    XtaskData,
    XtaskAudit(&'static [&'static str]),
}

/// xtask modules that only implement report or audit commands: (module, the xtask commands to
/// run when it changes). `policy.rs` (purity, unsafe-audit, csys-audit) runs in every selection
/// anyway. A module here must not be used by another xtask module outside this table, `main.rs`
/// and this command (the unit test `audit_modules_are_leaves` checks it); every other xtask
/// change escalates to FULL.
const XTASK_AUDITS: &[(&str, &[&str])] = &[
    ("xtask/src/policy.rs", &[]),
    ("xtask/src/forkalloc.rs", &[]),
    ("xtask/src/symgap.rs", &["symgap"]),
    ("xtask/src/status.rs", &["status"]),
    ("xtask/src/schedule.rs", &["status"]),
];

fn classify(path: &str) -> Option<Class> {
    let file = path.rsplit('/').next().unwrap_or(path);
    let full = |why: &'static str| Some(Class::Full(why));
    if path == "xtask/src/affected.rs" {
        return Some(Class::XtaskOwn);
    }
    if let Some((_, cmds)) = XTASK_AUDITS.iter().find(|(m, _)| *m == path) {
        return Some(Class::XtaskAudit(cmds));
    }
    if path.starts_with("xtask/") {
        return full("xtask (its commands are part of the gate)");
    }
    if path.starts_with("csys/src/") && path.ends_with(".S") {
        return None;
    }
    if path.starts_with("csys/") {
        return full("csys/ (the C layer under everything; only its assembly sources are mapped)");
    }
    if path.starts_with("crates/abi/") {
        return full("crates/abi (the ABI types every crate shares)");
    }
    if file == "build.rs" || file == "Cargo.toml" || file == "Cargo.lock" || file.starts_with("rust-toolchain") || file == "clippy.toml" || path.starts_with(".cargo/") {
        return full("build configuration");
    }
    if matches!(path, "spec/clauses.tsv" | "spec/symbols.tsv" | "spec/primitives.tsv") || path.starts_with("spec/abi/") || path.starts_with("spec/unicode/") || path.starts_with("include/") {
        return full("generated table or its input");
    }
    if file.ends_with("_gen.rs") || file == "tables.rs" || file == "builtin_items.rs" || path.starts_with("crates/math/fits/") || path.starts_with("crates/math/src/coeffs/") {
        return full("generated source");
    }
    if path.starts_with("crates/libc-so/") || path.starts_with("crates/libm-so/") || path.starts_with("crates/native/link/") || path.starts_with("crates/native/examples/") {
        return full("a product crate or its linker input");
    }
    if path.ends_with(".map") || path.ends_with(".ld") || path.ends_with(".lds") || path.ends_with(".version") || path.starts_with("scripts/") {
        return full("linker or export list, or a script");
    }
    if path == "conformance/run.sh" || path.starts_with("conformance/tools/") {
        return Some(Class::AllConf);
    }
    if path.starts_with("docs/") || path.starts_with("plan/") || path.starts_with("handoff/") || path.starts_with("spec/behaviour/") || path == "spec/schedule.tsv" || path.starts_with("spec/tools/") || path == "spec/ERRATA.md" || path.starts_with("tools/") || (!path.contains('/') && path.ends_with(".md")) || file.starts_with("LICENSE") || file == ".gitignore" {
        return Some(Class::Docs);
    }
    if path.starts_with("bench/") || path.starts_with("benchmarks/") {
        return Some(Class::XtaskData);
    }
    if path.starts_with("fuzz/") || path.starts_with("validation/") {
        return Some(Class::Nothing("not part of the test gate (bench, fuzz and validation have their own commands)"));
    }
    None
}

/// The selection for a set of changed files (paths relative to the repository root, `/`).
#[cfg(test)]
pub fn select(repo: &Repo, files: &[String]) -> Selection {
    select_opts(repo, files, false)
}

/// `products`: follow no edge through a module compiled only under a test cfg (what a product can change).
pub fn select_opts(repo: &Repo, files: &[String], products: bool) -> Selection {
    select_with(repo, files, products, Tier::Wide)
}

/// The selection of a tier (GATE-TIER): `Direct` for per-fix gates, `Wide` the earlier rules, `Full` everything.
pub fn select_tier(repo: &Repo, files: &[String], tier: Tier) -> Selection {
    select_with(repo, files, false, tier)
}

fn select_with(repo: &Repo, files: &[String], products: bool, tier: Tier) -> Selection {
    let mut sel = Selection { products, direct: tier == Tier::Direct, ..Selection::default() };
    if tier == Tier::Full {
        sel.full.push("--tier full: the landing gate's selection (everything)".to_string());
    }
    for f in files {
        let f = f.as_str();
        // The fuzz harnesses (a workspace of their own, not in `cargo test`) must still compile when a library
        // crate or they themselves change (HYGIENE-1); seeds and scripts cannot break the build.
        if (f.starts_with("crates/") && f.ends_with(".rs") && !f.contains("/tests/")) || (f.starts_with("fuzz/") && (f.ends_with(".rs") || f.ends_with("Cargo.toml"))) {
            sel.xtask_cmds.insert("fuzz-check".to_string());
        }
        if f == "crates/boundary/src/ffi.rs" || f.starts_with("csys/include/") || f.starts_with("csys/src/machine/") {
            sel.k5 = true;
        }
        if f.starts_with("csys/include/") && f.ends_with(".h") && header_map(repo, f, &mut sel) {
            continue;
        }
        match classify(f) {
            Some(Class::Full(why)) => {
                if f.starts_with("xtask/") {
                    sel.xtask_tests = true;
                }
                sel.full.push(format!("{f}: {why}"));
            }
            Some(Class::Docs) => sel.notes.push(format!("{f}: docs only (stubs, trace, policy checks)")),
            Some(Class::Nothing(why)) => sel.notes.push(format!("{f}: {why}")),
            Some(Class::AllConf) => sel.all_conf(repo, &format!("{f}: every conformance program")),
            Some(Class::XtaskData) => {
                sel.xtask_tests = true;
                sel.notes.push(format!("{f}: bench data (the xtask unit tests read it)"));
            }
            Some(Class::XtaskOwn) => {
                sel.xtask_tests = true;
                sel.notes.push(format!("{f}: the affected command itself (its unit tests)"));
            }
            Some(Class::XtaskAudit(cmds)) => {
                sel.xtask_tests = true;
                add(&mut sel.clippy, "xtask".to_string(), &format!("{f} changed"));
                sel.xtask_cmds.extend(cmds.iter().map(|c| c.to_string()));
                sel.notes.push(format!("{f}: an audit/report module of xtask (xtask unit tests, clippy, the policy checks{})", cmds.iter().map(|c| format!(", `cargo xtask {c}`")).collect::<String>()));
            }
            None => {
                classify_path(repo, f, &mut sel);
                sel.own_suite(repo, f);
            }
        }
    }
    sel
}

/// `--only a,b,...`: the named binaries for a red/green run, NOT a gate. A name is a test
/// target (`name`, or `crate::name` when two crates have one), `conformance:<program>`,
/// `lib:<crate>` (its unit tests) or `xtask` (the xtask unit tests). An unknown or ambiguous
/// name is an error, never a guess.
pub fn select_only(repo: &Repo, list: &str) -> Result<Selection, String> {
    let mut sel = Selection { only: true, ..Selection::default() };
    for n in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if let Some(p) = n.strip_prefix("conformance:") {
            repo.conf.iter().find(|c| c.name == p).ok_or(format!("--only: no conformance program `{p}`"))?;
            add(&mut sel.conf, p.to_string(), "named (--only)");
        } else if let Some(c) = n.strip_prefix("lib:") {
            repo.crates.iter().find(|x| x.name == c).ok_or(format!("--only: no crate `{c}`"))?;
            add(&mut sel.libs, c.to_string(), "named (--only)");
        } else if n == "xtask" {
            sel.xtask_tests = true;
        } else {
            let (krate, name) = n.split_once("::").map_or((None, n), |(c, t)| (Some(c), t));
            let hits: Vec<&Target> = repo.targets.iter().filter(|t| t.name == name && krate.is_none_or(|c| t.krate == c)).collect();
            match hits.as_slice() {
                [t] => add(&mut sel.tests, (t.krate.clone(), t.name.clone()), "named (--only)"),
                [] => return Err(format!("--only: no test target `{n}`")),
                _ => return Err(format!("--only: `{n}` is a target of several crates ({}); name it as crate::target", hits.iter().map(|t| t.krate.as_str()).collect::<Vec<_>>().join(", "))),
            }
        }
    }
    if sel.tests.is_empty() && sel.conf.is_empty() && sel.libs.is_empty() && !sel.xtask_tests {
        return Err("--only: nothing named".into());
    }
    Ok(sel)
}

fn classify_path(repo: &Repo, f: &str, sel: &mut Selection) {
    if let Some(name) = f.strip_prefix("conformance/c/") {
        let stem = name.strip_suffix(".c").unwrap_or(name);
        if f.ends_with(".c") {
            add(&mut sel.conf, stem.to_string(), &format!("{f} changed"));
            for t in repo.targets.iter().filter(|t| t.text.contains(stem)) {
                add(&mut sel.tests, (t.krate.clone(), t.name.clone()), &format!("{f}: the test names the program"));
            }
        } else if f.ends_with(".h") && !name.contains('/') {
            // A shared header: the programs that include it (none found: every program).
            let users: Vec<&Conf> = repo.conf.iter().filter(|c| c.includes.contains(name)).collect();
            if users.is_empty() {
                sel.all_conf(repo, &format!("{f}: a header no program includes by name (every conformance program)"));
            }
            for c in users {
                add(&mut sel.conf, c.name.clone(), &format!("{f}: the program includes it"));
            }
            for t in repo.targets.iter().filter(|t| t.text.contains(name)) {
                add(&mut sel.tests, (t.krate.clone(), t.name.clone()), &format!("{f}: the test names the header"));
            }
        } else {
            sel.full.push(format!("{f}: unknown conformance file"));
        }
        return;
    }
    if f.starts_with("csys/src/") {
        return csys_map(repo, f, sel);
    }
    let Some(c) = repo.crate_of(f) else {
        sel.full.push(format!("{f}: not a path the selection knows (run everything)"));
        return;
    };
    let (cname, rest) = (c.name.clone(), &f[c.dir.len() + 1..]);
    if let Some(inner) = rest.strip_prefix("src/") {
        if !inner.ends_with(".rs") {
            sel.full.push(format!("{f}: non-Rust file under src (a generator input or data table)"));
            return;
        }
        let parts: Vec<&str> = inner.split('/').collect();
        let name = if cname == "liella-boundary" && parts.len() >= 2 && parts[0] == "posix" {
            parts[1].trim_end_matches(".rs").to_string()
        } else {
            parts[0].trim_end_matches(".rs").to_string()
        };
        let root = parts.len() == 1 && (name == "lib" || name == "main");
        if sel.direct && !root {
            sel.narrow_direct(repo, &cname, &name, f);
        } else if !NARROW.contains(&cname.as_str()) || root {
            let why = if root { format!("{f}: crate root file (wide)") } else { format!("{f}: `{cname}` is shared infrastructure (wide)") };
            sel.wide(repo, &cname, &why);
        } else {
            sel.narrow(repo, &cname, &name, f);
        }
        return;
    }
    if let Some(inner) = rest.strip_prefix("tests/") {
        add(&mut sel.clippy, cname.clone(), &format!("{f} changed"));
        let parts: Vec<&str> = inner.split('/').collect();
        let own = if parts.len() == 1 { parts[0].trim_end_matches(".rs") } else { parts[0] };
        let namers = support_namers(repo, &c.dir, &cname, &parts, f);
        let mut found = false;
        for t in repo.targets.iter().filter(|t| t.krate == cname) {
            if t.name == own {
                add(&mut sel.tests, (cname.clone(), t.name.clone()), &format!("{f} changed"));
                found = true;
            }
            if parts.len() > 1 && t.support.contains(parts[0]) {
                if let Some(n) = &namers {
                    if n.contains(&t.name) {
                        add(&mut sel.tests, (cname.clone(), t.name.clone()), &format!("{f}: the test names the file (or a file of `{}` that names it)", parts[0]));
                    }
                } else {
                    add(&mut sel.tests, (cname.clone(), t.name.clone()), &format!("{f}: the test names the directory `{}`", parts[0]));
                }
                found = true;
            }
            // Another target that includes the file (`#[path = "x.rs"] mod`, `include!`, `include_str!`).
            if parts.len() == 1 && t.name != own && t.text.lines().any(|l| l.contains(parts[0]) && ["#[path", "include!(", "include_str!(", "include_bytes!("].iter().any(|k| l.contains(k))) {
                add(&mut sel.tests, (cname.clone(), t.name.clone()), &format!("{f}: the test names the file"));
            }
        }
        if !found {
            // A deleted or unknown test file: its crate's whole test set.
            for t in repo.targets.iter().filter(|t| t.krate == cname) {
                add(&mut sel.tests, (cname.clone(), t.name.clone()), &format!("{f}: cannot place the file (all tests of the crate)"));
            }
        }
        return;
    }
    if rest.starts_with("examples/") || rest.starts_with("benches/") || rest.ends_with(".md") {
        add(&mut sel.clippy, cname.clone(), &format!("{f} changed"));
        return;
    }
    sel.full.push(format!("{f}: unknown file of crate `{cname}`"));
}

/// GATE-TIER-3: the targets of `krate` whose own source names a non-Rust file `parts` of a `tests/` support directory
/// (`oracle/aio_diff.c`), by file stem, or names a non-Rust file of that directory that names it (`aio_run.sh`).
/// `None` (every target naming the directory, as before) for a Rust file (reached through `mod`), a file a Rust file
/// of the directory names, or a file no target's own source names.
fn support_namers(repo: &Repo, dir: &str, krate: &str, parts: &[&str], f: &str) -> Option<BTreeSet<String>> {
    if parts.len() < 2 || f.ends_with(".rs") {
        return None;
    }
    let stem = |p: &str| -> String {
        let file = p.rsplit('/').next().unwrap_or(p);
        file.split('.').next().unwrap_or(file).to_string()
    };
    let prefix = format!("{dir}/tests/{}/", parts[0]);
    let mut stems = BTreeSet::from([stem(f)]);
    for (p, t) in repo.support_texts.range(prefix.clone()..).take_while(|(p, _)| p.starts_with(&prefix)) {
        if p != f && t.contains(&stem(f)) {
            if p.ends_with(".rs") {
                return None;
            }
            stems.insert(stem(p));
        }
    }
    let namers: BTreeSet<String> = repo.targets.iter().filter(|t| t.krate == krate && t.support.contains(parts[0]) && stems.iter().any(|s| t.text[..t.own].contains(s.as_str()))).map(|t| t.name.clone()).collect();
    (!namers.is_empty()).then_some(namers)
}

/// GATE-TIER-3b: the C entry points through which an oracle program prints the output its test compares.
const ORACLE_OUTPUT: &[&str] = &["printf", "fprintf", "vprintf", "vfprintf", "dprintf", "puts", "fputs", "putchar", "fputc", "putc", "fwrite", "fflush"];

/// GATE-TIER-3b: the first non-Rust file of `t`'s support directories that its own source names by stem and that
/// calls one of `syms` (`name(`) (path, symbol).
fn support_c_call<'a>(repo: &'a Repo, t: &Target, syms: &BTreeSet<&'a str>) -> Option<(&'a str, &'a str)> {
    let dir = &repo.crates.iter().find(|c| c.name == t.krate)?.dir;
    let own = &t.text[..t.own];
    for s in &t.support {
        let prefix = format!("{dir}/tests/{s}/");
        for (p, text) in repo.support_texts.range(prefix.clone()..).take_while(|(p, _)| p.starts_with(&prefix)) {
            let file = p.rsplit('/').next().unwrap_or(p);
            let stem = file.split('.').next().unwrap_or(file);
            if p.ends_with(".rs") || stem.is_empty() || !own.contains(stem) {
                continue;
            }
            if let Some(sym) = syms.iter().find(|w| text.match_indices(&format!("{w}(")).any(|(i, _)| !text[..i].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))).copied() {
                return Some((p.as_str(), sym));
            }
        }
    }
    None
}

/// The crate whose build script compiles `csys/` and links it (checked by a unit test).
const CSYS_CRATE: &str = "liella-boundary";

/// An assembly source of csys: its symbols are followed to the Rust files (and conformance
/// programs) that use them, and each user is selected as if it had changed. FULL whenever the
/// file cannot be mapped: new or deleted, generated, a macro or include that can define symbols,
/// a symbol that may be part of the exported C ABI (not `liella_`), a symbol used by another
/// csys source, a link list, xtask, or a C header other than as a prototype, or no user at all.
/// The `liella_*` function names whose one-line declarations are all that the diff changes in `file`. `None` when
/// any changed line is something else (a struct, a macro, a multi-line declaration, a type) or nothing is found:
/// a structural header change cannot be mapped to symbols.
fn header_symbols(diff: &str, file: &str) -> Option<Vec<String>> {
    let mut inside = false;
    let mut out: BTreeSet<String> = BTreeSet::new();
    for l in diff.lines() {
        if l.starts_with("diff --git ") {
            inside = l.ends_with(&format!(" b/{file}"));
        } else if inside && !l.starts_with("+++") && !l.starts_with("---") && (l.starts_with('+') || l.starts_with('-')) {
            let t = l[1..].trim();
            if t.is_empty() || t.starts_with("/*") || t.starts_with("//") || t.starts_with('*') || t.ends_with("*/") {
                continue;
            }
            let (head, _) = t.split_once('(')?;
            let name = head.trim_end().rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next()?;
            if !t.ends_with(';') || !name.starts_with("liella_") || t.contains('{') || head.starts_with('#') {
                return None;
            }
            out.insert(name.to_string());
        }
    }
    if out.is_empty() { None } else { Some(out.into_iter().collect()) }
}

/// TOOLS-7 (3): a changed `csys/include/*.h` whose diff only adds, removes or edits one-line declarations of symbols
/// that csys assembly sources define is selected like those sources' symbols (their Rust and conformance users, the
/// tests naming the header), not FULL. `false` (the caller escalates) for anything it cannot map.
fn header_map(repo: &Repo, f: &str, sel: &mut Selection) -> bool {
    let Some(symbols) = repo.hdr_diff.as_deref().and_then(|d| header_symbols(d, f)) else { return false };
    let mut users: BTreeSet<&str> = BTreeSet::new();
    for s in &symbols {
        let Some(refs) = repo.csys_refs.get(s) else { return false };
        for r in refs.iter().filter(|r| r.file != f) {
            if r.decl && (r.file.ends_with(".rs") || r.file.starts_with("csys/include/") || r.file.starts_with("include/")) {
                continue;
            }
            let rust = r.file.starts_with("crates/") && r.file.ends_with(".rs");
            let program = r.file.starts_with("conformance/c/") && r.file.ends_with(".c");
            if !(rust || program || r.file.starts_with("csys/src/") && r.file.ends_with(".S")) {
                return false;
            }
            if rust || program {
                users.insert(&r.file);
            }
        }
    }
    if users.is_empty() {
        return false;
    }
    sel.notes.push(format!("{f}: only declarations of {} changed; used by {}; each is selected as if it changed, plus K5", symbols.join(", "), users.iter().copied().collect::<Vec<_>>().join(", ")));
    add(&mut sel.clippy, CSYS_CRATE.to_string(), &format!("{f} changed (compiled by its build script)"));
    let file = f.rsplit('/').next().unwrap_or(f);
    for t in repo.targets.iter().filter(|t| t.text.contains(file)) {
        add(&mut sel.tests, (t.krate.clone(), t.name.clone()), &format!("{f}: the test names the header"));
    }
    for u in users {
        csys_user(repo, f, u, &symbols, sel);
    }
    true
}

/// TOOLS-21 (coordinator decision, DIRECT tier only): the exported C-ABI symbols defined in `csys/src/machine/` whose
/// behaviour the machine tests pin, each with the test targets (crate `liella-boundary`) that call it. In the direct
/// tier a listed symbol selects these targets plus the conformance programs that name it instead of escalating to FULL
/// (rule 6); any other non-`liella_` symbol stays FULL, and `--tier full` / the wide tier are unchanged.
const MACHINE_EXPORTS: &[(&str, &str, &[&str])] = &[
    ("once.S", "pthread_once", &["s22_threads_symbols"]),
    ("once.S", "__pthread_once", &["s22_threads_symbols"]),
    ("once.S", "call_once", &["s22_threads_symbols"]),
    ("vfork.S", "vfork", &["s18_processes_symbols"]),
    ("vfork.S", "__vfork", &["s18_processes_symbols"]),
    ("fold.S", "memmove", &["machine_scanners", "small_copy", "s04_strings_symbols"]),
    ("fold.S", "memcpy", &["machine_scanners", "small_copy", "s04_strings_symbols"]),
    ("fold.S", "mempcpy", &["small_copy", "s04_strings_symbols"]),
    ("fold.S", "__mempcpy", &["s04_strings_symbols"]),
    ("fold.S", "memset", &["machine_scanners", "small_copy", "s04_strings_symbols"]),
    ("fold.S", "memcmp", &["machine_scanners", "scanner_races", "small_copy", "s04_strings_symbols"]),
    ("fold.S", "bcmp", &["machine_scanners", "s04_strings_symbols"]),
    ("fold.S", "__memcmpeq", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "memchr", &["machine_scanners", "scanner_races", "s04_strings_symbols"]),
    ("scan.S", "strcmp", &["machine_scanners", "scanner_races", "s04_strings_symbols"]),
    ("scan.S", "rawmemchr", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "strchr", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "index", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "strchrnul", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "strncmp", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "__rawmemchr", &["s04_strings_symbols"]),
    ("scan.S", "memrchr", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "strcpy", &["machine_scanners", "s04_strings_symbols"]),
    ("scan.S", "__strcpy_chk", &["s04_strings_symbols"]),
    ("scan.S", "stpcpy", &["s04_strings_symbols"]),
    ("scan.S", "__stpcpy", &["s04_strings_symbols"]),
    ("scan.S", "__stpcpy_chk", &["s04_strings_symbols"]),
    ("scan.S", "strncpy", &["s04_strings_symbols"]),
    ("scan.S", "__strncpy_chk", &["s04_strings_symbols"]),
    ("scan.S", "stpncpy", &["s04_strings_symbols"]),
    ("scan.S", "__stpncpy", &["s04_strings_symbols"]),
    ("scan.S", "__stpncpy_chk", &["s04_strings_symbols"]),
];

/// CSYS-MAP-2 (`cargo xtask static`, check `machine-exports`): every exported (non-`liella_`) symbol that a file named in
/// `MACHINE_EXPORTS` defines must be listed there, mapped to existing `liella-boundary` targets that name it. One-sided on
/// purpose: an entry whose symbol the file does not define (yet) is allowed, so the map can land before or after the
/// branch that adds the export; the developer adding an export to a listed file gets this message in the same commit.
/// Files not in the list (setjmp.S, context.S, entry.S, ...) are not checked: their exports stay FULL by design.
pub fn machine_exports_check(root: &Path) -> Result<(), String> {
    machine_exports_problems(&Repo::load(root)).map_or(Ok(()), |p| Err(p.join("\n")))
}

fn machine_exports_problems(repo: &Repo) -> Option<Vec<String>> {
    let files: BTreeSet<&str> = MACHINE_EXPORTS.iter().map(|(f, _, _)| *f).collect();
    let mut out = Vec::new();
    for file in files {
        let path = format!("csys/src/machine/{file}");
        let Some(c) = repo.csys.get(&path) else { continue };
        let Some(symbols) = c.symbols.as_ref() else {
            out.push(format!("{path}: its symbols cannot be read (see csys_symbols); MACHINE_EXPORTS cannot map it"));
            continue;
        };
        for s in symbols.iter().filter(|s| !s.starts_with("liella_")) {
            match MACHINE_EXPORTS.iter().find(|(f, x, _)| *f == file && x == s) {
                None => out.push(format!("{path} defines exported `{s}`, which MACHINE_EXPORTS (xtask/src/affected.rs) lacks: add (\"{file}\", \"{s}\", &[<liella-boundary test targets that call it>]) in the same commit")),
                Some((_, _, [])) => out.push(format!("{path}: MACHINE_EXPORTS maps `{s}` to no test target")),
                Some((_, _, targets)) => {
                    for t in targets.iter() {
                        let tgt = repo.targets.iter().find(|x| x.krate == CSYS_CRATE && x.name == *t);
                        if !tgt.is_some_and(|x| is_word(&x.text, s)) {
                            out.push(format!("{path}: MACHINE_EXPORTS maps `{s}` to {t}, which does not exist in {CSYS_CRATE} or does not name `{s}`"));
                        }
                    }
                }
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// CSYS-MAP-4 (`cargo xtask machine-exports --list [<tree>]`): every exported (non-`liella_`) symbol any `csys/src/machine/*.S`
/// of the tree defines that `MACHINE_EXPORTS` does not map, one line each, `file symbol (listed file|unlisted file: FULL by design)`.
/// Same symbol reading and same map lookup as the `machine-exports` static check; informational (exit 0), runnable on any tree.
pub fn machine_exports_list(root: &Path) -> Result<(), String> {
    let repo = Repo::load(root);
    let mut n = 0;
    for (path, c) in repo.csys.iter().filter(|(p, _)| p.starts_with("csys/src/machine/") && p.ends_with(".S")) {
        let file = path.trim_start_matches("csys/src/machine/");
        let Some(symbols) = c.symbols.as_ref() else {
            println!("{file} (symbols unreadable statically: macro/include/.set; every export stays FULL)");
            continue;
        };
        let listed = MACHINE_EXPORTS.iter().any(|(f, _, _)| *f == file);
        for s in symbols.iter().filter(|s| !s.starts_with("liella_") && !MACHINE_EXPORTS.iter().any(|(f, x, _)| *f == file && x == s)) {
            n += 1;
            println!("{file} {s} ({} file)", if listed { "listed" } else { "unlisted" });
        }
    }
    println!("{n} exported symbol(s) not in MACHINE_EXPORTS");
    Ok(())
}

/// The fixed test targets of exported symbol `s` defined in `f` (`MACHINE_EXPORTS`); `None`: not listed (FULL).
fn machine_export(f: &str, s: &str) -> Option<&'static [&'static str]> {
    let file = f.strip_prefix("csys/src/machine/")?;
    MACHINE_EXPORTS.iter().find(|(x, y, t)| *x == file && *y == s && !t.is_empty()).map(|(_, _, t)| *t)
}

fn csys_map(repo: &Repo, f: &str, sel: &mut Selection) {
    csys_map_in(repo, f, sel, &mut BTreeSet::new());
}

/// `csys_map` for `f`; `seen` holds the machine files already mapped (a sibling `.S` naming `f`'s symbols is mapped as
/// if it changed, direct tier only, TOOLS-21).
fn csys_map_in(repo: &Repo, f: &str, sel: &mut Selection, seen: &mut BTreeSet<String>) {
    seen.insert(f.to_string());
    let direct = sel.direct;
    let mut siblings: BTreeSet<&str> = BTreeSet::new();
    let mut full = |why: String| sel.full.push(format!("{f}: {why}"));
    let Some(cf) = repo.csys.get(f) else {
        return full("a csys file not in the tree (new or deleted): cannot map its symbols".into());
    };
    if cf.generated {
        return full("generated by `cargo xtask csys-gen` (its check runs in the full gate)".into());
    }
    let Some(symbols) = cf.symbols.as_ref().filter(|s| !s.is_empty()) else {
        return full("a csys source whose symbols cannot be read statically (C, or a macro/include/.set)".into());
    };
    let mut users: BTreeSet<&str> = BTreeSet::new();
    let mut exported: Vec<(&str, &[&str])> = Vec::new();
    for s in symbols {
        if !s.starts_with("liella_") {
            match machine_export(f, s).filter(|_| sel.direct) {
                Some(t) => {
                    exported.push((s, t));
                    continue;
                }
                None => return full(format!("defines `{s}`, which may be part of the exported C ABI")),
            }
        }
        for r in repo.csys_refs.get(s).into_iter().flatten().filter(|r| r.file != f) {
            let header = r.file.starts_with("csys/include/") || r.file.starts_with("include/");
            if r.decl && (r.file.ends_with(".rs") || header) {
                continue;
            }
            let rust = r.file.starts_with("crates/") && r.file.ends_with(".rs");
            let program = r.file.starts_with("conformance/c/") && r.file.ends_with(".c");
            let sibling = r.file.starts_with("csys/src/machine/") && r.file.ends_with(".S");
            if sibling && direct && f.starts_with("csys/src/machine/") {
                siblings.insert(&r.file);
                continue;
            }
            if !(rust || program) {
                return full(format!("`{s}` is named by `{}` (not a Rust or conformance source)", r.file));
            }
            users.insert(&r.file);
        }
    }
    if users.is_empty() && exported.is_empty() && siblings.is_empty() {
        return full("no Rust or conformance source uses its symbols".into());
    }
    for (s, targets) in &exported {
        for t in targets.iter() {
            add(&mut sel.tests, (CSYS_CRATE.to_string(), t.to_string()), &format!("{f}: `{s}` is a listed machine export (TOOLS-21)"));
        }
        for r in repo.csys_refs.get(*s).into_iter().flatten().filter(|r| !r.decl && r.file.starts_with("conformance/c/") && r.file.ends_with(".c")) {
            let stem = r.file["conformance/c/".len()..].trim_end_matches(".c");
            add(&mut sel.conf, stem.to_string(), &format!("{f}: the program calls the listed machine export `{s}`"));
        }
    }
    if !exported.is_empty() {
        sel.notes.push(format!("{f}: listed machine exports ({}) select their fixed machine tests and calling conformance programs", exported.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", ")));
    }
    sel.notes.push(format!("{f}: its symbols ({}) are used by {}; each is selected as if it changed", symbols.join(", "), users.iter().copied().collect::<Vec<_>>().join(", ")));
    add(&mut sel.clippy, CSYS_CRATE.to_string(), &format!("{f} changed (compiled by its build script)"));
    let file = f.rsplit('/').next().unwrap_or(f);
    for t in repo.targets.iter().filter(|t| t.text.contains(file)) {
        add(&mut sel.tests, (t.krate.clone(), t.name.clone()), &format!("{f}: the test names the file"));
    }
    for u in users {
        csys_user(repo, f, u, symbols, sel);
    }
    for s in siblings {
        if !seen.contains(s) {
            sel.notes.push(format!("{f}: its symbols are named by the machine file {s}, mapped as if it changed (TOOLS-21)"));
            csys_map_in(repo, s, sel, seen);
        }
    }
}

/// Whether a definition of function `w` in rustfmt-formatted `text` sits in a trait or a trait
/// impl block (an enclosing, less indented line starting `impl ... for` or `trait`).
fn in_trait(text: &str, w: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let indent = |l: &str| l.len() - l.trim_start().len();
    lines.iter().enumerate().filter(|(_, l)| l.contains(&format!("fn {w}(")) || l.contains(&format!("fn {w}<"))).any(|(i, l)| {
        let mut ind = indent(l);
        for m in lines[..i].iter().rev().filter(|m| !m.trim().is_empty()) {
            if indent(m) < ind {
                ind = indent(m);
                let t = m.trim_start().trim_start_matches("pub ").trim_start_matches("unsafe ");
                if (t.starts_with("impl") && t.contains(" for ")) || t.starts_with("trait ") {
                    return true;
                }
            }
        }
        false
    })
}

fn is_word(text: &str, w: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|x| x == w)
}

/// A function span of a rustfmt-formatted source: name, first and last line, C entry point.
type Span = (String, usize, usize, bool);

/// The functions of a rustfmt-formatted `text` that name one of `words`, closed over the other
/// functions of the same text that name those (a helper calling the user is affected too).
/// Spans follow indentation: a `fn` line to the first `}` line at its indentation. `None` when
/// unsure: an occurrence outside every function, a span that does not close, or an affected
/// function that is a C entry point (`extern "C"`, `no_mangle`, `export_name`, `export!`),
/// which C reaches without a Rust caller.
fn affected_fns(text: &str, words: &[String]) -> Option<BTreeSet<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let indent = |l: &str| l.len() - l.trim_start().len();
    let fn_name = |l: &str| -> Option<String> {
        let t = l.trim_start();
        let i = t.find("fn ")?;
        if !t[..i].split_whitespace().all(|w| matches!(w, "pub" | "pub(crate)" | "pub(super)" | "const" | "unsafe" | "safe" | "async" | "extern" | "\"C\"" | "\"C-unwind\"")) {
            return None;
        }
        let name: String = t[i + 3..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        (!name.is_empty()).then_some(name)
    };
    let mut spans: Vec<Span> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(name) = fn_name(l) else { continue };
        if l.trim_end().ends_with(';') {
            continue;
        }
        let ind = indent(l);
        let mut end = None;
        if l.trim_end().ends_with('}') && l.contains('{') {
            end = Some(i);
        } else {
            for (k, m) in lines.iter().enumerate().skip(i + 1) {
                if m.trim().is_empty() {
                    continue;
                }
                if indent(m) < ind {
                    return None;
                }
                if indent(m) == ind && m.trim_start().starts_with('}') {
                    end = Some(k);
                    break;
                }
            }
        }
        let end = end?;
        let attrs = lines[..i].iter().rev().take_while(|a| a.trim_start().starts_with("#[") || a.trim_start().starts_with("///")).any(|a| a.contains("no_mangle") || a.contains("export_name"));
        spans.push((name, i, end, attrs || l.contains("extern \"C") || l.contains("export!")));
    }
    let mut affected: BTreeSet<String> = BTreeSet::new();
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with("//") || !words.iter().any(|w| is_word(l, w)) {
            continue;
        }
        let around: Vec<&Span> = spans.iter().filter(|s| s.1 <= i && i <= s.2).collect();
        if around.is_empty() {
            return None;
        }
        affected.extend(around.iter().map(|s| s.0.clone()));
    }
    loop {
        let before = affected.len();
        for s in &spans {
            if !affected.contains(&s.0) && lines[s.1..=s.2].iter().any(|l| !l.trim_start().starts_with("//") && affected.iter().any(|w| is_word(l, w))) {
                affected.insert(s.0.clone());
            }
        }
        if affected.len() == before {
            break;
        }
    }
    if spans.iter().any(|s| s.3 && affected.contains(&s.0)) || affected.iter().any(|w| text.lines().any(|l| l.contains("export!") && is_word(l, w))) {
        return None;
    }
    Some(affected)
}

/// A Rust user `u` of the csys symbols of `f`. In a helper module (no spec section) of a
/// narrowed crate the users are followed at function granularity: the functions that call the
/// symbols (closed within the module), then every module and test that names one of those
/// functions, each module selected as if it changed. Anything else, or when unsure: `u` as if
/// it changed (rules 4 and 5).
fn csys_user(repo: &Repo, f: &str, u: &str, symbols: &[String], sel: &mut Selection) {
    let fallback = |sel: &mut Selection, why: &str| {
        sel.notes.push(format!("{f}: {u} followed as a changed file ({why})"));
        classify_path(repo, u, sel);
    };
    let Some(c) = repo.crate_of(u) else { return fallback(sel, "not in a crate") };
    let Some(inner) = u[c.dir.len() + 1..].strip_prefix("src/") else { return fallback(sel, "not under src") };
    let name = inner.split('/').next().unwrap_or(inner).trim_end_matches(".rs");
    if !NARROW.contains(&c.name.as_str()) || (c.name == "liella-boundary" && name == "posix") {
        return fallback(sel, "not a helper module of a narrowed crate");
    }
    let Some(node) = repo.nodes.iter().find(|n| n.krate == c.name && n.name == name && !n.root && n.section.is_none()) else {
        return fallback(sel, "a crate root or not a module");
    };
    let Some(fns) = affected_fns(&node.text, symbols) else { return fallback(sel, "its functions could not be delimited, or one is a C entry point") };
    let fn_list: Vec<String> = fns.into_iter().collect();
    // A trait method (`impl T for X`, or a default body in `trait T`) is called through the
    // trait without naming this module, which neither the module graph nor a name search sees.
    if let Some(w) = fn_list.iter().find(|w| in_trait(&node.text, w)) {
        sel.full.push(format!("{f}: {u}: `{w}` uses its symbols and is a trait method (reached through the trait, not by name)"));
        return;
    }
    for root in repo.nodes.iter().filter(|r| r.root && r.krate == node.krate) {
        if root.reexports.iter().any(|st| st.contains(&node.name) || fn_list.iter().any(|w| st.contains(w))) {
            return fallback(sel, "re-exported at its crate root");
        }
    }
    // Another crate can call only a `pub` function (not `pub(crate)` or private); a macro body
    // (`macro_rules!`) expands in its users' crates, so one in the module counts as public.
    let public = node.text.contains("macro_rules!") || node.text.lines().map(str::trim_start).any(|l| l.starts_with("pub ") && fn_list.iter().any(|w| l.contains(&format!("fn {w}(")) || l.contains(&format!("fn {w}<"))));
    // A caller names the module (as the module graph sees it: `sys::`, or a word of a `use`)
    // and one of the functions.
    let names_module = |x: &Node| x.refs.contains(&(Some(node.krate.clone()), node.name.clone())) || (x.krate == node.krate && x.refs.contains(&(None, node.name.clone())));
    let callers: Vec<&Node> = repo.nodes.iter().filter(|x| !(x.krate == node.krate && x.name == node.name) && (x.krate == node.krate || (public && repo.may_use(&x.krate, &node.krate))) && names_module(x) && fn_list.iter().any(|w| is_word(&x.text, w))).collect();
    let tests: Vec<&Target> = repo.targets.iter().filter(|t| repo.may_use(&t.krate, &node.krate) && fn_list.iter().any(|w| is_word(&t.text, w))).collect();
    if fn_list.is_empty() || (callers.is_empty() && tests.is_empty()) {
        return fallback(sel, "no module names its affected functions");
    }
    sel.notes.push(format!("{f}: in {u} the functions {} use its symbols; followed into {}", fn_list.join(", "), callers.iter().map(|x| format!("{}::{}", x.krate, x.name)).collect::<Vec<_>>().join(", ")));
    add(&mut sel.libs, node.krate.clone(), &format!("{f}: unit tests of {u}"));
    for t in tests {
        add(&mut sel.tests, (t.krate.clone(), t.name.clone()), &format!("{f}: the test names a function of {u} that uses it"));
    }
    for x in callers {
        let why = format!("{f} (via {}::{})", node.name, x.name);
        if x.root || !NARROW.contains(&x.krate.as_str()) {
            sel.wide(repo, &x.krate, &why);
        } else {
            sel.narrow(repo, &x.krate, &x.name, &why);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Git, output, running

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").current_dir(root).args(args).output().map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Changed file names: the working tree against the merge base with `base`, plus untracked.
/// Without git history (a VM role tree has an empty repository), the `.changed` list the sync wrote.
pub(crate) fn changed_files(root: &Path, base: &str) -> Result<Vec<String>, String> {
    if git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        let list = std::fs::read_to_string(root.join(".changed"))
            .map_err(|e| format!("no git history and no .changed list ({e}); sync with sync_tree.sh or pass --files"))?;
        let set: BTreeSet<String> = list.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        return Ok(set.into_iter().collect());
    }
    let base = if git(root, &["rev-parse", "--verify", "--quiet", base]).is_ok() {
        base.to_string()
    } else if base == "origin/main" && git(root, &["rev-parse", "--verify", "--quiet", "main"]).is_ok() {
        "main".to_string()
    } else {
        return Err(format!("base `{base}` does not exist"));
    };
    let mb = git(root, &["merge-base", "HEAD", &base])?;
    let mut set = BTreeSet::new();
    for l in git(root, &["diff", "--name-only", mb.trim()])?.lines() {
        set.insert(l.trim().to_string());
    }
    for l in git(root, &["status", "--porcelain", "--untracked-files=all"])?.lines() {
        let Some(rest) = l.get(3..) else { continue };
        let name = rest.rsplit(" -> ").next().unwrap_or(rest).trim().trim_matches('\u{22}');
        set.insert(name.to_string());
    }
    set.retain(|s| !s.is_empty());
    Ok(set.into_iter().collect())
}

fn print_selection(repo: &Repo, files: &[String], sel: &Selection) {
    println!("changed files: {}", files.len());
    for f in files {
        println!("  {f}");
    }
    if !sel.full.is_empty() {
        println!("FULL (escalated; run the whole gate):");
        for r in &sel.full {
            println!("  {r}");
        }
        return;
    }
    for n in &sel.notes {
        println!("note: {n}");
    }
    if sel.only {
        println!("ONLY mode: the named binaries and nothing else. NOT A GATE (no stubs, policy checks or clippy).");
    } else {
        println!("checks: stubs (with trace), purity, unsafe-audit, csys-audit");
    }
    for (c, why) in &sel.clippy {
        println!("clippy: {c}  [{why}]");
    }
    if sel.xtask_tests {
        println!("test: xtask unit tests (cargo test -p xtask)");
    }
    for c in &sel.xtask_cmds {
        println!("xtask: cargo xtask {c}");
    }
    for (c, why) in &sel.libs {
        println!("lib: {c}  [{why}]");
    }
    for ((c, t), why) in &sel.tests {
        println!("test: {c}::{t}  [{why}]");
    }
    for (n, why) in &sel.conf {
        println!("conformance: {n}  [{why}]");
    }
    println!("selected: {} of {} test targets, {} of {} conformance programs, {} unit-test libs", sel.tests.len(), repo.targets.len(), sel.conf.len(), repo.conf.len(), sel.libs.len());
}

/// How one step ended. `Skipped` is only for a test step that failed solely because no test matched.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Outcome {
    Ok,
    Skipped,
    Failed,
}

/// TS-2b (TOOL-CLEAN, 2026-10-08): was this failed test step a failure ONLY because no test matched the selection?
/// `step_out` is THAT step's own output (never the cumulative log). nextest (through `xtask nextest`) then prints
/// `error: no tests to run` and the wrapper ends with `xtask nextest: ... failed: exit status: 4`; anything else
/// that looks like a failure (a failed test, a build error, another exit status, an unknown `error` line) keeps it
/// FAILED. An empty test binary in a run that has tests, or "0 passed / 0 failed" alone, is NOT this.
fn no_tests_only(step_out: &str) -> bool {
    let mut saw = false;
    for l in step_out.lines() {
        let t = l.trim();
        if t == "error: no tests to run" {
            saw = true;
        } else if t.starts_with("---- ") || t.starts_with("error") || t.contains("could not compile") {
            return false;
        } else if let Some(r) = t.strip_prefix("test result: ") {
            // The wrapper's own line for the empty run; a real failure has failed tests.
            if !r.contains("; 0 failed;") || !r.contains(" 0 passed;") {
                return false;
            }
        } else if t.starts_with("xtask ") && !t.ends_with("failed: exit status: 4") {
            return false;
        }
    }
    saw
}

fn classify_step(success: bool, step_out: &str, allow_skip: bool) -> Outcome {
    if success {
        Outcome::Ok
    } else if allow_skip && no_tests_only(step_out) {
        Outcome::Skipped
    } else {
        Outcome::Failed
    }
}

fn step_out(root: &Path, log: &Path, label: &str, cmd: &mut Command, allow_skip: bool) -> Outcome {
    let started = Instant::now();
    // This step's own output starts where the log ends now (the classification never reads earlier steps).
    let from = fs::metadata(log).map_or(0, |m| m.len());
    let file = fs::OpenOptions::new().create(true).append(true).open(log);
    let (success, failed_to_start) = match file.and_then(|f| f.try_clone().map(|g| (f, g))) {
        Ok((out, err)) => {
            use std::io::Write;
            let _ = writeln!(&out, "== {label}");
            (cmd.current_dir(root).stdout(Stdio::from(out)).stderr(Stdio::from(err)).status().is_ok_and(|s| s.success()), false)
        }
        Err(e) => {
            eprintln!("cannot open {}: {e}", log.display());
            (false, true)
        }
    };
    let own = if failed_to_start || success || !allow_skip {
        String::new()
    } else {
        let bytes = fs::read(log).unwrap_or_default();
        String::from_utf8_lossy(&bytes[(from as usize).min(bytes.len())..]).into_owned()
    };
    let outcome = classify_step(success, &own, allow_skip);
    if outcome == Outcome::Skipped {
        // Relabel this step's slice of the log so no later reader (summary, ~/bin/test-summary) counts a failed suite.
        let bytes = fs::read(log).unwrap_or_default();
        let cut = (from as usize).min(bytes.len());
        let relabelled = own.replace("error: no tests to run", "skipped: no tests to run").replace("test result: FAILED.", "no tests ran, so not a suite result:").replace("xtask nextest:", "xtask nextest (skipped):");
        let _ = fs::write(log, [&bytes[..cut], relabelled.as_bytes()].concat());
    }
    let shown = match outcome {
        Outcome::Ok => "ok  ",
        Outcome::Skipped => "skip",
        Outcome::Failed => "FAIL",
    };
    if outcome == Outcome::Failed {
        log_step_failed(log, label, "");
    }
    println!("{:>6.1}s  {shown}  {label}{}", started.elapsed().as_secs_f64(), if outcome == Outcome::Skipped { "  (no test matched the selection: skipped, not failed)" } else { "" });
    outcome
}

/// TOOLS-9: a line `STEP FAILED: <label>: <why>` in the affected log, which `~/bin/test-summary` lists next to the
/// failing tests, so a failed step that is not a test (K5, a policy check, clippy, products) is never lost.
fn log_step_failed(log: &Path, label: &str, why: &str) {
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(f, "STEP FAILED: {label}{}{}", if why.is_empty() { "" } else { ": " }, why.lines().next().unwrap_or(""));
    }
}

/// An in-process check (not a child command): its outcome is printed, and a failure is also written to the log.
fn check_logged(log: &Path, label: &str, r: Result<(), String>) -> Result<(), ()> {
    match r {
        Ok(()) => {
            println!("  ok    {label}");
            Ok(())
        }
        Err(e) => {
            println!("  FAIL  {label}: {e}");
            log_step_failed(log, label, &e);
            Err(())
        }
    }
}

fn step(root: &Path, log: &Path, label: &str, cmd: &mut Command) -> bool {
    step_out(root, log, label, cmd, false) == Outcome::Ok
}

/// TOOLS-7 (1): like `step`, and the step's own output (everything it appended to `log`) is also written to
/// `<root>/target/<name>`, so a failed step's FULL output (clippy: every error with its context, not only the
/// first error line of the summary) survives in one file. The file is removed first, so it is never a stale one.
fn step_saving(root: &Path, log: &Path, label: &str, cmd: &mut Command, name: &str) -> bool {
    let dest = root.join("target").join(name);
    let _ = fs::remove_file(&dest);
    let from = fs::metadata(log).map_or(0, |m| m.len());
    let ok = step(root, log, label, cmd);
    let bytes = fs::read(log).unwrap_or_default();
    let _ = fs::write(&dest, &bytes[(from as usize).min(bytes.len())..]);
    if !ok {
        println!("        full output of the failed step: {}", dest.display());
    }
    ok
}

/// A few lines from a cargo/xtask log, in the format of `~/bin/test-summary`.
fn summary(log: &str) -> String {
    let (mut suites, mut ok, mut bad, mut skipped, mut passed, mut failed, mut ignored) = (0, 0, 0, 0, 0u64, 0u64, 0u64);
    let mut fails = BTreeSet::new();
    let mut errs: Vec<String> = Vec::new();
    // The "  --> file:line:col" after a kept error goes on that error's line (one per error).
    let mut want_loc = false;
    let mut seen = BTreeSet::new();
    let mut conf = String::new();
    for l in log.lines() {
        if want_loc {
            if let Some(loc) = l.trim_start().strip_prefix("--> ") {
                if let Some(e) = errs.last_mut() {
                    e.push_str(&format!(" [{}]", loc.trim()));
                }
                want_loc = false;
                continue;
            }
            if l.starts_with("error") || l.starts_with("test result: ") {
                want_loc = false;
            }
        }
        if let Some(r) = l.strip_prefix("test result: ") {
            suites += 1;
            if r.starts_with("ok.") {
                ok += 1;
            } else {
                bad += 1;
            }
            let w: Vec<&str> = r.split([' ', ';', '.']).filter(|s| !s.is_empty()).collect();
            for i in 1..w.len() {
                let n: u64 = w[i - 1].parse().unwrap_or(0);
                match w[i] {
                    "passed" => passed += n,
                    "failed" => failed += n,
                    "ignored" => ignored += n,
                    _ => {}
                }
            }
        } else if l == "skipped: no tests to run" {
            skipped += 1;
        } else if let Some(t) = l.strip_prefix("---- ").and_then(|x| x.strip_suffix(" stdout ----")) {
            fails.insert(t.to_string());
        } else if l.starts_with("error") && seen.insert(l.to_string()) {
            errs.push(l.to_string());
            want_loc = true;
        } else if l.starts_with("conformance: ") && l.contains("passed") {
            conf = l.to_string();
        }
    }
    let mut out = format!("suites: {suites} ({ok} ok, {bad} failed{}); tests: {passed} passed, {failed} failed, {ignored} ignored\n", if skipped > 0 { format!(", {skipped} skipped") } else { String::new() });
    if !conf.is_empty() {
        out.push_str(&conf);
        out.push('\n');
    }
    for t in fails.iter().take(15) {
        out.push_str(&format!("FAILED: {t}\n"));
    }
    if fails.len() > 15 {
        out.push_str(&format!("... {} more failed tests\n", fails.len() - 15));
    }
    for e in errs.iter().take(5) {
        out.push_str(e);
        out.push('\n');
    }
    out
}

fn run_selection(root: &Path, repo: &Repo, sel: &Selection, with_exports: bool) -> Result<(), String> {
    let started = Instant::now();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let log = root.join("target").join("affected.log");
    let _ = fs::create_dir_all(root.join("target"));
    let _ = fs::remove_file(&log);
    let mut ok = true;
    let mut skipped: Vec<String> = Vec::new();
    if !sel.full.is_empty() {
        println!("escalated to FULL: running the workspace clippy and `cargo xtask test`");
        ok &= step(root, &log, "stubs", Command::new(&cargo).args(["xtask", "stubs"]));
        ok &= step_saving(root, &log, "clippy --workspace", Command::new(&cargo).args(["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]), "clippy.log");
        ok &= step(root, &log, "cargo xtask test", Command::new(&cargo).args(["xtask", "test"]));
    } else {
        if sel.only {
            println!("ONLY mode: the named binaries, no stubs, policy checks or clippy. NOT A GATE.");
        } else {
            ok &= check_logged(&log, "stubs", crate::spec::stubs(root, false)).is_ok();
            // Each check on its own, so the log names every one that failed (TOOLS-9: a summary said "static checks
            // ok" while K5 had failed; the failing names were only in the console output).
            ok &= check_logged(&log, "purity", crate::policy::purity(root)).is_ok();
            ok &= check_logged(&log, "unsafe-audit", crate::policy::unsafe_audit(root)).is_ok();
            ok &= check_logged(&log, "csys-audit", crate::policy::csys_audit(root)).is_ok();
            if sel.k5 {
                ok &= check_logged(&log, "K5 (ffi.rs and liella_csys.h agree)", crate::csys::machine_decls_match(root)).is_ok();
            }
        }
        if !sel.clippy.is_empty() {
            // The workspace, as the gate runs it: per-package feature unification differs and
            // would report (or hide) lints the gate does not. Incremental, so cheap.
            let mut cmd = Command::new(&cargo);
            cmd.args(["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]);
            ok &= step_saving(root, &log, &format!("clippy --workspace (changed: {})", sel.clippy.keys().cloned().collect::<Vec<_>>().join(", ")), &mut cmd, "clippy.log");
        }
        if sel.xtask_tests {
            ok &= step(root, &log, "xtask unit tests", Command::new(&cargo).args(["test", "-p", "xtask"]));
        }
        let needs_products = !sel.conf.is_empty() || sel.xtask_cmds.contains("symgap") || repo.targets.iter().any(|t| t.products && sel.tests.contains_key(&(t.krate.clone(), t.name.clone())));
        if needs_products {
            let mut cmd = Command::new(&cargo);
            cmd.args(["xtask", "so"]);
            ok &= step(root, &log, "products (cargo xtask so)", &mut cmd);
        }
        // Per crate: its unit tests and its selected targets, then again with the exports
        // interposed (as the gate does) for the crates that can link the boundary.
        let mut per: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (c, t) in sel.tests.keys() {
            per.entry(c).or_default().push(t);
        }
        for c in sel.libs.keys() {
            per.entry(c).or_default();
        }
        let selftest = ["liella", "liella-boundary", "liella-rt", "liella-math"];
        let interpose = with_exports || (!sel.only && sel.clippy.keys().any(|c| matches!(c.as_str(), "liella-mem" | "liella-rt" | "liella-kernel" | "liella-abi" | "liella-boundary")));
        if !interpose {
            println!("note: the exports-interposed pass is skipped (no change in mem, rt, kernel, abi or boundary); --with-exports runs it");
        }
        let nextest = crate::nextest::available(&cargo);
        for (c, ts) in &per {
            for pass in 0..2 {
                // `liella`'s clause tests take caller-owned blocks (`getifaddrs`, `strdup`, ...)
                // from the C family, which is Liella's heap only with the exports interposed: there
                // its poisoned frees catch a read after `free` (FREE-POISON-TESTS-2), so they always
                // get the second pass.
                let c_family = *c == "liella" && ts.iter().any(|t| t.ends_with("_clauses"));
                if pass == 1 && !((interpose || c_family) && selftest.contains(c)) {
                    continue;
                }
                // The pure-Rust sweeps run in the first pass only (`nextest::PASS0_ONLY`, #889);
                // a pass left with no target and no unit tests is not run (no `--test` would run all).
                let ts: Vec<&str> = ts.iter().copied().filter(|t| pass == 0 || !crate::nextest::pass0_only(c, t)).collect();
                if pass == 1 && ts.is_empty() && !sel.libs.contains_key(*c) {
                    continue;
                }
                // Through nextest when the gate's `xtask test` uses it (`nextest.rs`): the same
                // runner and serial groups, and the summary lines of a `cargo test` log.
                let mut cmd = Command::new(&cargo);
                crate::nextest::poison(&mut cmd);
                if nextest {
                    cmd.args(["xtask", "nextest", "-p", c]);
                } else {
                    cmd.args(["test", "-p", c]);
                }
                if pass == 1 {
                    cmd.args(["--features", "liella-boundary/export,liella-boundary/selftest"]);
                }
                if sel.libs.contains_key(*c) {
                    cmd.arg("--lib");
                }
                for t in ts {
                    cmd.args(["--test", t]);
                }
                let label = format!("test -p {c}{}", if pass == 1 { " (exports interposed)" } else { "" });
                match step_out(root, &log, &label, &mut cmd, true) {
                    Outcome::Ok => {}
                    Outcome::Skipped => skipped.push(label),
                    Outcome::Failed => ok = false,
                }
            }
        }
        if !sel.conf.is_empty() {
            let mut cmd = Command::new("sh");
            cmd.arg("conformance/run.sh");
            for n in sel.conf.keys() {
                cmd.arg(n);
            }
            ok &= step(root, &log, &format!("conformance ({} programs)", sel.conf.len()), &mut cmd);
        }
        for c in &sel.xtask_cmds {
            ok &= step(root, &log, &format!("cargo xtask {c}"), Command::new(&cargo).args(["xtask", c]));
        }
    }
    println!("--- summary ({}), log {}", format_args!("{:.0}s", started.elapsed().as_secs_f64()), log.display());
    print!("{}", summary(&read(&log)));
    if !skipped.is_empty() {
        println!("skipped (no test matched the selection; counted apart, not a failure): {} - {}", skipped.len(), skipped.join("; "));
    }
    println!("affected: {}{}", if ok { "ok" } else { "FAILED" }, if sel.only { " (ONLY mode: named binaries, NOT A GATE)" } else { "" });
    if !ok {
        // D-X.23: test builds carry no DWARF; panics still name file:line (core::panic::Location).
        println!("hint: for a debugger or source-level backtrace, rerun the red test with debuginfo: CARGO_PROFILE_DEV_DEBUG=full cargo xtask affected --only <target>");
    }
    let _ = repo;
    if ok { Ok(()) } else { Err("a selected test or check failed".into()) }
}

/// Is a failed test (`crate::target name`, `crate name` for unit tests, `xtask::bin/xtask name`) in the selection?
fn covers(sel: &Selection, failed: &str) -> bool {
    if !sel.full.is_empty() {
        return true;
    }
    let bin = failed.split_whitespace().next().unwrap_or_default();
    if bin.starts_with("xtask") {
        return sel.xtask_tests;
    }
    match bin.split_once("::") {
        Some((c, t)) => sel.tests.contains_key(&(c.to_string(), t.to_string())),
        None => sel.libs.contains_key(bin),
    }
}

fn counts(sel: &Selection) -> String {
    if sel.full.is_empty() { format!("{}t/{}c/{}l", sel.tests.len(), sel.conf.len(), sel.libs.len()) } else { "FULL".to_string() }
}

/// GATE-TIER replay proof: `records` holds `LOG <name>` / `FILES <path>...` / `FAILS <line>|<line>|...` (the output of
/// `work/GATE-TIER/failures.sh` over the gate logs). For each record with changed files, the tier's selection is
/// computed on THIS tree and every `FAILED: <test>` line is checked to be inside it. Returns the report and the
/// number of misses. A record without files is an `--only` run (not a gate) and is skipped.
pub fn replay(repo: &Repo, records: &str, tier: Tier) -> (String, usize) {
    let (mut out, mut misses) = (String::new(), 0);
    let (mut log, mut files): (String, Vec<String>) = (String::new(), Vec::new());
    let (mut gates, mut fails_seen, mut tests_tier, mut tests_wide, mut full_tier, mut flaky) = (0, 0, 0, 0, 0, 0);
    let (mut conf_tier, mut conf_wide) = (0, 0);
    // `FLAKE <test>` lines: known flakes (tracked as their own tasks), counted apart, never a miss.
    let flakes: Vec<&str> = records.lines().filter_map(|l| l.strip_prefix("FLAKE ")).map(str::trim).filter(|f| !f.is_empty()).collect();
    for line in records.lines() {
        if let Some(l) = line.strip_prefix("LOG ") {
            log = l.trim().to_string();
        } else if let Some(f) = line.strip_prefix("FILES") {
            files = f.split_whitespace().map(String::from).collect();
        } else if let Some(fails) = line.strip_prefix("FAILS") {
            if files.is_empty() {
                out.push_str(&format!("{log}: no changed files (an --only run, not a gate): skipped\n"));
                continue;
            }
            gates += 1;
            let sel = select_tier(repo, &files, tier);
            let wide = select_tier(repo, &files, Tier::Wide);
            if sel.full.is_empty() {
                tests_tier += sel.tests.len();
                tests_wide += wide.tests.len();
                conf_tier += sel.conf.len();
                conf_wide += wide.conf.len();
            } else {
                full_tier += 1;
            }
            let mut notes = Vec::new();
            for f in fails.split('|').map(str::trim).filter(|f| !f.is_empty()) {
                if let Some(t) = f.strip_prefix("FAILED: ") {
                    fails_seen += 1;
                    if !covers(&sel, t) && flakes.iter().any(|fl| t.contains(fl)) {
                        flaky += 1;
                        notes.push(format!("outside, known flake {t}"));
                    } else if !covers(&sel, t) {
                        misses += 1;
                        notes.push(format!("MISS {t}"));
                    }
                } else if f.contains("conformance (") {
                    notes.push(format!("conformance program not named in the log ({f})"));
                }
            }
            out.push_str(&format!("{log}: files={} {tier:?}={} wide={}{}{}\n", files.len(), counts(&sel), counts(&wide), if notes.is_empty() { "" } else { " | " }, notes.join("; ")));
            files.clear();
        }
    }
    out.push_str(&format!("replay {tier:?}: {gates} gates, {fails_seen} FAILED lines, {misses} misses, {flaky} outside but known flakes; test targets selected over the narrowed gates {tests_tier} ({tier:?}) vs {tests_wide} (wide), conformance programs {conf_tier} vs {conf_wide}; {full_tier} FULL\n"));
    (out, misses)
}

/// The base ref when `--base` is absent: `AFFECTED_BASE` if set and non-empty, else `origin/main`.
fn default_base(env: Option<String>) -> String {
    env.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| "origin/main".to_string())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    // AFFECTED_BASE: the ref a stacked branch (one cut from an unmerged parent) selects against; `--base` wins.
    let mut base = default_base(std::env::var("AFFECTED_BASE").ok());
    let mut run = false;
    let mut with_exports = false;
    let mut files: Option<Vec<String>> = None;
    let mut only: Option<String> = None;
    let mut tier = Tier::Direct;
    let mut replay_file: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => base = it.next().ok_or("--base needs a ref")?.clone(),
            "--tier" => tier = Tier::parse(it.next().ok_or("--tier needs direct, wide or full")?)?,
            "--replay" => replay_file = Some(it.next().ok_or("--replay needs a records file")?.clone()),
            "--run" => run = true,
            "--list" => run = false,
            "--with-exports" => with_exports = true,
            "--only" => only = Some(it.next().ok_or("--only needs a list: <target>[,crate::target][,conformance:<program>][,lib:<crate>][,xtask]")?.clone()),
            "--files" => {
                let mut v = Vec::new();
                while let Some(f) = it.clone().next().filter(|f| !f.starts_with("--")) {
                    v.push(f.clone());
                    it.next();
                }
                files = Some(v);
            }
            other => return Err(format!("unknown argument `{other}` (usage: affected [--base <ref>] [--files <path>...] [--only <names>] [--tier direct|wide|full] [--replay <records>] [--list | --run] [--with-exports])")),
        }
    }
    if let Some(path) = replay_file {
        let repo = Repo::load(root);
        let text = fs::read_to_string(&path).map_err(|e| format!("--replay {path}: {e}"))?;
        let (report, misses) = replay(&repo, &text, tier);
        print!("{report}");
        return if misses == 0 { Ok(()) } else { Err(format!("replay: {misses} failures outside the {tier:?} tier")) };
    }
    if let Some(list) = only {
        let repo = Repo::load(root);
        let sel = select_only(&repo, &list)?;
        print_selection(&repo, &[], &sel);
        return if run { run_selection(root, &repo, &sel, with_exports) } else { Ok(()) };
    }
    let files = match files {
        Some(f) => f,
        None => changed_files(root, &base)?,
    };
    let repo = Repo::load(root);
    let sel = select_tier(&repo, &files, tier);
    println!("tier: {tier:?} (per-fix gates: direct; the landing gate runs everything)");
    print_selection(&repo, &files, &sel);
    if run { run_selection(root, &repo, &sel, with_exports) } else { Ok(()) }
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_honours_the_env_and_falls_back_to_main() {
        assert_eq!(default_base(None), "origin/main");
        assert_eq!(default_base(Some(String::new())), "origin/main");
        assert_eq!(default_base(Some("  ".into())), "origin/main");
        assert_eq!(default_base(Some("origin/opt/lv3".into())), "origin/opt/lv3");
    }

    // TOOLS-7 (1): a failed step's FULL output lands in target/<name> (only that step's, not earlier steps'), and a
    // stale file from an earlier run is replaced; a passing step leaves its (short) output there too.
    #[test]
    fn step_saving_keeps_the_whole_output_of_the_failed_step() {
        let root = std::env::temp_dir().join(format!("xtask-stepsave-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target")).unwrap();
        let log = root.join("target").join("affected.log");
        fs::write(root.join("target").join("clippy.log"), "STALE").unwrap();
        assert!(step(&root, &log, "earlier step", Command::new("sh").args(["-c", "echo earlier-output"])));
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo 'error: first'; echo '  --> a.rs:1:1'; echo 'error: second'; echo '  --> b.rs:2:2' >&2; exit 1"]);
        assert!(!step_saving(&root, &log, "clippy --workspace", &mut cmd, "clippy.log"));
        let saved = fs::read_to_string(root.join("target").join("clippy.log")).unwrap();
        for want in ["== clippy --workspace", "error: first", "error: second", "b.rs:2:2"] {
            assert!(saved.contains(want), "missing {want:?} in {saved:?}");
        }
        assert!(!saved.contains("STALE") && !saved.contains("earlier-output"), "{saved:?}");
        let _ = fs::remove_dir_all(&root);
    }

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn repo() -> &'static Repo {
        static REPO: std::sync::OnceLock<Repo> = std::sync::OnceLock::new();
        // The sync's `.changed.hdr` (present in a VM tree) must not decide the unit tests: no header diff here.
        REPO.get_or_init(|| Repo { hdr_diff: None, ..Repo::load(&root()) })
    }

    // TOOLS-7 (3): only one-line `liella_*` declarations are mappable; anything structural is not.
    #[test]
    fn header_symbols_reads_one_line_declarations_only() {
        let head = "diff --git a/csys/include/h.h b/csys/include/h.h\n--- a/csys/include/h.h\n+++ b/csys/include/h.h\n@@ -1,0 +2,3 @@\n";
        let d = |body: &str| format!("diff --git a/other.h b/other.h\n+struct s {{\n{head}{body}");
        assert_eq!(header_symbols(&d("+/* c */\n+void *liella_a(void *d, const void *s, unsigned long n);\n-int liella_b(void);\n"), "csys/include/h.h"), Some(vec!["liella_a".to_string(), "liella_b".to_string()]));
        for bad in ["+struct liella_s {\n", "+#define LIELLA_X(a) (a)\n", "+int liella_a(int a,\n", "+typedef int liella_t;\n", "+int not_ours(void);\n", "+static inline int liella_i(void) { return 0; }\n", "+/* only a comment */\n"] {
            assert_eq!(header_symbols(&d(bad), "csys/include/h.h"), None, "{bad:?}");
        }
        assert_eq!(header_symbols(&d("+int liella_a(void);\n"), "csys/include/zzz.h"), None, "other file's lines are not this file's");
    }

    #[test]
    fn header_change_maps_to_users_or_escalates_and_k5_always_runs() {
        let (_, s) = sel(&["csys/include/liella_csys.h"]);
        assert!(!s.full.is_empty() && s.k5, "no diff available: FULL, K5 flagged");
        let (_, s) = sel(&["crates/boundary/src/ffi.rs"]);
        assert!(s.k5, "ffi.rs changed: K5");
        let (_, s) = sel(&["csys/src/machine/new_file.S"]);
        assert!(s.k5, "csys/src/machine changed: K5");
        let (_, s) = sel(&["crates/core/src/lib.rs"]);
        assert!(!s.k5, "unrelated change: no K5");
        // With a diff: some symbol defined by an assembly source and used by Rust maps; a struct line does not.
        let mut probe = Repo::load(&root());
        let syms: Vec<String> = probe.csys_refs.keys().filter(|k| k.starts_with("liella_")).cloned().collect();
        let mut mapped = false;
        for sym in &syms {
            probe.hdr_diff = Some(format!("diff --git a/csys/include/liella_csys.h b/csys/include/liella_csys.h\n+void {sym}(void);\n"));
            let mut sel = Selection::default();
            if header_map(&probe, "csys/include/liella_csys.h", &mut sel) {
                // A user of the symbol may itself escalate (csys_user follows it as a changed file): try the next one.
                if sel.full.is_empty() && !sel.clippy.is_empty() {
                    mapped = true;
                    break;
                }
            }
        }
        assert!(mapped, "no csys symbol maps (the rule never fires)");
        probe.hdr_diff = Some("diff --git a/csys/include/liella_csys.h b/csys/include/liella_csys.h\n+struct liella_s {\n".into());
        assert!(!header_map(&probe, "csys/include/liella_csys.h", &mut Selection::default()));
    }

    fn sel(files: &[&str]) -> (&'static Repo, Selection) {
        let repo = repo();
        let s = select(repo, &files.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        (repo, s)
    }

    fn has_test(s: &Selection, name: &str) -> bool {
        s.tests.keys().any(|(_, t)| t == name)
    }

    #[test]
    fn model_has_the_workspace() {
        let repo = repo();
        assert!(repo.crates.iter().any(|c| c.name == "liella-boundary"));
        assert!(repo.dependents("liella").contains("liella-boundary"), "boundary depends on core");
        assert!(repo.dependents("liella").contains("liella"), "a crate depends on itself here");
        assert!(repo.dependents("liella-kernel").contains("liella-math") || repo.dependents("liella-kernel").contains("liella-boundary"));
        assert!(repo.targets.len() > 100 && repo.conf.len() > 50, "{} targets, {} conformance", repo.targets.len(), repo.conf.len());
    }

    #[test]
    fn escalations() {
        for f in [
            "csys/src/x.c",
            "crates/abi/src/lib.rs",
            "Cargo.toml",
            "Cargo.lock",
            "crates/core/Cargo.toml",
            "crates/boundary/build.rs",
            "rust-toolchain.toml",
            "xtask/src/main.rs",
            "xtask/src/trace.rs",
            "spec/clauses.tsv",
            "spec/abi/layouts.tsv",
            "include/liella/native.h",
            "crates/boundary/src/posix/s10_math_gen.rs",
            "crates/core/src/unicode/tables.rs",
            "crates/boundary/src/posix/s10_math.tsv",
            "crates/libc-so/src/lib.rs",
            "scripts/anything.sh",
            "somewhere/unknown.bin",
        ] {
            let (_, s) = sel(&[f]);
            assert!(!s.full.is_empty(), "{f} must escalate to full");
        }
    }

    // TS-2b: the exact output of an empty nextest run, as `cargo xtask nextest -p liella-ld` prints it.
    const NO_TESTS: &str = "== test -p liella-ld\n    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.69s\n────────────\n Nextest run ID fcec0948 with nextest profile: default\n    Starting 0 tests across 1 binary\n────────────\n     Summary [   0.000s] 0 tests run: 0 passed, 0 skipped\nerror: no tests to run\n(hint: use `--no-tests` to customize)\ntest result: FAILED. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.000s (nextest)\nxtask nextest: cd \"/h/w/b\" && \"cargo\" \"nextest\" \"run\" \"-p\" \"liella-ld\" failed: exit status: 4\n";

    #[test]
    fn an_empty_selection_is_a_skip_and_a_real_failure_is_not() {
        assert!(no_tests_only(NO_TESTS));
        assert_eq!(classify_step(false, NO_TESTS, true), Outcome::Skipped);
        assert_eq!(classify_step(false, NO_TESTS, false), Outcome::Failed, "only test steps may skip");
        assert_eq!(classify_step(true, "", true), Outcome::Ok);
        // A failed test, a build error, another exit status, and a bare empty-summary line are all FAILED.
        let failed_test = "     Summary [1s] 3 tests run: 2 passed, 1 failed, 0 skipped\n---- x::y stdout ----\ntest result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\nxtask nextest: cmd failed: exit status: 100\n";
        assert_eq!(classify_step(false, failed_test, true), Outcome::Failed);
        let build = "error[E0425]: cannot find value `x`\nerror: could not compile `liella-ld`\nxtask nextest: cmd failed: exit status: 101\n";
        assert_eq!(classify_step(false, build, true), Outcome::Failed);
        let zero_only = "test result: FAILED. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in ?s (nextest: no summary line - the run did not finish)\nxtask nextest: cmd failed: exit status: 101\n";
        assert_eq!(classify_step(false, zero_only, true), Outcome::Failed, "0 passed / 0 failed is not the message");
        assert_eq!(classify_step(false, &NO_TESTS.replace("exit status: 4", "exit status: 101"), true), Outcome::Failed);
        assert_eq!(classify_step(false, &format!("error: linker failed\n{NO_TESTS}"), true), Outcome::Failed, "another error line wins");
        assert_eq!(classify_step(false, "", true), Outcome::Failed, "no output is a failure");
    }

    #[test]
    fn a_skipped_step_is_counted_apart_in_the_summary() {
        let relabelled = NO_TESTS.replace("error: no tests to run", "skipped: no tests to run").replace("test result: FAILED.", "no tests ran, so not a suite result:").replace("xtask nextest:", "xtask nextest (skipped):");
        let log = format!("test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n{relabelled}");
        let s = summary(&log);
        assert!(s.starts_with("suites: 1 (1 ok, 0 failed, 1 skipped); tests: 10 passed, 0 failed, 0 ignored"), "{s}");
        assert!(!s.contains("FAILED") && !s.contains("error"), "{s}");
    }

    // AFF-OWNER: whatever the module graph reaches, the suite of the crate that owns a changed source file runs.
    #[test]
    fn the_owning_crates_suite_is_always_selected() {
        let r = repo();
        let mut checked = 0;
        for c in r.crates.iter().filter(|c| c.dir != "xtask") {
            // The top-level files of each crate's src (one per module): the cheapest sweep that still reaches the
            // re-exported-module case (a `crates/core/src/error.rs` change handed to `wide` of another crate).
            let files: Vec<PathBuf> = sorted_dir(&root().join(&c.dir).join("src")).into_iter().filter(|p| p.extension().is_some_and(|e| e == "rs")).collect();
            for f in &files {
                let rel = f.strip_prefix(root()).unwrap_or(f).to_string_lossy().replace('\\', "/");
                let s = select(r, std::slice::from_ref(&rel));
                if s.full.is_empty() {
                    assert!(s.libs.contains_key(&c.name) || s.tests.keys().any(|(k, _)| *k == c.name), "{rel}: no suite of its crate `{}` selected: {s:?}", c.name);
                    checked += 1;
                }
            }
        }
        assert!(checked > 20, "the sweep checked only {checked} files");
    }

    #[test]
    fn docs_only_runs_no_tests() {
        for f in ["docs/differences-from-glibc.md", "plan/05.md", "handoff/process.md", "README.md", "spec/behaviour/x.md", "handoff/reports/PR-4.json", "tools/probe/probe"] {
            let (_, s) = sel(&[f]);
            assert!(s.full.is_empty() && s.tests.is_empty() && s.conf.is_empty() && s.libs.is_empty() && s.clippy.is_empty(), "{f}: {s:?}");
        }
    }

    #[test]
    fn xtask_and_bench_files_select_the_xtask_tests() {
        for f in ["bench/instr_limits.tsv", "bench/c/x.c", "benchmarks/a.md", "xtask/src/so.rs", "xtask/Cargo.toml"] {
            let (_, s) = sel(&[f]);
            assert!(s.xtask_tests, "{f} must select -p xtask: {s:?}");
        }
        let (_, s) = sel(&["bench/instr_limits.tsv"]);
        assert!(s.full.is_empty() && s.tests.is_empty() && s.conf.is_empty(), "bench data selects only the xtask tests: {s:?}");
        assert!(!sel(&["fuzz/seeds/x"]).1.xtask_tests, "fuzz stays unselected");
    }

    #[test]
    fn own_module_runs_only_xtask_tests() {
        let (_, s) = sel(&["xtask/src/affected.rs"]);
        assert!(s.full.is_empty() && s.xtask_tests && s.tests.is_empty() && s.conf.is_empty());
    }

    #[test]
    fn a_test_file_selects_its_target() {
        let (_, s) = sel(&["crates/core/tests/printf_products.rs"]);
        assert!(s.full.is_empty() && s.tests.len() == 1 && has_test(&s, "printf_products"), "{s:?}");
        assert!(s.conf.is_empty() && s.clippy.contains_key("liella"));
    }

    #[test]
    fn a_support_file_selects_the_targets_that_name_it() {
        let (_, s) = sel(&["crates/core/tests/oracle/printf_diff.c"]);
        assert!(has_test(&s, "printf_products"), "{s:?}");
    }

    #[test]
    fn a_conformance_program_selects_itself_and_the_tests_naming_it() {
        let (_, s) = sel(&["conformance/c/s12_printf.c"]);
        assert!(s.conf.contains_key("s12_printf") && s.conf.len() == 1);
        assert!(has_test(&s, "s12_stdio_clauses"), "the clause test reads its output: {s:?}");
        let (repo, s) = sel(&["conformance/run.sh"]);
        assert_eq!(s.conf.len(), repo.conf.len());
        assert!(s.tests.is_empty());
    }

    #[test]
    fn root_and_infrastructure_files_are_wide() {
        for f in ["crates/core/src/lib.rs", "crates/kernel/src/lib.rs", "crates/rt/src/sync.rs", "crates/mem/src/lib.rs"] {
            let (repo, s) = sel(&[f]);
            assert!(s.full.is_empty(), "{f}");
            assert_eq!(s.conf.len(), repo.conf.len(), "{f}: wide selects every conformance program");
            assert!(has_test(&s, "s12_stdio_clauses") && has_test(&s, "s05_ctype_clauses"), "{f}");
        }
    }

    /// The independent check of the three constructed cases: every test source that names the
    /// module with a plain text search is in the selection.
    fn reachers(repo: &Repo, module: &str) -> Vec<(String, String)> {
        let pat = format!("{module}::");
        repo.targets.iter().filter(|t| t.text.contains(&pat)).map(|t| (t.krate.clone(), t.name.clone())).collect()
    }

    #[test]
    fn printf_change_is_a_superset_of_the_tests_that_reach_it() {
        let (repo, s) = sel(&["crates/core/src/printf.rs"]);
        assert!(s.full.is_empty());
        for k in reachers(repo, "printf") {
            assert!(s.tests.contains_key(&k), "{k:?} names printf:: but is not selected");
        }
        for t in ["printf_products", "printf_corpus_products", "s12_stdio_clauses", "s12_stdio_symbols", "stdio_products"] {
            assert!(has_test(&s, t), "{t} must be selected");
        }
        for c in ["s12_printf", "s12_symbols_printf", "s12_symbols_printf_chk"] {
            assert!(s.conf.contains_key(c), "{c} must be selected");
        }
        // Narrowing is real: other sections' clause tests and programs are left out.
        assert!(!has_test(&s, "s05_ctype_clauses") && !has_test(&s, "s18_processes_clauses"));
        assert!(!s.conf.contains_key("s18_fork") && !s.conf.contains_key("s22_cancel"));
    }

    #[test]
    fn locale_change_is_a_superset_of_the_tests_that_reach_it() {
        let (repo, s) = sel(&["crates/core/src/locale/mod.rs"]);
        assert!(s.full.is_empty());
        for k in reachers(repo, "locale") {
            assert!(s.tests.contains_key(&k), "{k:?} names locale:: but is not selected");
        }
        for t in ["s07_locale_clauses", "locale_products", "locale_data"] {
            assert!(has_test(&s, t), "{t} must be selected");
        }
        assert!(s.conf.contains_key("s07_strfmon"));
    }

    #[test]
    fn csys_change_is_full() {
        let (_, s) = sel(&["csys/src/machine/x.S"]);
        assert!(!s.full.is_empty() && s.tests.is_empty());
    }

    #[test]
    fn csys_assembly_selects_the_users_of_its_symbols() {
        // restorer.S defines only `liella_` symbols, used by a plain function of sys.rs. (scan.S,
        // the earlier example, defines the C symbol `memchr` since MEMCHR-2: it escalates.)
        let (repo, s) = sel(&["csys/src/machine/restorer.S"]);
        assert!(s.full.is_empty(), "{:?}", s.full);
        assert!(s.clippy.contains_key(CSYS_CRATE));
        assert!(s.notes.iter().any(|n| n.contains("crates/boundary/src/sys.rs")), "{:?}", s.notes);
        // Independent check: every test that names one of its symbols is selected.
        let syms = repo.csys["csys/src/machine/restorer.S"].symbols.clone().unwrap_or_default();
        assert!(syms.iter().any(|x| x == "liella_machine_restorer"), "{syms:?}");
        for t in repo.targets.iter().filter(|t| syms.iter().any(|x| t.text.contains(x.as_str()))) {
            assert!(s.tests.contains_key(&(t.krate.clone(), t.name.clone())), "{}::{} names a restorer.S symbol", t.krate, t.name);
        }
        // A trait method that uses a symbol escalates (cpuid.S: `Machine::cpuid` in sys.rs).
        let (_, s) = sel(&["csys/src/machine/cpuid.S"]);
        assert!(s.full.iter().any(|r| r.contains("trait method")), "{s:?}");
    }

    #[test]
    fn function_spans_and_traits() {
        let src = "pub fn a() {\n    ffi::liella_x();\n}\n\nfn b(\n    v: u8,\n) -> u8 {\n    a();\n    v\n}\n\nfn c() {}\n\nimpl Machine for Sys {\n    fn m(&self) {\n        b(1);\n    }\n}\n";
        let w = ["liella_x".to_string()];
        assert_eq!(affected_fns(src, &w), Some(BTreeSet::from(["a".to_string(), "b".to_string(), "m".to_string()])));
        assert!(in_trait(src, "m") && !in_trait(src, "a") && !in_trait(src, "b"));
        // Outside every function, or a C entry point: unsure.
        assert_eq!(affected_fns("static P: fn() = liella_x;\n", &w), None);
        assert_eq!(affected_fns("#[unsafe(no_mangle)]\npub extern \"C\" fn e() {\n    liella_x();\n}\n", &w), None);
        assert_eq!(affected_fns("fn e() {\n    liella_x();\n}\nexport!(e);\n", &w), None);
    }

    #[test]
    fn csys_falls_back_to_full_when_unsure() {
        for f in [
            "csys/src/machine/setjmp.S",   // defines setjmp (exported C ABI)
            "csys/src/machine/scan.S",       // defines memchr (exported C ABI, MEMCHR-2)
            "csys/src/machine/entry.S",    // defines _start
            "csys/src/machine/new_file.S", // not in the tree
            "csys/src/sys/lock.c",         // generated
            "csys/src/adapter/va.c",       // C: not read statically
            "csys/include/liella_csys.h",  // a header (no .changed.hdr diff in the unit tests' repo: not mappable)
            "csys/syscalls.tsv",           // a generator input
        ] {
            let (_, s) = sel(&[f]);
            assert!(!s.full.is_empty(), "{f} must escalate to full: {s:?}");
        }
        // TOOLS-21: in the DIRECT tier a listed machine export selects its fixed machine tests, not FULL; an unlisted
        // export (setjmp, getcontext, _start) stays FULL; --tier full is FULL. CSYS-MAP-2: read against a fixture whose
        // listed files export exactly `FIXTURE_EXPORTS`, not the live exports, so a branch that adds an export cannot
        // break this test; `machine_exports_check` (`cargo xtask static`) holds the live tree to the map.
        let fx = machine_fixture(&[]);
        let t = |f: &str, tr: Tier| select_tier(&fx, &[f.to_string()], tr);
        // GATE-MATH-SHARD: a direct-tier selection of scan.S or fold.S takes about 20 s in the
        // debug build (it follows every user of their symbols), and this test asked for the same
        // two selections six times (390 s on a loaded CI host). Each is computed once, the two
        // side by side; every check below reads the same selections as before.
        let (scan, fold) = std::thread::scope(|sc| {
            let scan = sc.spawn(|| t("csys/src/machine/scan.S", Tier::Direct));
            let fold = t("csys/src/machine/fold.S", Tier::Direct);
            (scan.join().expect("scan.S selection"), fold)
        });
        let direct = |f: &str| if f.ends_with("scan.S") { &scan } else { &fold };
        for f in ["csys/src/machine/scan.S", "csys/src/machine/fold.S"] {
            let d = direct(f);
            assert!(d.full.is_empty(), "{f}: a listed export must not escalate in the direct tier: {:?}", d.full);
            assert!(has_test(d, "machine_scanners") && has_test(d, "s04_strings_symbols"), "{f}: {:?}", d.tests);
            assert!(!t(f, Tier::Full).full.is_empty(), "{f}: --tier full stays FULL");
        }
        assert!(has_test(&fold, "small_copy"));
        // CSYS-MAP-4: the scanner race tests run when the scanners they call (memchr, strcmp in scan.S; memcmp in fold.S) change.
        for f in ["csys/src/machine/scan.S", "csys/src/machine/fold.S"] {
            assert!(has_test(direct(f), "scanner_races"), "{f}: scanner_races must be selected in the direct tier");
        }
        // fold.S defines `liella_machine_chr_limit`, read by scan.S: the sibling is mapped as if it changed (direct tier).
        let d = &fold;
        assert!(d.notes.iter().any(|n| n.contains("mapped as if it changed") && n.contains("scan.S")), "{:?}", d.notes);
        for f in ["csys/src/machine/setjmp.S", "csys/src/machine/context.S", "csys/src/machine/entry.S"] {
            assert!(!t(f, Tier::Direct).full.is_empty(), "{f}: an unlisted export must stay FULL in the direct tier");
        }
        // The fixture is consistent with the map (each listed export mapped to existing targets that name it).
        assert_eq!(machine_exports_problems(&fx), None);
        // An export the map lacks: the direct tier escalates (safe), and the static check names the symbol and the fix.
        let fx = machine_fixture(&["zz_unlisted_export"]);
        assert!(!select_tier(&fx, &["csys/src/machine/scan.S".to_string()], Tier::Direct).full.is_empty());
        let p = machine_exports_problems(&fx).unwrap_or_default();
        assert!(p.len() == 1 && p[0].contains("`zz_unlisted_export`") && p[0].contains("add (\"scan.S\", \"zz_unlisted_export\""), "{p:?}");
    }

    /// CSYS-MAP-2 fixture: the exported (non-`liella_`) symbols of the listed machine files, fixed; scan.S gets `extra` too.
    const FIXTURE_EXPORTS: &[(&str, &[&str])] = &[
        ("fold.S", &["memmove", "memcpy", "mempcpy", "__mempcpy", "memset", "memcmp", "bcmp", "__memcmpeq"]),
        ("scan.S", &["memchr"]),
    ];

    /// The live repository with the listed machine files' exports replaced by `FIXTURE_EXPORTS` (their `liella_`
    /// symbols, which tie the files to their Rust users and to each other, stay live).
    fn machine_fixture(extra: &[&str]) -> Repo {
        let mut r = Repo { hdr_diff: None, ..Repo::load(&root()) };
        for (file, exports) in FIXTURE_EXPORTS {
            let c = r.csys.get_mut(&format!("csys/src/machine/{file}")).expect("listed machine file");
            let mut syms: Vec<String> = c.symbols.take().expect("readable").into_iter().filter(|s| s.starts_with("liella_")).collect();
            syms.extend(exports.iter().map(|s| s.to_string()));
            if *file == "scan.S" {
                syms.extend(extra.iter().map(|s| s.to_string()));
            }
            c.symbols = Some(syms);
        }
        r
    }

    #[test]
    fn csys_symbol_reading() {
        assert_eq!(csys_symbols("a.S", "\t.globl liella_a, liella_b\n\t.weak w\nliella_c:\n.Llocal:\n\tret\n"), Some(vec!["liella_a".into(), "liella_b".into(), "liella_c".into(), "w".into()]));
        assert_eq!(csys_symbols("a.S", "#include \"x.h\"\n\t.globl liella_a\n"), None);
        assert_eq!(csys_symbols("a.S", ".macro M\n.endm\n"), None);
        // TOOLS-9: a one-line .symver alias maps to the alias and the exported name; a malformed one stays unmappable.
        assert_eq!(
            csys_symbols("a.S", "\t.globl liella_x_alias\nliella_x_alias:\n\tjmp liella_x@PLT\n\t.symver liella_x_alias, stime@GLIBC_2.2.5\n"),
            Some(vec!["liella_x_alias".into(), "stime".into()])
        );
        assert_eq!(csys_symbols("a.S", "\t.symver a, b@@V\n"), Some(vec!["a".into(), "b".into()]));
        assert_eq!(csys_symbols("a.S", "\t.symver a\n"), None);
        assert_eq!(csys_symbols("a.S", "\t.symver a, b\n"), None);
        assert_eq!(csys_symbols("a.c", "long liella_a(void) { return 0; }\n"), None);
        let syms = BTreeSet::from(["liella_a".to_string()]);
        let mut out = BTreeMap::new();
        sym_refs("x.rs", "    pub(crate) safe fn liella_a(s: *const u8) -> usize;\n    /// As `liella_a`\n    let n = ffi::liella_a(p);\n", &syms, &mut out);
        let r = &out["liella_a"];
        assert_eq!(r.iter().map(|r| r.decl).collect::<Vec<_>>(), [true, false], "a declaration, a comment (skipped), a use");
    }

    #[test]
    fn csys_crate_compiles_csys() {
        assert!(read(&root().join("crates/boundary/build.rs")).contains("csys"));
        assert!(repo().crates.iter().any(|c| c.name == CSYS_CRATE && c.dir == "crates/boundary"));
    }

    #[test]
    fn an_audit_module_selects_its_checks_not_full() {
        let (_, s) = sel(&["xtask/src/policy.rs"]);
        assert!(s.full.is_empty() && s.xtask_tests && s.clippy.contains_key("xtask") && s.tests.is_empty() && s.conf.is_empty(), "{s:?}");
        let (_, s) = sel(&["xtask/src/symgap.rs"]);
        assert!(s.full.is_empty() && s.xtask_cmds.contains("symgap"));
        for f in ["xtask/src/so.rs", "xtask/src/spec.rs", "xtask/src/csys.rs", "xtask/src/main.rs"] {
            assert!(!sel(&[f]).1.full.is_empty(), "{f} must escalate to full");
        }
    }

    #[test]
    fn audit_modules_are_leaves() {
        let audit: Vec<&str> = XTASK_AUDITS.iter().map(|(m, _)| *m).collect();
        let mut files = Vec::new();
        files_under(&root().join("xtask/src"), &["rs"], &mut files);
        for p in files {
            let rel = p.strip_prefix(root()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            if rel == "xtask/src/main.rs" || rel == "xtask/src/affected.rs" || audit.contains(&rel.as_str()) {
                continue;
            }
            let text = read(&p);
            for m in &audit {
                let stem = m.trim_start_matches("xtask/src/").trim_end_matches(".rs");
                assert!(!text.contains(&format!("{stem}::")), "{rel} uses the audit module {stem}: it is no longer a leaf, take it out of XTASK_AUDITS");
            }
        }
    }

    #[test]
    fn a_conformance_header_selects_its_includers() {
        let (repo, s) = sel(&["conformance/c/not_included.h"]);
        assert!(s.full.is_empty() && s.conf.len() == repo.conf.len(), "a header nobody includes by name: every program");
        assert!(!sel(&["conformance/c/sub/x.h"]).1.full.is_empty());
    }

    #[test]
    fn only_mode_selects_exactly_the_named_binaries() {
        let repo = repo();
        let s = select_only(repo, "s12_stdio_clauses,conformance:s12_printf,lib:liella-math").unwrap_or_else(|e| panic!("{e}"));
        assert!(s.only && s.full.is_empty() && s.tests.len() == 1 && has_test(&s, "s12_stdio_clauses"));
        assert!(s.conf.len() == 1 && s.conf.contains_key("s12_printf") && s.libs.len() == 1 && s.clippy.is_empty());
        let krate = &s.tests.keys().next().map(|(c, _)| c.clone()).unwrap_or_default();
        assert!(select_only(repo, &format!("{krate}::s12_stdio_clauses")).is_ok());
        for bad in ["no_such_target", "conformance:no_such", "lib:no-such", "", "liella-thread-db::s12_stdio_clauses"] {
            assert!(select_only(repo, bad).is_err(), "{bad}");
        }
    }

    fn tier(files: &[&str], t: Tier) -> Selection {
        select_tier(repo(), &files.iter().map(|s| s.to_string()).collect::<Vec<_>>(), t)
    }

    // GATE-TIER-2 red-before-green: a conformance program is C, so no module name reaches it; it is reached through the
    // entry points it calls. s12_printf calls `printf`/`snprintf` (csys -> `liella_vfprintf4` etc., whose bodies name
    // boundary::printf, a one-hop user of core::printf). Before: 0 conformance programs for a core printf change.
    #[test]
    fn direct_tier_reaches_conformance_programs_through_their_entry_points() {
        let repo = repo();
        assert!(repo.conf.iter().any(|c| c.name == "s12_printf" && c.calls.contains("printf")), "s12_printf calls printf");
        let d = tier(&["crates/core/src/printf.rs"], Tier::Direct);
        assert!(d.full.is_empty() && d.conf.contains_key("s12_printf"), "{:?}", d.conf.keys().collect::<Vec<_>>());
        assert!(d.conf["s12_printf"].contains("calls `"), "{}", d.conf["s12_printf"]);
        // Only programs calling an entry that reaches the change: one calling no stdio entry is not selected.
        let w = tier(&["crates/core/src/printf.rs"], Tier::Wide);
        assert!(d.conf.keys().all(|c| repo.conf.iter().any(|x| x.name == *c && !x.calls.is_empty())), "every selected program calls an entry");
        assert!(d.conf.len() < repo.conf.len(), "direct {} of {} programs (wide {})", d.conf.len(), repo.conf.len(), w.conf.len());
        // A program reached through an entry shares the entry module's section: s00_errno's `fclose` does not pull it in.
        let s = tier(&["crates/boundary/src/stdio.rs"], Tier::Direct);
        for (c, why) in s.conf.iter().filter(|(_, w)| w.contains("calls `")) {
            let secs = &repo.conf.iter().find(|x| x.name == *c).unwrap_or_else(|| panic!("{c}")).sections;
            assert!(secs.contains(&12), "{c} selected through an entry of another section: {why}");
        }
    }

    // GATE-TIER-2 red-before-green: a module re-exported at its crate root resolves to the modules naming the
    // re-exported items (`liella_kernel::Errno`), so it stays direct. Before: wide (every dependent crate).
    #[test]
    fn direct_tier_resolves_re_exports_to_their_users() {
        let repo = repo();
        let kernel = repo.crates.iter().find(|c| c.dir == "crates/kernel").map(|c| c.name.clone()).unwrap_or_default();
        let root = repo.nodes.iter().find(|n| n.root && n.krate == kernel).unwrap_or_else(|| panic!("kernel root"));
        assert!(root.reexports.iter().any(|st| st.contains("errno") && st.contains("Errno") && !st.contains("*")), "kernel re-exports errno::Errno");
        let d = tier(&["crates/kernel/src/errno.rs"], Tier::Direct);
        assert!(d.full.is_empty() && d.wide_crates.is_empty(), "stays direct: wide {:?}", d.wide_crates);
        // Library modules import `liella_kernel::errno::Errno`; test targets name the re-export (`liella_kernel::Errno`).
        let via_item: Vec<&Target> = repo.targets.iter().filter(|t| t.krate != kernel && t.refs.contains(&(Some(kernel.clone()), "Errno".to_string()))).collect();
        assert!(!via_item.is_empty(), "some test target names the re-exported `Errno`");
        for t in via_item {
            assert!(d.tests.contains_key(&(t.krate.clone(), t.name.clone())), "{}::{} names {kernel}::Errno but is not selected", t.krate, t.name);
        }
        assert!(d.tests.len() < tier(&["crates/kernel/src/errno.rs"], Tier::Wide).tests.len());
    }

    // GATE-TIER: the direct tier keeps every target that names the changed module, and is narrower than wide.
    #[test]
    fn direct_tier_keeps_the_namers_and_is_narrower_than_wide() {
        let repo = repo();
        for (f, m) in [("crates/core/src/printf.rs", "printf"), ("crates/core/src/locale/mod.rs", "locale")] {
            let (d, w) = (tier(&[f], Tier::Direct), tier(&[f], Tier::Wide));
            assert!(d.full.is_empty() && d.direct, "{f}: {:?}", d.full);
            for k in reachers(repo, m) {
                assert!(d.tests.contains_key(&k), "{f}: {k:?} names {m}:: but is not in the direct tier");
            }
            assert!(d.tests.keys().all(|k| w.tests.contains_key(k)), "{f}: direct must be inside wide");
            assert!(d.tests.len() < w.tests.len(), "{f}: direct {} vs wide {} targets", d.tests.len(), w.tests.len());
            assert!(d.libs.contains_key("liella"), "{f}: the owning crate's unit tests");
        }
        let d = tier(&["crates/core/src/printf.rs"], Tier::Direct);
        assert!(has_test(&d, "printf_products") && !has_test(&d, "s18_processes_clauses"), "{:?}", d.tests.keys());
    }

    // GATE-TIER red-before-green: the one-hop users are followed. Some target names a one-hop user of
    // core::printf (not the module itself) and is selected only through it; dropping the users fails here.
    #[test]
    fn direct_tier_follows_one_hop_users() {
        let repo = repo();
        let d = tier(&["crates/core/src/printf.rs"], Tier::Direct);
        let users: Vec<&Node> = repo.nodes.iter().filter(|n| d.modules.contains(&(n.krate.clone(), n.name.clone())) && !(n.krate == "liella" && n.name == "printf")).collect();
        assert!(!users.is_empty(), "core::printf has one-hop users");
        // Some change in core selects a target through the name of a one-hop user (not the changed module).
        let mut via_user = Vec::new();
        for n in repo.nodes.iter().filter(|n| n.krate == "liella" && !n.root) {
            let f = format!("crates/core/src/{}.rs", n.name);
            let s = tier(&[f.as_str()], Tier::Direct);
            via_user.extend(s.tests.iter().filter(|(_, why)| why.contains("names `") && !why.contains(&format!("names `{}`", n.name))).map(|(k, _)| (n.name.clone(), k.clone())));
        }
        assert!(!via_user.is_empty(), "no target is ever selected through a one-hop user");
        // Not transitive: a user of a user is not a module of the selection unless it names printf itself.
        let w = tier(&["crates/core/src/printf.rs"], Tier::Wide);
        assert!(d.modules.len() < w.modules.len(), "direct {} vs wide {} modules", d.modules.len(), w.modules.len());
    }

    // GATE-TIER: the tier does not touch the escalations, and `full` is everything.
    #[test]
    fn direct_tier_keeps_the_full_escalations_and_root_files_stay_wide() {
        for f in ["csys/src/x.c", "crates/abi/src/lib.rs", "Cargo.toml", "crates/boundary/build.rs", "xtask/src/main.rs", "spec/clauses.tsv", "include/liella/native.h", "crates/libc-so/src/lib.rs"] {
            assert!(!tier(&[f], Tier::Direct).full.is_empty(), "{f} must stay FULL in the direct tier");
        }
        assert!(!tier(&["docs/x.md"], Tier::Full).full.is_empty(), "--tier full escalates");
        let (repo, d) = (repo(), tier(&["crates/core/src/lib.rs"], Tier::Direct));
        assert_eq!(d.conf.len(), repo.conf.len(), "a crate root is still wide");
        let d = tier(&["crates/core/tests/printf_products.rs"], Tier::Direct);
        assert!(d.tests.len() == 1 && has_test(&d, "printf_products"), "a test file selects its target");
    }

    // GATE-TIER-3: an rt-internal module reaches its own crate's test targets (REALLOC-GROW-2: a heap.rs plant must run
    // liella-rt::heap_and_sync). Every liella-rt target naming an rt module is selected for a change to it.
    #[test]
    fn direct_tier_selects_the_own_crate_targets_of_an_internal_module() {
        let repo = repo();
        let d = tier(&["crates/rt/src/heap.rs"], Tier::Direct);
        assert!(d.tests.contains_key(&("liella-rt".to_string(), "heap_and_sync".to_string())), "{:?}", d.tests.keys().collect::<Vec<_>>());
        assert!(d.libs.contains_key("liella-rt"));
        for n in repo.nodes.iter().filter(|n| n.krate == "liella-rt" && !n.root) {
            let s = tier(&[format!("crates/rt/src/{}.rs", n.name).as_str()], Tier::Direct);
            for t in repo.targets.iter().filter(|t| t.krate == "liella-rt" && t.refs.contains(&(Some("liella-rt".to_string()), n.name.clone()))) {
                assert!(s.tests.contains_key(&(t.krate.clone(), t.name.clone())), "{} not selected for rt::{}", t.name, n.name);
            }
        }
    }

    // GATE-TIER-3 red-before-green: one file of `tests/oracle/` selects the targets whose own source names it (or names
    // the run script that names it), not every target naming the directory (AIO-510: 48 targets for aio_diff.c).
    #[test]
    fn direct_tier_maps_a_support_file_to_the_targets_naming_it() {
        let repo = repo();
        let all = repo.targets.iter().filter(|t| t.krate == "liella" && t.support.contains("oracle")).count();
        let d = tier(&["crates/core/tests/oracle/aio_diff.c"], Tier::Direct);
        assert!(d.tests.contains_key(&("liella".to_string(), "aio_products".to_string())), "{:?}", d.tests.keys().collect::<Vec<_>>());
        assert!(d.tests.len() < all, "{} of {all} oracle targets", d.tests.len());
        for (t, why) in &d.tests {
            let tt = repo.targets.iter().find(|x| x.krate == t.0 && x.name == t.1).unwrap_or_else(|| panic!("{t:?}"));
            assert!(tt.text[..tt.own].contains("aio_"), "{t:?} selected without naming the file: {why}");
        }
        // A Rust file of a support directory keeps the directory rule (it is reached through `mod`).
        let names = |f: &str| support_namers(repo, "crates/core", "liella", &f.split('/').skip(3).collect::<Vec<_>>(), f);
        assert!(names("crates/core/tests/oracle/x.rs").is_none());
        assert!(names("crates/core/tests/oracle/no_such_file_named.c").is_none(), "an unnamed file keeps the directory rule");
    }

    // GATE-TIER-3 red-before-green: a sectionless core module does not pull in other sections' programs through
    // sectionless cross-crate helpers (pattern -> boundary::process -> `_exit` -> s01_abort); printf still reaches s12.
    #[test]
    fn direct_tier_follows_cross_crate_users_only_into_matching_sections() {
        let d = tier(&["crates/core/src/pattern/mod.rs"], Tier::Direct);
        for p in ["s01_abort", "s01_argp", "s01_cxa_finalize"] {
            assert!(!d.conf.contains_key(p), "{p}: {}", d.conf[p]);
        }
        let p = tier(&["crates/core/src/printf.rs"], Tier::Direct);
        assert!(p.conf.contains_key("s12_printf"), "{:?}", p.conf.keys().collect::<Vec<_>>());
    }

    // GATE-TIER-3b: an oracle diff prints its results: a change to printf's entry (or a module it names) reaches the tests
    // whose C support files call it.
    #[test]
    fn direct_tier_reaches_tests_whose_c_support_files_call_the_entry() {
        for f in ["crates/boundary/src/printf.rs", "crates/boundary/src/posix/s12_stdio.rs"] {
            let d = tier(&[f], Tier::Direct);
            assert!(d.tests.contains_key(&("liella".to_string(), "aio_products".to_string())), "{f}: {:?}", d.tests.keys().collect::<Vec<_>>());
        }
    }

    // GATE-TIER: the replay finds a failure outside the tier and passes one inside it; `--only` records are skipped.
    #[test]
    fn replay_reports_failures_outside_the_tier() {
        let repo = repo();
        let inside = "LOG a\nFILES crates/core/src/printf.rs\nFAILS FAILED: liella::printf_products x|STEP FAILED: test -p liella|\n";
        let (out, misses) = replay(repo, inside, Tier::Direct);
        assert_eq!(misses, 0, "{out}");
        let outside = "LOG b\nFILES crates/core/src/printf.rs\nFAILS FAILED: liella-boundary::s18_processes_symbols x|\nLOG c\nFILES \nFAILS FAILED: liella::y z|\n";
        let (out, misses) = replay(repo, outside, Tier::Direct);
        assert_eq!(misses, 1, "{out}");
        assert!(out.contains("MISS liella-boundary::s18_processes_symbols") && out.contains("c: no changed files"), "{out}");
        let (out, misses) = replay(repo, &format!("FLAKE s18_processes_symbols x\n{outside}"), Tier::Direct);
        assert!(misses == 0 && out.contains("1 outside but known flakes"), "{out}");
        let (_, misses) = replay(repo, "LOG d\nFILES Cargo.toml\nFAILS FAILED: liella::anything x|\n", Tier::Direct);
        assert_eq!(misses, 0, "a FULL selection covers everything");
    }

    #[test]
    fn every_posix_module_selects_its_own_section() {
        let repo = repo();
        for n in repo.nodes.iter().filter(|n| n.section.is_some() && !n.name.ends_with("_gen")) {
            let sec = n.section.unwrap_or(0);
            let file = format!("crates/boundary/src/posix/{}.rs", n.name);
            let s = select(repo, std::slice::from_ref(&file));
            let eligible = repo.dependents("liella-boundary");
            assert!(s.full.is_empty(), "{file}");
            for c in repo.conf.iter().filter(|c| c.sections.contains(&sec)) {
                assert!(s.conf.contains_key(&c.name), "{file}: {} (section {sec}) not selected", c.name);
            }
            for t in repo.targets.iter().filter(|t| t.sections.contains(&sec) && eligible.contains(&t.krate)) {
                assert!(s.tests.contains_key(&(t.krate.clone(), t.name.clone())), "{file}: {} (section {sec}) not selected", t.name);
            }
        }
    }

    #[test]
    fn section_and_reference_extraction() {
        assert_eq!(sections_of("s12_stdio_clauses", ""), BTreeSet::from([12]));
        assert_eq!(sections_of("printf_products", "//! spec §12.6; D-7.10; s3_2_c_1_x; bars3_2
fn f() {} // §9"), BTreeSet::from([3, 7, 12]));
        let aliases = BTreeMap::from([("liella".to_string(), "liella".to_string()), ("liella_kernel".to_string(), "liella-kernel".to_string())]);
        let r = refs_of("use crate::{printf, fs::{dir}, stream as s};
use liella::{ctype, text::x};
use std::{fmt, mem};
mod hidden;
use liella_kernel::errno::Errno;
fn f() { let x = locale::get(); std::mem::swap(a, b); liella::time::now(); }
// whole::line
fn plain() {}", &aliases);
        let own = |m: &str| (None, m.to_string());
        let of = |c: &str, m: &str| (Some(c.to_string()), m.to_string());
        for want in [own("printf"), own("fs"), own("locale"), of("liella", "ctype"), of("liella", "text"), of("liella", "time"), of("liella-kernel", "errno")] {
            assert!(r.contains(&want), "{want:?} in {r:?}");
        }
        for not in ["hidden", "whole", "plain", "fmt", "mem", "s", "std", "x"] {
            assert!(!r.iter().any(|(_, m)| m == not), "{not} in {r:?}");
        }
    }

    #[test]
    fn summary_matches_the_test_summary_format() {
        let log = "test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.1s\ntest result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out\n---- foo::bar stdout ----\nconformance: 5 passed, 0 failed\n";
        let s = summary(log);
        assert!(s.starts_with("suites: 2 (1 ok, 1 failed); tests: 13 passed, 2 failed, 1 ignored"), "{s}");
        assert!(s.contains("FAILED: foo::bar") && s.contains("conformance: 5 passed"));
    }

    #[test]
    fn summary_keeps_each_errors_location() {
        let log = "error: unused variable: `x`\n  --> crates/core/src/a.rs:3:9\n   |\nerror[E0308]: mismatched types\n   --> crates/core/src/b.rs:7:1\n  --> crates/core/src/c.rs:1:1\nerror: unused variable: `x`\n  --> crates/core/src/d.rs:5:5\nerror: could not compile `liella`\n";
        let s = summary(log);
        assert!(s.contains("error: unused variable: `x` [crates/core/src/a.rs:3:9]\n"), "{s}");
        assert!(s.contains("error[E0308]: mismatched types [crates/core/src/b.rs:7:1]\n"), "{s}");
        assert!(s.contains("error: could not compile `liella`\n") && !s.contains("c.rs") && !s.contains("d.rs"), "{s}");
    }
}
