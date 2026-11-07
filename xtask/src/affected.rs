//! `cargo xtask affected (--list | --run) [--base <ref>] [--files <path>...] [--only <names>]`: list (`--list`) or run
//! (`--run`) only the tests a change can affect (PR-4; owner item: token cost). One of `--list` and `--run` is required:
//! without either the command prints the one-line usage and exits 2 (XTASK-AFFECTED-EXPLICIT; a gate that forgot `--run`
//! listed the selection, ran nothing and exited 0).
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
//!    toolchain file, `xtask/` (but this module and the audit modules, rule 7, and xtask test code: `xtask/tests/`
//!    or a source whose base diff stays inside its trailing `#[cfg(test)]` module, D-X.41), the generated tables and their inputs (`spec/clauses.tsv`, `symbols.tsv`,
//!    `primitives.tsv`, `spec/abi`, `spec/unicode`, `*_gen.rs`, `tables.rs`, `builtin_items.rs`,
//!    `include/`; but the math wiring, coefficients, fits and tables generator, rule 8), the product crates, linker and export lists,
//!    any non-Rust file under a crate's `src/`, and any path this table does not know.
//! 2. DOCS ONLY (`docs/`, `plan/`, `handoff/`, `results/`, top-level `*.md`, `spec/**/*.md`, `spec/behaviour`,
//!    `spec/schedule.tsv`, and the operator tools under the top-level `tools/`, e.g. the probe service, which no test
//!    reaches): `stubs` (which runs `trace`), the policy checks and `cargo xtask static`, no tests; but a file that code
//!    names as a path (a quoted path on a non-comment line, e.g. a test's `spec/behaviour/*.glibc.txt` record or
//!    `docs/codemap/symbols.tsv`, which this command reads) selects its readers: that test target, the crate's unit
//!    tests, or the xtask unit tests (XTASK-AFFECTED-DOCS-PATHS, D-X.35).
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
//! 7. An xtask audit/report module (`XTASK_AUDITS`: policy, symgap, status, schedule, plants): the
//!    xtask unit tests, clippy, the policy checks and the module's own command.
//! 8. XTASK-AFFECTED-MATH (ruling #3188): the math export wiring (`crates/boundary/src/posix/s10_math.tsv`,
//!    `s10_math_gen.rs`, `csys/src/adapter/math_*.c`) selects the math export tests (`Selection::math_exports`: the
//!    `s10_math_gen` module as if changed, every `liella-math` target, every target whose own source names `s10_math`
//!    or `libm` or carries section 10, the conformance programs of section 10 or calling a math export) plus `cargo
//!    xtask math-gen --check`; a coefficient file `crates/math/src/coeffs/<family>.rs` selects the `coeffs` module as if
//!    changed, the same math export tests and `cargo xtask tables --check`. XTASK-AFFECTED-MATH-2 (ruling #3204): a
//!    fit file `crates/math/fits/<name>.fit` selects what a coefficient file selects plus `cargo xtask remez --check
//!    <fit>` (a removed fit: FULL); the tables generator `crates/mathtools/src/tables.rs` selects its own mapping as a
//!    `liella-mathtools` source, what a coefficient file selects, and `cargo xtask tables --check` / `kernel-index
//!    --check`. In every tier but `--tier full`; the other files under `fits/` (the `.py` table scripts, subdirectories),
//!    other generated files and anything else of rule 1 still escalate.
//!
//! TIERS (GATE-TIER, coordinator ruling 10-11): `--tier direct` (the DEFAULT, per-fix gates) applies rule 4 to EVERY
//! crate's non-root module through `Selection::narrow_direct`: the changed module, its one-hop users, the targets
//! naming either (not through hubs or test-support modules), the spec sections of those modules and of the C entries
//! whose bodies name one of them (the products tests reach code only through C entries); no reverse
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
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

/// Crates whose changes are narrowed through the module graph.
const NARROW: &[&str] = &["liella", "liella-boundary", "liella-legacy", "liella-math"];
/// xtask sources that name csys symbols only to check the csys source or products, from `cargo xtask static` and
/// `cargo xtask so` (D-X.34): a reference from one of them is no user of the symbol.
const STATIC_CHECKERS: &[&str] = &["xtask/src/classed.rs", "xtask/src/jcc.rs"];

// ---------------------------------------------------------------------------------------------
// The repository model

struct Crate {
    name: String,
    dir: String,
    deps: BTreeSet<String>,
    /// The package has a library target (`src/lib.rs` or a `[lib]` section). A bin-only package (xtask) has no
    /// `--lib`: its unit tests are those of its bin targets (XTASK-AFFECTED-XTASK-TESTS).
    lib: bool,
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
    /// XTASK-AFFECTED-MATH-2: the fit files in the tree (`crates/math/fits/<name>.fit`). A changed fit that is not
    /// here was removed, and `cargo xtask remez --check` cannot check it: FULL.
    fits: BTreeSet<String>,
    /// XTASK-AFFECTED-DOCS-PATHS: the xtask sources (path, text) that may read a documentation file; this command's own
    /// unit tests are left out (they name made-up paths, not files they read).
    xtask_texts: Vec<(String, String)>,
    /// XTASK-AFFECTED-XTASK-TESTS (D-X.41): `git diff -U0` of `xtask/` against the base (from git, else the file
    /// `.changed.xtask` a sync may write). `None`: no diff, so a changed xtask source cannot be shown to touch only
    /// its test module (FULL, as before).
    xtask_diff: Option<String>,
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
    let mut in_macro = false;
    for l in text.lines() {
        let t = l.trim_start();
        let first = t.split([' ', '\t']).next().unwrap_or("");
        // CLASSED-SLOT-INFRA (D-4.53): a `.macro` body that declares nothing global and labels only `.L` locals defines no
        // symbol where it is written (its expansions are read where they occur); a `#define` of an upper-case name to a
        // number literal renames nothing. Anything else in either stays unmappable.
        if in_macro {
            let label = l.split_once(':').is_some_and(|(x, _)| !x.is_empty() && !x.starts_with(".L") && !x.contains([' ', '\t']));
            if first == ".endm" {
                in_macro = false;
            } else if label || matches!(first, ".macro" | ".globl" | ".global" | ".weak" | ".symver" | ".set" | ".equ" | ".equiv" | "#define" | "#include") {
                return None;
            }
            continue;
        }
        if first == ".macro" {
            in_macro = true;
            continue;
        }
        if first == "#define" {
            let mut w = t[first.len()..].split_whitespace();
            let name_ok = w.next().is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'));
            let value_ok = w.next().is_some_and(|v| v.bytes().all(|b| b.is_ascii_hexdigit() || b == b'x')) && w.next().is_none();
            if name_ok && value_ok {
                continue;
            }
            return None;
        }
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

    /// XTASK-PLANTS-PRODUCTS-REBUILD: does test target `name` (of package `krate`, or of any package) use the products,
    /// by the rule `run` uses to build them? `None` when no target or more than one has that name.
    pub fn test_uses_products(&self, krate: Option<&str>, name: &str) -> Option<bool> {
        let mut found = self.targets.iter().filter(|t| t.name == name && krate.is_none_or(|k| t.krate == k));
        let first = found.next()?;
        if found.next().is_some() {
            return None;
        }
        Some(first.products)
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
            let manifest = read(&root.join(dir).join("Cargo.toml"));
            let (name, deps) = parse_manifest(&manifest);
            if !name.is_empty() {
                let lib = root.join(dir).join("src/lib.rs").is_file() || manifest.lines().any(|l| l.trim() == "[lib]");
                crates.push(Crate { name, dir: dir.clone(), deps, lib });
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
        let mut xs = Vec::new();
        files_under(&root.join("xtask/src"), &["rs"], &mut xs);
        let xtask_texts = xs
            .iter()
            .map(|p| {
                let text = read(p);
                let text = if p.ends_with("xtask/src/affected.rs") { text.split("\n#[cfg(test)]\nmod tests {").next().unwrap_or_default().to_string() } else { text };
                (rel(p), text)
            })
            .collect();
        Repo { crates, support_texts, closure, nodes, targets, conf, hubs, test_only, csys, entries, entry_hubs, csys_refs, hdr_diff: fs::read_to_string(root.join(".changed.hdr")).ok(), fits: fit_files(root), xtask_texts, xtask_diff: fs::read_to_string(root.join(".changed.xtask")).ok() }
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

    /// XTASK-AFFECTED-MATH (ruling #3188): the tests that can observe the math exports, for a change of their wiring
    /// (`s10_math.tsv`, `s10_math_gen.rs`, the `math_*.c` forwarders) or of a generated coefficient file. The export
    /// module itself as if it had changed (rule 4 of the tier: its users, the targets naming it, the conformance
    /// programs calling its entries), every test target of `liella-math` (the libm product, clause and kernel suites),
    /// every target of any crate whose own source names `s10_math` or `libm` or carries section 10, the unit tests of
    /// both crates, and every conformance program that carries section 10 or calls an entry of the export modules.
    fn math_exports(&mut self, repo: &Repo, why: &str) {
        let via = |what: &str| format!("{why}: {what}");
        if self.direct {
            self.narrow_direct(repo, "liella-boundary", "s10_math_gen", MATH_GEN);
        } else {
            self.narrow(repo, "liella-boundary", "s10_math_gen", MATH_GEN);
        }
        for c in [MATH_EXPORTS_CRATE, MATH_CRATE] {
            add(&mut self.libs, c.to_string(), &via("the unit tests of the export and kernel crates"));
            add(&mut self.clippy, c.to_string(), &via("the export and kernel crates"));
        }
        for t in &repo.targets {
            let own = &t.text[..t.own.min(t.text.len())];
            if t.krate == MATH_CRATE {
                add(&mut self.tests, (t.krate.clone(), t.name.clone()), &via("a liella-math suite (they call the math exports)"));
            } else if own.contains("s10_math") || own.contains("libm") || t.sections.contains(&10) {
                add(&mut self.tests, (t.krate.clone(), t.name.clone()), &via("the test names s10_math or libm, or carries section 10"));
            }
        }
        let export_nodes: BTreeSet<usize> = repo.nodes.iter().enumerate().filter(|(_, n)| n.krate == MATH_EXPORTS_CRATE && (n.name == "s10_math_gen" || n.name == "s10_math")).map(|(i, _)| i).collect();
        for c in &repo.conf {
            if c.sections.contains(&10) {
                add(&mut self.conf, c.name.clone(), &via("the program carries section 10"));
            } else if let Some(sym) = c.calls.iter().find(|s| repo.entries[*s].iter().any(|e| export_nodes.contains(&e.node))) {
                add(&mut self.conf, c.name.clone(), &via(&format!("the program calls the math export `{sym}`")));
            }
        }
    }

    /// XTASK-AFFECTED-MATH: what a generated coefficient file selects (also for its fit and the tables generator,
    /// XTASK-AFFECTED-MATH-2). The family's users: the `coeffs` module as if it had changed (its one-hop users in the
    /// tier), the math export tests and `cargo xtask tables --check`.
    fn math_coeffs(&mut self, repo: &Repo, f: &str) {
        if self.direct {
            self.narrow_direct(repo, MATH_CRATE, "coeffs", f);
        } else {
            self.narrow(repo, MATH_CRATE, "coeffs", f);
        }
        self.math_exports(repo, &format!("{f}: generated coefficients"));
        self.xtask_cmds.insert("tables --check".to_string());
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
    ///   user (never section 0 by default, never "carries no section"),
    /// * the targets carrying the spec section of a C entry (a `posix/sNN` definition) whose body names the module or
    ///   a one-hop user that is not a hub, an entry hub or test support: the products tests reach a helper module this way.
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
        // XTASK-PLANTS-TRACKED-COPY: the sections of the C entries the change reaches. A test that diffs the products
        // (`catalogs_products`) reaches a module only through the entries its C program calls, so it names no module and
        // carries only sections; a module behind a helper (legacy::intl, used by boundary::catalog, which the
        // `posix/s07_locale` entries name) has no section of its own nor a `posix/sNN` one-hop user, and selected none of
        // them. An entry counts when its body names the module or a one-hop user that is neither a hub, an entry hub nor
        // test support (as for conformance programs, which have their own call-based rule below and do not use these).
        let entry_names = |e: &Entry, i: usize| e.refs.iter().any(|(m_c, m)| repo.nodes[i].name == *m && repo.nodes[i].krate == *m_c.as_deref().unwrap_or(repo.nodes[e.node].krate.as_str()));
        let mut entry_secs: BTreeMap<u32, String> = BTreeMap::new();
        for (sym, es) in &repo.entries {
            for e in es {
                let Some(s) = repo.nodes[e.node].section.filter(|s| !sections.contains(s) && !entry_secs.contains_key(s)) else {
                    continue;
                };
                if let Some(i) = named.iter().copied().find(|&i| !repo.entry_hubs.contains(&i) && entry_names(e, i)) {
                    entry_secs.insert(s, format!("`{sym}` in {} names {}", label(e.node), label(i)));
                }
            }
        }
        if !entry_secs.is_empty() {
            self.notes.push(format!("{file}: DIRECT tier: sections of the C entries reached: {}", entry_secs.iter().map(|(s, w)| format!("{s} ({w})")).collect::<Vec<_>>().join(", ")));
        }
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
            } else if let Some((s, w)) = entry_secs.iter().find(|(s, _)| t.sections.contains(s)) {
                via(&format!("spec section {s} of a C entry the change reaches ({w})"))
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
    /// XTASK-AFFECTED-XTASK-TESTS: xtask test code only (`xtask/tests/`, or a source whose diff stays inside its
    /// trailing `#[cfg(test)]` module): the xtask unit tests, clippy and `cargo xtask static`, not FULL.
    XtaskTestCode,
    /// XTASK-AFFECTED-MATH: the math export wiring (`s10_math.tsv`, `s10_math_gen.rs`, `csys/src/adapter/math_*.c`).
    MathWiring,
    /// XTASK-AFFECTED-MATH: a generated coefficient file `crates/math/src/coeffs/<family>.rs`.
    MathCoeffs,
    /// XTASK-AFFECTED-MATH-2: a fit file `crates/math/fits/<name>.fit` (the input of `cargo xtask remez`).
    MathFit,
    /// XTASK-AFFECTED-MATH-2: the tables generator `crates/mathtools/src/tables.rs` (`cargo xtask tables`).
    MathTables,
    /// D-X.39: another `crates/mathtools/src/` source that a generator is built from (`mathtools_feeds`): its own
    /// mapping plus the check of each generated set it feeds.
    MathToolFeed(Feeds),
}

/// D-X.39: which generated sets a liella-mathtools source feeds. `remez`: the fitted coefficient files
/// (`cargo xtask remez --check <every fit>`); `tables`: the constant tables and `crates/math/KERNELS.md`
/// (`cargo xtask tables --check`, `kernel-index --check`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Feeds {
    pub remez: bool,
    pub tables: bool,
}

/// D-X.39: the generator inputs under `crates/mathtools/src/` (the fitting code reached CI without `remez --check`
/// before this map). The remez binary is built from `remez.rs` and `mp/`; the tables binary from `tables.rs`, the
/// `*_tables.rs` modules and `mp/`; `lib.rs` declares both. `cmp.rs`, `cplx.rs` and `bin/mathcmp.rs` feed no
/// generated file (the comparison tool): `None`, the ordinary mapping only. `tables.rs` keeps its own class.
fn mathtools_feeds(path: &str) -> Option<Feeds> {
    let f = path.strip_prefix("crates/mathtools/src/")?;
    let (remez, tables) = match f {
        "remez.rs" | "bin/remez.rs" => (true, false),
        "bessel_tables.rs" | "gamma_tables.rs" | "special_tables.rs" | "bin/tables.rs" => (false, true),
        "lib.rs" => (true, true),
        _ if f.starts_with("mp/") && f.ends_with(".rs") => (true, true),
        _ => return None,
    };
    Some(Feeds { remez, tables })
}

const MATH_EXPORTS_CRATE: &str = "liella-boundary";
const MATH_CRATE: &str = "liella-math";
const MATH_TSV: &str = "crates/boundary/src/posix/s10_math.tsv";
const MATH_GEN: &str = "crates/boundary/src/posix/s10_math_gen.rs";
const MATH_FITS: &str = "crates/math/fits/";
const MATH_TABLES_GEN: &str = "crates/mathtools/src/tables.rs";

/// XTASK-AFFECTED-MATH-2: the fit files of the tree, `crates/math/fits/<name>.fit` (no subdirectory).
fn fit_files(root: &Path) -> BTreeSet<String> {
    let Ok(dir) = fs::read_dir(root.join(MATH_FITS)) else { return BTreeSet::new() };
    dir.filter_map(Result::ok).filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".fit")).map(|n| format!("{MATH_FITS}{n}")).collect()
}

/// XTASK-AFFECTED-MATH (ruling #3188): the generated math files that map to the math suites instead of FULL. Only these
/// exact files: the math fits (`crates/math/fits/`), any other `*_gen.rs` and a file in a subdirectory of `coeffs/` or
/// `adapter/` keep escalating. `cargo xtask math-gen --check` / `tables --check` prove the generated files match their
/// inputs; the selected suites prove what the exports do. XTASK-AFFECTED-MATH-2 (ruling #3204): a fit file
/// `crates/math/fits/<name>.fit` (not a subdirectory, not the `.py` table scripts) and exactly the tables generator
/// `crates/mathtools/src/tables.rs` map too (`remez --check` / `tables --check` / `kernel-index --check`).
fn math_class(path: &str) -> Option<Class> {
    if path == MATH_TSV || path == MATH_GEN {
        return Some(Class::MathWiring);
    }
    if path.strip_prefix("csys/src/adapter/math_").is_some_and(|f| f.ends_with(".c") && !f.contains('/')) {
        return Some(Class::MathWiring);
    }
    if path.strip_prefix("crates/math/src/coeffs/").is_some_and(|f| f.ends_with(".rs") && !f.contains('/')) {
        return Some(Class::MathCoeffs);
    }
    if path.strip_prefix(MATH_FITS).is_some_and(|f| f.ends_with(".fit") && !f.contains('/')) {
        return Some(Class::MathFit);
    }
    if path == MATH_TABLES_GEN {
        return Some(Class::MathTables);
    }
    mathtools_feeds(path).map(Class::MathToolFeed)
}

/// xtask modules that only implement report or audit commands: (module, the xtask commands to
/// run when it changes). `policy.rs` (purity, unsafe-audit, csys-audit) runs in every selection
/// anyway. A module here must not be used by another xtask module outside this table, `main.rs`
/// and this command (the unit test `audit_modules_are_leaves` checks it); every other xtask
/// change escalates to FULL.
const XTASK_AUDITS: &[(&str, &[&str])] = &[
    ("xtask/src/policy.rs", &[]),
    ("xtask/src/forkalloc.rs", &[]),
    ("xtask/src/fs_order.rs", &[]),
    // `cargo xtask plants` only (ruling #2993): no other command or gate step runs it.
    ("xtask/src/plants.rs", &[]),
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
    if let Some(c) = math_class(path) {
        return Some(c);
    }
    // XTASK-AFFECTED-XTASK-TESTS (D-X.41): xtask's integration tests run only under `cargo test -p xtask`; no gate
    // step executes them.
    if path.starts_with("xtask/tests/") {
        return Some(Class::XtaskTestCode);
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
    if path == "conformance/README.md" {
        return Some(Class::Docs);
    }
    if path == "conformance/run.sh" || path.starts_with("conformance/tools/") {
        return Some(Class::AllConf);
    }
    // XTASK-AFFECTED-DOCS-PATHS: `results/` (timing and sweep tables) and the prose under `spec/` (`*.md`: the errata,
    // `spec/security/`) join the docs; `docs_readers` then selects whatever names the file in code.
    if path.starts_with("docs/") || path.starts_with("plan/") || path.starts_with("handoff/") || path.starts_with("results/") || path.starts_with("spec/behaviour/") || path == "spec/schedule.tsv" || path.starts_with("spec/tools/") || (path.starts_with("spec/") && path.ends_with(".md")) || path.starts_with("tools/") || (!path.contains('/') && path.ends_with(".md")) || file.starts_with("LICENSE") || file == ".gitignore" {
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

/// XTASK-AFFECTED-XTASK-TESTS (D-X.41): does the change to xtask source `f` stay inside its test module? Needs the
/// base diff (`Repo::xtask_diff`) and the file's current text; anything missing is "no" (FULL).
fn xtask_test_only(repo: &Repo, f: &str) -> bool {
    if !(f.starts_with("xtask/src/") && f.ends_with(".rs")) {
        return false;
    }
    let Some(diff) = repo.xtask_diff.as_deref() else { return false };
    let Some((_, text)) = repo.xtask_texts.iter().find(|(p, _)| p == f) else { return false };
    test_module_change(diff, f, text)
}

/// The 1-based line range strictly inside the file's test module: a column-0 `#[cfg(test)]` line, then a column-0
/// `mod <name> {` line, up to the first column-0 `}` line, after which only blank lines may follow (the module ends
/// the file, so a lookalike inside a string literal of the code above is never taken for it). `None`: no such module.
fn test_module_interior(text: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    for i in (0..lines.len()).filter(|&i| lines[i] == "#[cfg(test)]") {
        if !lines.get(i + 1).is_some_and(|m| m.starts_with("mod ") && m.ends_with(" {")) {
            continue;
        }
        let close = (i + 2..lines.len()).find(|&j| lines[j] == "}")?;
        if lines[close + 1..].iter().all(|l| l.trim().is_empty()) {
            // 0-based i is the attribute and i+1 the `mod` line: the body is 1-based i+3 ..= close.
            return Some((i + 3, close));
        }
    }
    None
}

/// Whether `diff` (`git diff -U0`) changes `file` (current text `text`) only inside its test module: the file has one
/// plain modification section (no new, deleted, renamed or binary file), every hunk's new lines (for a pure deletion,
/// the lines on both sides of it) lie strictly inside the module, and no added or removed line starts at column 0
/// (which could open, close or move the module or an item around it). Anything else is "no": FULL, as before.
fn test_module_change(diff: &str, file: &str, text: &str) -> bool {
    let Some((lo, hi)) = test_module_interior(text) else { return false };
    let inside = |n: usize| (lo..=hi).contains(&n);
    let header = format!("diff --git a/{file} b/{file}");
    let mut sections = diff.split("\ndiff --git ").enumerate().map(|(k, s)| if k == 0 { s.to_string() } else { format!("diff --git {s}") });
    let Some(sec) = sections.find(|s| s.lines().next() == Some(header.as_str())) else { return false };
    if sections.any(|s| s.lines().next() == Some(header.as_str())) {
        return false;
    }
    let mut hunks = 0;
    for l in sec.lines().skip(1) {
        if l.starts_with("new file") || l.starts_with("deleted file") || l.starts_with("rename ") || l.starts_with("copy ") || l.starts_with("Binary ") || l.starts_with("old mode") || l.starts_with("similarity") {
            return false;
        }
        // Header lines come before the first hunk only (after it, `--- x` is a removed line `-- x`).
        if (hunks == 0 && (l.starts_with("--- ") || l.starts_with("+++ ") || l.starts_with("index "))) || l.starts_with('\\') {
            continue;
        }
        if let Some(h) = l.strip_prefix("@@ ") {
            let Some(new) = h.split_whitespace().find_map(|w| w.strip_prefix('+')) else { return false };
            let (start, count) = new.split_once(',').map_or((new, "1"), |(s, c)| (s, c));
            let (Ok(start), Ok(count)) = (start.parse::<usize>(), count.parse::<usize>()) else { return false };
            let ok = if count == 0 { inside(start) && inside(start + 1) } else { inside(start) && inside(start + count - 1) };
            if !ok {
                return false;
            }
            hunks += 1;
        } else if let Some(body) = l.strip_prefix('+').or_else(|| l.strip_prefix('-')) {
            if body.chars().next().is_some_and(|c| !c.is_whitespace()) {
                return false;
            }
        } else {
            return false;
        }
    }
    hunks > 0
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
        let class = match classify(f) {
            Some(Class::Full(_)) if xtask_test_only(repo, f) => Some(Class::XtaskTestCode),
            c => c,
        };
        match class {
            Some(Class::XtaskTestCode) => {
                sel.xtask_tests = true;
                add(&mut sel.clippy, "xtask".to_string(), &format!("{f} changed"));
                sel.xtask_cmds.insert("static".to_string());
                sel.notes.push(format!("{f}: xtask test code only (xtask unit tests, clippy, `cargo xtask static`; not FULL)"));
            }
            Some(Class::Full(why)) => {
                if f.starts_with("xtask/") {
                    sel.xtask_tests = true;
                }
                sel.full.push(format!("{f}: {why}"));
            }
            Some(Class::Docs) => docs_readers(repo, f, &mut sel),
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
            Some(Class::MathWiring) => {
                sel.math_exports(repo, &format!("{f}: math export wiring"));
                sel.xtask_cmds.insert("math-gen --check".to_string());
                sel.notes.push(format!("{f}: math export wiring (the math export suites and `cargo xtask math-gen --check`, not FULL)"));
            }
            Some(Class::MathCoeffs) => {
                sel.math_coeffs(repo, f);
                sel.notes.push(format!("{f}: generated coefficients (liella-math's suites, the math export suites and `cargo xtask tables --check`, not FULL)"));
            }
            Some(Class::MathFit) if !repo.fits.contains(f) => sel.full.push(format!("{f}: a removed fit file (`cargo xtask remez --check` cannot check it)")),
            Some(Class::MathFit) => {
                // The fit's family: what its generated coefficient file selects, plus the fit's own check.
                sel.math_coeffs(repo, f);
                sel.xtask_cmds.insert(format!("remez --check {f}"));
                sel.notes.push(format!("{f}: a fit file (the family's math suites, `cargo xtask remez --check {f}` and `cargo xtask tables --check`, not FULL)"));
            }
            Some(Class::MathTables) => {
                // The generator is a liella-mathtools source like any other (its own mapping), and it writes every
                // coefficient family: the whole math set and both checks.
                ordinary_mapping(repo, f, &mut sel);
                sel.math_coeffs(repo, f);
                sel.xtask_cmds.insert("kernel-index --check".to_string());
                sel.notes.push(format!("{f}: the tables generator (its own mapping, every math suite, `cargo xtask tables --check` and `kernel-index --check`, not FULL)"));
            }
            Some(Class::MathToolFeed(feeds)) => {
                // A generator input: its own mapping (liella-mathtools' unit tests) and the checks that prove the
                // generated files still follow from it. The checks pass only when the output is unchanged, so the
                // math suites are not selected here (a changed output is a changed coefficient file, its own row).
                ordinary_mapping(repo, f, &mut sel);
                let mut what = Vec::new();
                if feeds.remez {
                    if repo.fits.is_empty() {
                        sel.full.push(format!("{f}: feeds `cargo xtask remez`, but the tree has no fit file to check"));
                    } else {
                        let fits: Vec<&str> = repo.fits.iter().map(String::as_str).collect();
                        sel.xtask_cmds.insert(format!("remez --check {}", fits.join(" ")));
                        what.push("`cargo xtask remez --check` (every fit)");
                    }
                }
                if feeds.tables {
                    sel.xtask_cmds.insert("tables --check".to_string());
                    sel.xtask_cmds.insert("kernel-index --check".to_string());
                    what.push("`cargo xtask tables --check` and `kernel-index --check`");
                }
                sel.notes.push(format!("{f}: a generator input (its own mapping, {}, not FULL)", what.join(", ")));
            }
            None => ordinary_mapping(repo, f, &mut sel),
        }
    }
    sel
}

/// The mapping of a source file that no rule above claims: its path class and the unit tests of the crate that owns
/// it (AFF-OWNER). One helper for the unclaimed files and the tables generator (XTASK-AFFECTED-MATH-3), so the
/// generator's selection cannot drift from what any other liella-mathtools source selects.
fn ordinary_mapping(repo: &Repo, f: &str, sel: &mut Selection) {
    classify_path(repo, f, sel);
    sel.own_suite(repo, f);
}

/// XTASK-AFFECTED-DOCS-PATHS (D-X.35): a documentation file (rule 2) selects no tests by itself, only the always-run
/// checks and `cargo xtask static` (lab names, decision and compat ids, the register, trace). A file that code names
/// as a path (`names_path`) selects what reads it: the test target, the owning crate's unit tests (a crate source),
/// or the xtask unit tests (an xtask source). Known limit: a path assembled at run time (`format!("docs/{x}")`) is
/// not seen; the readers found that way today are static checks, which run anyway.
fn docs_readers(repo: &Repo, f: &str, sel: &mut Selection) {
    sel.xtask_cmds.insert("static".to_string());
    let why = format!("{f}: the source names the file");
    let mut readers = Vec::new();
    for t in repo.targets.iter().filter(|t| names_path(&t.text, f)) {
        add(&mut sel.tests, (t.krate.clone(), t.name.clone()), &why);
        readers.push(format!("{}::{}", t.krate, t.name));
    }
    for n in repo.nodes.iter().filter(|n| names_path(&n.text, f)) {
        add(&mut sel.libs, n.krate.clone(), &why);
        readers.push(format!("lib:{}", n.krate));
    }
    if let Some((x, _)) = repo.xtask_texts.iter().find(|(_, t)| names_path(t, f)) {
        sel.xtask_tests = true;
        readers.push(format!("xtask ({x})"));
    }
    if readers.is_empty() {
        sel.notes.push(format!("{f}: docs only (stubs, policy checks, `cargo xtask static`; no tests)"));
    } else {
        sel.notes.push(format!("{f}: docs read by code (stubs, policy checks, `cargo xtask static`, and its readers: {})", readers.join(", ")));
    }
}

/// Whether `text` names `path` as a path in code: on a line that is not a comment, right after a quote or a `/`
/// (`"spec/behaviour/x.txt"`, `"/../../spec/behaviour/x.txt"`, `root.join("docs/x.tsv")`). A path cited in a comment
/// or a doc comment is a reference for the reader, not a read. A top-level file (`README.md`) has no directory to tell
/// a path from a bare file name (`"README.md"` in a list of names to skip), so it needs a `/` before it or `join("`.
fn names_path(text: &str, path: &str) -> bool {
    let at = |pre: &str| pre.ends_with('/') || (pre.ends_with('"') && (path.contains('/') || pre.ends_with("join(\"")));
    text.lines().filter(|l| !matches!(l.trim_start().get(..2), Some("//" | "/*" | "* "))).any(|l| l.match_indices(path).any(|(i, _)| at(&l[..i])))
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
    // A LINK-HELPERS helper source: the programs that link it are found by directive, not by name
    // (every conformance program is the safe selection).
    if f.starts_with("conformance/c/helpers/") && f.ends_with(".c") {
        sel.all_conf(repo, &format!("{f}: a dynamic-link helper source (every conformance program)"));
        return;
    }
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
    ("scan.S", "strstr", &["strstr_find", "s04_strings_symbols"]),
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
            // A static checker of the csys source names its symbols to find them; it runs in `cargo xtask static`
            // and `so`, which every gate runs, so it selects nothing more (D-X.34).
            if STATIC_CHECKERS.contains(&r.file.as_str()) {
                continue;
            }
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

/// D-X.41: `git diff -U0` of `xtask/` (the working tree against the merge base with `base`, as `changed_files`).
/// `None` without git history or base (a VM tree: the caller keeps the sync's `.changed.xtask`, if any). An untracked
/// file is not in it, so it is never shown to be test-only.
fn git_xtask_diff(root: &Path, base: &str) -> Option<String> {
    git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok()?;
    let base = if git(root, &["rev-parse", "--verify", "--quiet", base]).is_ok() { base } else if base == "origin/main" { "main" } else { return None };
    let mb = git(root, &["merge-base", "HEAD", base]).ok()?;
    git(root, &["diff", "-U0", "--no-color", "--no-ext-diff", mb.trim(), "--", "xtask/"]).ok()
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

/// The number of `SKIPPED <test>: <reason>` lines a log holds (TEST-SILENT-SKIPS): raw lines count once
/// each; a line the nextest report folded (`... (xN)`) counts N. The report's own heading and the
/// `SKIPPED lines:` summary do not start with `SKIPPED <word>:`.
fn count_skips(log: &str) -> u64 {
    log.lines()
        .filter_map(|l| {
            let l = l.trim_start().strip_prefix("SKIPPED ")?;
            if !l.contains(": ") || l.starts_with("lines:") {
                return None;
            }
            let n = l.strip_suffix(')').and_then(|r| r.rsplit_once(" (x")).and_then(|(_, n)| n.parse::<u64>().ok());
            Some(n.unwrap_or(1))
        })
        .sum()
}

/// A step's line, and for a failed test step the report of `failed_tests_report`, go to `out` (the console; a buffer in
/// the unit tests). `test_crate` is `Some(crate)` for a test step: it may be a skip (`no_tests_only`), and when it fails
/// its failing tests are named right under its FAIL line, before the next step starts (XTASK-AFFECTED-FAIL-EARLY).
fn step_out(root: &Path, log: &Path, label: &str, cmd: &mut Command, test_crate: Option<&str>, out: &mut dyn std::io::Write) -> Outcome {
    use std::io::Write;
    let allow_skip = test_crate.is_some();
    let started = Instant::now();
    // This step's own output starts where the log ends now (the classification never reads earlier steps).
    let from = fs::metadata(log).map_or(0, |m| m.len());
    let file = fs::OpenOptions::new().create(true).append(true).open(log);
    let (success, failed_to_start) = match file.and_then(|f| f.try_clone().map(|g| (f, g))) {
        Ok((out, err)) => {
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
    let _ = writeln!(out, "{:>6.1}s  {shown}  {label}{}", started.elapsed().as_secs_f64(), if outcome == Outcome::Skipped { "  (no test matched the selection: skipped, not failed)" } else { "" });
    if let (Outcome::Failed, Some(c)) = (outcome, test_crate) {
        let bytes = fs::read(log).unwrap_or_default();
        let first_line = bytes[..(from as usize).min(bytes.len())].iter().filter(|&&b| b == b'\n').count() + 1;
        let _ = write!(out, "{}", failed_tests_report(&own, c, log, first_line));
    }
    let _ = out.flush();
    outcome
}

/// How many failing tests a failed test step names under its FAIL line, and how many lines of each one's panic it
/// shows (XTASK-AFFECTED-FAIL-EARLY); a longer line is cut to `EARLY_WIDTH` characters.
const EARLY_TESTS: usize = 10;
const EARLY_PANIC_LINES: usize = 5;
const EARLY_WIDTH: usize = 300;

/// The failing tests of a test step's own output, in order, each `<crate>[::<binary>] <test>` with the first lines of
/// its panic (empty when the output holds none). Two runners write it:
/// - nextest (through `xtask nextest`): a `<STATUS> [ 0.1s] (n/m) <binary id> <test>` line per test, a failing test's
///   captured output under header lines naming it, and at the end one `---- <binary id> <test> stdout ----` line per
///   failure (`nextest.rs`);
/// - `cargo test`: `Running <file> (<deps path>)` per binary, then `---- <test> stdout ----` followed by that test's
///   output for each failure; the name gets the binary of the `Running` line before it.
fn failed_tests(own: &str, krate: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<&str> = own.lines().collect();
    let mut binary = krate.to_string();
    // (full name, the name as its `---- ` line has it)
    let mut names: Vec<(String, String)> = Vec::new();
    for l in &lines {
        let t = l.trim();
        if let Some(r) = t.strip_prefix("Running ") {
            // "Running unittests src/lib.rs (target/debug/deps/liella_mem-0123)", "Running tests/x.rs (.../x-0123)"
            let deps = r.rsplit_once('(').map_or("", |(_, p)| p.trim_end_matches(')'));
            let stem = deps.rsplit('/').next().unwrap_or("");
            let stem = stem.rsplit_once('-').map_or(stem, |(s, _)| s);
            binary = if r.starts_with("unittests src/lib.rs") || stem.is_empty() { krate.to_string() } else if r.starts_with("unittests ") { format!("{krate}::bin/{stem}") } else { format!("{krate}::{stem}") };
        } else if let Some(n) = t.strip_prefix("---- ").and_then(|x| x.strip_suffix(" stdout ----")) {
            let full = if n.contains(' ') { n.to_string() } else { format!("{binary} {n}") };
            if !names.iter().any(|(f, _)| *f == full) {
                names.push((full, n.to_string()));
            }
        }
    }
    let mut panics: Vec<Vec<String>> = vec![Vec::new(); names.len()];
    // The failure whose output the scan is in: set by a line naming it (a nextest status or output header, a
    // `---- ` line), cleared by a line naming another test or ending a `cargo test` section.
    let mut current: Option<usize> = None;
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        let section = t.strip_prefix("---- ").and_then(|x| x.strip_suffix(" stdout ----"));
        let named = names.iter().position(|(full, short)| section == Some(short.as_str()) || t.ends_with(full.as_str()) || t.contains(&format!("{full} ")));
        let status = t.split_once(" [").is_some_and(|(s, _)| !s.is_empty() && s.bytes().all(|b| b.is_ascii_uppercase() || b == b'-'));
        if named.is_some() {
            current = named;
        } else if status || t.starts_with("---- ") || lines[i] == "failures:" || lines[i].starts_with("test result: ") || t.starts_with("Running ") {
            // nextest indents a test's captured output (its own `failures:` and `test result:` lines too), under
            // `stdout ───` / `stderr ───` headers that do not name the test: only column-0 lines end a section.
            current = None;
        } else if let Some(k) = current.filter(|&k| panics[k].is_empty() && t.contains("panicked at")) {
            let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
            let mut j = i;
            while j < lines.len() && panics[k].len() < EARLY_PANIC_LINES {
                let p = lines[j].trim_end();
                let p = p.strip_prefix(indent).unwrap_or(p);
                let q = p.trim_start();
                if j > i && (q.is_empty() || q.starts_with("note: run with") || q.starts_with("stack backtrace:") || q.starts_with("---- ")) {
                    break;
                }
                let cut: String = p.chars().take(EARLY_WIDTH).collect();
                panics[k].push(if cut.len() < p.len() { format!("{cut} [line cut]") } else { cut });
                j += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    names.into_iter().map(|(f, _)| f).zip(panics).collect()
}

/// What a failed test step prints under its FAIL line: its failing tests (at most `EARLY_TESTS`, with the count of the
/// rest), the first `EARLY_PANIC_LINES` lines of each one's panic, and the log holding the step's whole output with
/// the line the step starts at. The end-of-run summary still lists every failure.
fn failed_tests_report(own: &str, krate: &str, log: &Path, first_line: usize) -> String {
    let fails = failed_tests(own, krate);
    let mut out = String::new();
    if fails.is_empty() {
        out.push_str("        no failing test named in the step's output (a build error, or a runner that did not finish)\n");
        for e in own.lines().filter(|l| l.starts_with("error")).take(EARLY_PANIC_LINES) {
            out.push_str(&format!("          {}\n", e.chars().take(EARLY_WIDTH).collect::<String>()));
        }
    } else {
        out.push_str(&format!("        failing tests ({}):\n", fails.len()));
        for (name, panic) in fails.iter().take(EARLY_TESTS) {
            out.push_str(&format!("          {name}\n"));
            if panic.is_empty() {
                out.push_str("            (no panic message in the output; see the log)\n");
            }
            for p in panic {
                out.push_str(&format!("            | {p}\n"));
            }
        }
        if fails.len() > EARLY_TESTS {
            out.push_str(&format!("          ... and {} more failing tests (all are in the summary at the end)\n", fails.len() - EARLY_TESTS));
        }
    }
    out.push_str(&format!("        full output: {} (this step from line {first_line})\n", log.display()));
    out
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
    step_out(root, log, label, cmd, None, &mut std::io::stdout()) == Outcome::Ok
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

/// How many trailing lines of a failed conformance program's stdout, and of its stderr, `affected` prints (D-X.27).
const CONF_TAIL_LINES: usize = 40;
/// A longer line of that output is cut to this many characters (one runaway line does not flood the console).
const CONF_TAIL_WIDTH: usize = 300;

/// D-X.27: the selected conformance programs as one step (`conformance/run.sh <name>...`): whether it passed, and when
/// it failed the report of `conformance_failures` (empty when it passed), which the caller prints to the console, where `cargo xtask plants` matches a plant's `msg:` (before, the console
/// named the step only, and a plant caught for the right reason by a program's own check was WARN-NO-MSG).
fn run_conformance(root: &Path, log: &Path, names: &[&str]) -> (bool, String) {
    let from = fs::metadata(log).map_or(0, |m| m.len());
    let mut cmd = Command::new("sh");
    cmd.arg("conformance/run.sh").args(names);
    let ok = step(root, log, &format!("conformance ({} programs)", names.len()), &mut cmd);
    if ok {
        return (true, String::new());
    }
    let bytes = fs::read(log).unwrap_or_default();
    let own = String::from_utf8_lossy(&bytes[(from as usize).min(bytes.len())..]).into_owned();
    (false, conformance_failures(&root.join("conformance").join("build"), &own))
}

/// Linux x86-64 signal numbers (kernel UAPI `asm/signal.h`), for a status of 128 + n.
fn signal_name(n: u32) -> Option<&'static str> {
    const NAMES: [&str; 31] = [
        "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGBUS", "SIGFPE", "SIGKILL", "SIGUSR1", "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM", "SIGSTKFLT", "SIGCHLD", "SIGCONT", "SIGSTOP", "SIGTSTP", "SIGTTIN", "SIGTTOU", "SIGURG", "SIGXCPU", "SIGXFSZ", "SIGVTALRM", "SIGPROF", "SIGWINCH", "SIGIO", "SIGPWR", "SIGSYS",
    ];
    NAMES.get((n as usize).wrapping_sub(1)).copied()
}

/// `status N, expected ...` of `run.sh`, with a status above 128 read as the shell does: death by signal N - 128.
fn describe_status(detail: &str) -> String {
    let Some(rest) = detail.strip_prefix("status ") else { return detail.to_string() };
    let (n, tail) = rest.split_at(rest.find(',').unwrap_or(rest.len()));
    match n.trim().parse::<u32>() {
        Ok(rc) if rc > 128 && rc <= 128 + 64 => {
            let sig = rc - 128;
            format!("killed by signal {sig}{} (status {rc}){tail}", signal_name(sig).map(|s| format!(" {s}")).unwrap_or_default())
        }
        Ok(rc) => format!("exit status {rc}{tail}"),
        Err(_) => detail.to_string(),
    }
}

/// The last `CONF_TAIL_LINES` lines of `path` under a heading, each line prefixed `    | `, with a note when the file
/// had more lines (or a line was cut); `(empty)` or `(absent)` otherwise.
fn conf_tail(what: &str, path: &Path) -> String {
    let Ok(bytes) = fs::read(path) else { return format!("  {what}: (absent: {})\n", path.display()) };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return format!("  {what}: (empty)\n");
    }
    let skip = lines.len().saturating_sub(CONF_TAIL_LINES);
    let mut out = if skip > 0 {
        format!("  {what}: last {CONF_TAIL_LINES} of {} lines ({skip} cut; whole file {}):\n", lines.len(), path.display())
    } else {
        format!("  {what} ({} lines):\n", lines.len())
    };
    for l in &lines[skip..] {
        let n = l.chars().count();
        if n > CONF_TAIL_WIDTH {
            out.push_str(&format!("    | {} ... ({} more characters cut)\n", l.chars().take(CONF_TAIL_WIDTH).collect::<String>(), n - CONF_TAIL_WIDTH));
        } else {
            out.push_str(&format!("    | {l}\n"));
        }
    }
    out
}

/// D-X.27: for each program the step's own output (`own`, what `run.sh` printed) names as failed - a line
/// `conformance: FAIL <name> (<why>)` - its name, how it ended (the `conformance: <name>: ...` lines: exit status or
/// signal, missing stderr text) and the tails of its stdout and stderr in `build`. A program that did not build (or
/// whose helper did not) shows its build log instead: its stdout and stderr files there are an earlier run's. Programs
/// that passed print nothing; a failed step that names no program shows the end of its own output.
fn conformance_failures(build: &Path, own: &str) -> String {
    let mut failed: Vec<(&str, &str)> = Vec::new();
    let mut details: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for l in own.lines() {
        let Some(r) = l.strip_prefix("conformance: ") else { continue };
        if let Some(f) = r.strip_prefix("FAIL ") {
            let (name, why) = f.split_once(' ').unwrap_or((f, ""));
            failed.push((name, why.trim().trim_start_matches('(').trim_end_matches(')')));
        } else if let Some((name, d)) = r.split_once(": ") {
            if !name.is_empty() && !name.contains(' ') {
                details.entry(name).or_default().push(d);
            }
        }
    }
    let mut out = String::new();
    if failed.is_empty() {
        let lines: Vec<&str> = own.lines().collect();
        out.push_str("conformance: the step failed but named no failed program; the end of its output:\n");
        for l in &lines[lines.len().saturating_sub(CONF_TAIL_LINES)..] {
            out.push_str(&format!("    | {l}\n"));
        }
        return out;
    }
    for (name, why) in failed {
        let how: Vec<String> = details.get(name).into_iter().flatten().map(|d| describe_status(d)).collect();
        out.push_str(&format!("conformance FAILED: {name}: {}\n", if how.is_empty() { why.to_string() } else { how.join("; ") }));
        if why.starts_with("stderr:") {
            out.push_str(&conf_tail("stdout", &build.join(format!("{name}.log.stdout"))));
            out.push_str(&conf_tail("stderr", &build.join(format!("{name}.log.stderr"))));
        } else if build.join(format!("{name}.log")).exists() && (why.contains("does not build") || why.contains("helper")) {
            out.push_str(&conf_tail("build log", &build.join(format!("{name}.log"))));
        }
    }
    out
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
        // Step times (XTASK-TEST-STEP-TIMES): a `step:` line per step on the console, those `cargo xtask test` wrote
        // to the log among them, and the summary of all of them, slowest first.
        let mut times = crate::steptime::Steps::default();
        ok &= times.time("stubs", || step(root, &log, "stubs", Command::new(&cargo).args(["xtask", "stubs"])));
        ok &= times.time("clippy-workspace", || step_saving(root, &log, "clippy --workspace", Command::new(&cargo).args(["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]), "clippy.log"));
        let from = fs::metadata(&log).map_or(0, |m| m.len());
        ok &= step(root, &log, "cargo xtask test", Command::new(&cargo).args(["xtask", "test"]));
        let bytes = fs::read(&log).unwrap_or_default();
        for (name, secs) in String::from_utf8_lossy(&bytes[(from as usize).min(bytes.len())..]).lines().filter_map(crate::steptime::parse) {
            // The remez binary's per-fit lines are parts of the `remez-check` step: shown, not summed again.
            if name.contains('/') {
                println!("{}", crate::steptime::line(name, Duration::from_secs_f64(secs)));
            } else {
                times.add(&format!("test/{name}"), Duration::from_secs_f64(secs));
            }
        }
        times.print_summary();
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
            ok &= step_out(root, &log, "xtask unit tests", Command::new(&cargo).args(["test", "-p", "xtask"]), Some("xtask"), &mut std::io::stdout()) == Outcome::Ok;
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
                    cmd.arg(unit_tests_flag(repo, c));
                }
                for t in &ts {
                    cmd.args(["--test", t]);
                }
                // The heavy products sweeps run shard 0 only with the exports interposed
                // (`nextest::PASS1_SHARD0`, D-X.36); `cargo test` has no filter for it and runs all.
                if pass == 1 && nextest {
                    cmd.args(crate::nextest::pass1_shard_filter(c, &ts).unwrap_or_default());
                }
                let label = format!("test -p {c}{}", if pass == 1 { " (exports interposed)" } else { "" });
                match step_out(root, &log, &label, &mut cmd, Some(c), &mut std::io::stdout()) {
                    Outcome::Ok => {}
                    Outcome::Skipped => skipped.push(label),
                    Outcome::Failed => ok = false,
                }
            }
        }
        if !sel.conf.is_empty() {
            let (passed, report) = run_conformance(root, &log, &sel.conf.keys().map(String::as_str).collect::<Vec<_>>());
            print!("{report}");
            ok &= passed;
        }
        for c in &sel.xtask_cmds {
            // A command may carry its flags (`math-gen --check`).
            ok &= step(root, &log, &format!("cargo xtask {c}"), Command::new(&cargo).arg("xtask").args(c.split_whitespace()));
        }
    }
    println!("--- summary ({}), log {}", format_args!("{:.0}s", started.elapsed().as_secs_f64()), log.display());
    let text = read(&log);
    print!("{}", summary(&text));
    println!("SKIPPED lines: {} (tests that skipped part of their checks because a tool or capability was absent; they tested less)", count_skips(&text));
    if !skipped.is_empty() {
        println!("skipped (no test matched the selection; counted apart, not a failure): {} - {}", skipped.len(), skipped.join("; "));
    }
    println!("affected: {}{}", if ok { "ok" } else { "FAILED" }, if sel.only { " (ONLY mode: named binaries, NOT A GATE)" } else { "" });
    if !ok {
        // D-X.23: test builds carry no DWARF; panics still name file:line (core::panic::Location).
        println!("hint: for a debugger or source-level backtrace, rerun the red test with debuginfo: CARGO_PROFILE_DEV_DEBUG=full cargo xtask affected --only <target> --run");
    }
    let _ = repo;
    if ok { Ok(()) } else { Err("a selected test or check failed".into()) }
}

/// The cargo target flag that runs a package's unit tests: `--lib`, or `--bins` for a bin-only package (xtask, which
/// a change to a crate it depends on reaches through `wide`), where `--lib` stops the build at once ("no library
/// targets found in package") and the gate went red without running a test (XTASK-AFFECTED-XTASK-TESTS, D-X.41).
/// An unknown package keeps `--lib`.
fn unit_tests_flag(repo: &Repo, krate: &str) -> &'static str {
    if repo.crates.iter().any(|c| c.name == krate && !c.lib) { "--bins" } else { "--lib" }
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

/// XTASK-AFFECTED-EXPLICIT: the one line printed (exit 2) when `affected` is given neither `--list` nor `--run`. There is
/// no default mode: a gate that ran `cargo xtask affected` without `--run` used to list the selection, run nothing and
/// exit 0, a green gate that tested nothing.
pub const USAGE: &str = "usage: cargo xtask affected (--list | --run) [--base <ref>] [--files <path>...] [--only <names>] [--tier direct|wide|full] [--with-exports]  or  affected --replay <records> [--tier direct|wide|full]; --list prints the selection, --run runs it, one of them is required";

/// `--list` prints the selection; `--run` runs it. Exactly one is required (except with `--replay`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    List,
    Run,
}

/// The parsed arguments of `cargo xtask affected`.
#[derive(Debug)]
struct Opts {
    base: String,
    mode: Option<Mode>,
    with_exports: bool,
    files: Option<Vec<String>>,
    only: Option<String>,
    tier: Tier,
    replay_file: Option<String>,
}

impl Opts {
    /// The mode to act in, or `None` when the command must be refused with [`USAGE`] (exit 2). `--replay` is a mode of
    /// its own (it reads gate records, lists and runs nothing) and needs neither flag.
    fn mode(&self) -> Option<Mode> {
        if self.replay_file.is_some() { Some(self.mode.unwrap_or(Mode::List)) } else { self.mode }
    }
}

fn parse_args(args: &[String], default_base: String) -> Result<Opts, String> {
    let mut o = Opts { base: default_base, mode: None, with_exports: false, files: None, only: None, tier: Tier::Direct, replay_file: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => o.base = it.next().ok_or("--base needs a ref")?.clone(),
            "--tier" => o.tier = Tier::parse(it.next().ok_or("--tier needs direct, wide or full")?)?,
            "--replay" => o.replay_file = Some(it.next().ok_or("--replay needs a records file")?.clone()),
            "--run" | "--list" => {
                let m = if a == "--run" { Mode::Run } else { Mode::List };
                if o.mode.is_some_and(|prev| prev != m) {
                    return Err(format!("--list and --run together: give one ({USAGE})"));
                }
                o.mode = Some(m);
            }
            "--with-exports" => o.with_exports = true,
            "--only" => o.only = Some(it.next().ok_or("--only needs a list: <target>[,crate::target][,conformance:<program>][,lib:<crate>][,xtask]")?.clone()),
            "--files" => {
                let mut v = Vec::new();
                while let Some(f) = it.clone().next().filter(|f| !f.starts_with("--")) {
                    v.push(f.clone());
                    it.next();
                }
                o.files = Some(v);
            }
            other => return Err(format!("unknown argument `{other}` ({USAGE})")),
        }
    }
    Ok(o)
}

/// `cargo xtask affected`: exit 2 with [`USAGE`] when neither `--list` nor `--run` is given, 1 on any other error.
pub fn main(root: &Path, args: &[String]) -> ExitCode {
    // AFFECTED_BASE: the ref a stacked branch (one cut from an unmerged parent) selects against; `--base` wins.
    let result = parse_args(args, default_base(std::env::var("AFFECTED_BASE").ok())).and_then(|o| match o.mode() {
        None => Ok(None),
        Some(mode) => execute(root, &o, mode, &mut run_selection).map(Some),
    });
    match result {
        Ok(Some(())) => ExitCode::SUCCESS,
        Ok(None) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("xtask affected: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs a selection: `run_selection`, or a recorder in the unit tests.
type Runner<'a> = dyn FnMut(&Path, &Repo, &Selection, bool) -> Result<(), String> + 'a;

/// The work of one `affected` command; `runner` runs a selection when the mode is `--run`.
fn execute(root: &Path, o: &Opts, mode: Mode, runner: &mut Runner<'_>) -> Result<(), String> {
    let tier = o.tier;
    if let Some(path) = &o.replay_file {
        let repo = Repo::load(root);
        let text = fs::read_to_string(path).map_err(|e| format!("--replay {path}: {e}"))?;
        let (report, misses) = replay(&repo, &text, tier);
        print!("{report}");
        return if misses == 0 { Ok(()) } else { Err(format!("replay: {misses} failures outside the {tier:?} tier")) };
    }
    if let Some(list) = &o.only {
        let repo = Repo::load(root);
        let sel = select_only(&repo, list)?;
        print_selection(&repo, &[], &sel);
        return if mode == Mode::Run { runner(root, &repo, &sel, o.with_exports) } else { Ok(()) };
    }
    let files = match &o.files {
        Some(f) => f.clone(),
        None => changed_files(root, &o.base)?,
    };
    let mut repo = Repo::load(root);
    // D-X.41: the xtask diff describes the base comparison only; a `--files` list is not one (no diff: FULL as before).
    repo.xtask_diff = if o.files.is_some() { None } else { git_xtask_diff(root, &o.base).or(repo.xtask_diff) };
    let sel = select_tier(&repo, &files, tier);
    println!("tier: {tier:?} (per-fix gates: direct; the landing gate runs everything)");
    print_selection(&repo, &files, &sel);
    if mode == Mode::Run { runner(root, &repo, &sel, o.with_exports) } else { Ok(()) }
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

    fn opts(args: &[&str]) -> Result<Opts, String> {
        parse_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), "origin/main".into())
    }

    // XTASK-AFFECTED-EXPLICIT: no default mode. Without --list or --run there is no mode (main exits 2 with USAGE; the
    // binary's exit status is checked in xtask/tests/affected_mode.rs); --replay needs neither; both together are refused.
    #[test]
    fn affected_requires_list_or_run() {
        // Trace: no spec clause (test-harness tooling; XTASK-AFFECTED-EXPLICIT).
        assert_eq!(opts(&[]).unwrap().mode(), None);
        assert_eq!(opts(&["--base", "origin/next", "--tier", "direct", "--files", "docs/plants.md"]).unwrap().mode(), None);
        assert_eq!(opts(&["--list"]).unwrap().mode(), Some(Mode::List));
        assert_eq!(opts(&["--files", "a.rs", "--run", "--base", "x"]).unwrap().mode(), Some(Mode::Run));
        assert_eq!(opts(&["--replay", "records.txt"]).unwrap().mode(), Some(Mode::List));
        let both = opts(&["--list", "--run"]).unwrap_err();
        assert!(both.contains("--list and --run together"), "{both}");
        assert!(USAGE.contains("--list") && USAGE.contains("--run") && !USAGE.contains('\n'), "{USAGE}");
    }

    // XTASK-AFFECTED-EXPLICIT: --run hands the selection to the runner, --list does not.
    #[test]
    fn affected_run_runs_the_selection_and_list_does_not() {
        // Trace: no spec clause (test-harness tooling; XTASK-AFFECTED-EXPLICIT).
        for (flag, want_runs) in [("--run", 1), ("--list", 0)] {
            let o = opts(&["--files", "docs/plants.md", flag]).unwrap();
            let mut runs = 0;
            let mut seen_docs_note = false;
            let mut rec = |_: &Path, _: &Repo, sel: &Selection, exports: bool| -> Result<(), String> {
                runs += 1;
                seen_docs_note = sel.notes.iter().any(|n| n.starts_with("docs/plants.md: docs only")) && sel.full.is_empty() && !exports;
                Ok(())
            };
            execute(&root(), &o, o.mode().unwrap(), &mut rec).unwrap();
            assert_eq!(runs, want_runs, "{flag}");
            assert_eq!(seen_docs_note, want_runs == 1, "{flag}: the runner got the docs-only selection");
        }
        let mut fail = |_: &Path, _: &Repo, _: &Selection, _: bool| -> Result<(), String> { Err("a selected test failed".into()) };
        let o = opts(&["--files", "docs/plants.md", "--run"]).unwrap();
        assert_eq!(execute(&root(), &o, Mode::Run, &mut fail).unwrap_err(), "a selected test failed");
    }

    // TOOLS-7 (1): a failed step's FULL output lands in target/<name> (only that step's, not earlier steps'), and a
    // stale file from an earlier run is replaced; a passing step leaves its (short) output there too.
    #[test]
    fn skipped_lines_are_counted_with_their_repeats() {
        // Trace: no spec clause (test-harness reporting; TEST-SILENT-SKIPS).
        let log = "SKIPPED a: no cc\n  SKIPPED b: no msgfmt (x2)\nSKIPPED lines: 9 (x)\nskipped: old\n";
        assert_eq!(count_skips(log), 3);
    }

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

    /// XTASK-AFFECTED-FAIL-EARLY: an env-gated fixture. It passes unless `LIELLA_AFFECTED_FAIL_FIXTURE` is set; with it
    /// set, `LIELLA_AFFECTED_FAIL_FIXTURE=1 cargo xtask affected --only xtask --run` shows a real failed test step's
    /// report (the end-to-end plant in handoff/XTASK-AFFECTED-FAIL-EARLY-plants.txt).
    #[test]
    fn affected_fail_early_fixture() {
        // Trace: no spec clause (test-harness reporting fixture; XTASK-AFFECTED-FAIL-EARLY).
        if std::env::var_os("LIELLA_AFFECTED_FAIL_FIXTURE").is_some() {
            panic!("FIXTURE-PANIC-MARKER: this test fails on purpose because LIELLA_AFFECTED_FAIL_FIXTURE is set");
        }
    }

    /// The output of `cargo test -p liella-x` with one failure in the unit tests and one in `tests/sweep.rs`.
    const CARGO_TEST_FAILS: &str = "   Compiling liella-x v0.1.0\n    Finished `test` profile [unoptimized] target(s) in 1.00s\n     Running unittests src/lib.rs (target/debug/deps/liella_x-0123abcd)\n\nrunning 2 tests\ntest a::passes ... ok\ntest a::breaks ... FAILED\n\nfailures:\n\n---- a::breaks stdout ----\n\nthread 'a::breaks' panicked at src/a.rs:9:5:\nassertion `left == right` failed\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\nfailures:\n    a::breaks\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n     Running tests/sweep.rs (target/debug/deps/sweep-89abcdef)\n\nrunning 1 test\ntest edge ... FAILED\n\nfailures:\n\n---- edge stdout ----\nthread 'edge' panicked at tests/sweep.rs:3:5:\nL1\nL2\nL3\nL4\nL5\n\nfailures:\n    edge\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

    // A failed test step names its failing tests and their panics right under its FAIL line, before the next step's
    // line; a passing test step prints its line only; an earlier step's output is not read.
    #[test]
    fn a_failed_test_step_names_its_failures_under_its_fail_line() {
        // Trace: no spec clause (test-harness reporting; XTASK-AFFECTED-FAIL-EARLY).
        let root = std::env::temp_dir().join(format!("xtask-failearly-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target")).unwrap();
        let log = root.join("target").join("affected.log");
        fs::write(root.join("fails.txt"), CARGO_TEST_FAILS).unwrap();
        let mut out: Vec<u8> = Vec::new();
        let sh = |script: &str| {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        };
        assert_eq!(step_out(&root, &log, "test -p liella-w", &mut sh("echo '---- old stdout ----'; echo \"thread 'old' panicked at x\"; exit 0"), Some("liella-w"), &mut out), Outcome::Ok);
        let passing = String::from_utf8(out.clone()).unwrap();
        assert_eq!(passing.lines().count(), 1, "a passing step printed more than its line: {passing:?}");
        assert_eq!(step_out(&root, &log, "test -p liella-x", &mut sh("cat fails.txt; exit 101"), Some("liella-x"), &mut out), Outcome::Failed);
        assert_eq!(step_out(&root, &log, "test -p liella-z", &mut sh("exit 0"), Some("liella-z"), &mut out), Outcome::Ok);
        let got = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = got.lines().map(|l| l.split_once("s  ").map_or(l, |(_, r)| r)).collect();
        let want = [
            "ok    test -p liella-w".to_string(),
            "FAIL  test -p liella-x".to_string(),
            "        failing tests (2):".to_string(),
            "          liella-x a::breaks".to_string(),
            "            | thread 'a::breaks' panicked at src/a.rs:9:5:".to_string(),
            "            | assertion `left == right` failed".to_string(),
            "            |   left: 1".to_string(),
            "            |  right: 2".to_string(),
            "          liella-x::sweep edge".to_string(),
            "            | thread 'edge' panicked at tests/sweep.rs:3:5:".to_string(),
            "            | L1".to_string(),
            "            | L2".to_string(),
            "            | L3".to_string(),
            "            | L4".to_string(),
            format!("        full output: {} (this step from line 4)", log.display()),
            "ok    test -p liella-z".to_string(),
        ];
        assert_eq!(lines, want, "the console was {got:?}");
        let _ = fs::remove_dir_all(&root);
    }

    // nextest's format: status and output header lines name the binary id and test, `xtask nextest` adds a `---- `
    // line per failure. At most 10 tests are shown (with the count of the rest), long lines are cut, a failure
    // without a panic says so, a panic under another test's line is not attributed, and a step naming no failing
    // test shows its first error lines.
    #[test]
    fn the_failed_test_report_reads_nextest_output_and_stays_bounded() {
        // Trace: no spec clause (test-harness reporting; XTASK-AFFECTED-FAIL-EARLY).
        let mut own = String::from("== test -p liella-y\n    Starting 14 tests across 1 binary\n        SIGSEGV [   0.010s] (  1/14) liella-y::t crash\n");
        own.push_str("        PASS [   0.001s] (  2/14) liella-y::t fine\nthread 'fine' panicked at tests/t.rs:1:1:\nnot this one\n");
        // nextest 0.9.1xx (as on the build hosts, 11-06): indented output under `stdout ───` / `stderr ───`; case_1
        // has the older `──── STDERR: <binary id> <test>` header, which names the test.
        for i in 0..12 {
            let (out_h, err_h, ind) = if i == 1 { (format!("──── STDOUT:             liella-y::t case_{i}"), format!("──── STDERR:             liella-y::t case_{i}"), "") } else { ("  stdout ───".to_string(), "  stderr ───".to_string(), "    ") };
            own.push_str(&format!("        FAIL [   0.010s] ({:>3}/14) liella-y::t case_{i}\n{out_h}\n\n{ind}running 1 test\n{ind}test case_{i} ... FAILED\n\n{ind}failures:\n\n{ind}failures:\n{ind}    case_{i}\n\n{ind}test result: FAILED. 0 passed; 1 failed\n\n", i + 3));
            own.push_str(&format!("{err_h}\n\n{ind}thread 'case_{i}' (12{i}) panicked at tests/t.rs:{i}:1:\n{ind}boom {i}{}\n{ind}note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n", if i == 0 { "x".repeat(400) } else { String::new() }));
        }
        own.push_str("     Summary [   0.200s] 14 tests run: 1 passed, 13 failed\n---- liella-y::t crash stdout ----\n");
        for i in 0..12 {
            own.push_str(&format!("---- liella-y::t case_{i} stdout ----\n"));
        }
        let r = failed_tests_report(&own, "liella-y", Path::new("/x/affected.log"), 7);
        for want in [
            "        failing tests (13):\n          liella-y::t crash\n            (no panic message in the output; see the log)\n          liella-y::t case_0\n            | thread 'case_0' (120) panicked at tests/t.rs:0:1:\n            | boom 0xxx",
            "xxx [line cut]\n          liella-y::t case_1\n            | thread 'case_1' (121) panicked at tests/t.rs:1:1:\n            | boom 1\n          liella-y::t case_2\n",
            "          liella-y::t case_8\n            | thread 'case_8' (128) panicked at tests/t.rs:8:1:\n            | boom 8\n          ... and 3 more failing tests",
            "        full output: /x/affected.log (this step from line 7)\n",
        ] {
            assert!(r.contains(want), "missing {want:?} in {r}");
        }
        assert!(!r.contains("case_9\n") && !r.contains("'fine'") && !r.contains("not this one"), "{r}");
        assert!(r.lines().all(|l| l.chars().count() <= EARLY_WIDTH + 30), "a line was not cut: {r}");
        let build = failed_tests_report("== test -p liella-y\nerror[E0425]: cannot find value `x` in this scope\n  --> tests/t.rs:1:1\n", "liella-y", Path::new("/x/affected.log"), 1);
        assert_eq!(build, "        no failing test named in the step's output (a build error, or a runner that did not finish)\n          error[E0425]: cannot find value `x` in this scope\n        full output: /x/affected.log (this step from line 1)\n");
    }

    /// A temporary root whose `conformance/run.sh` stands in for the real runner, printing its message formats:
    /// `good` passes; `bad` exits with status 134 (SIGABRT) and a missing stderr text, 5 stdout and 50 stderr lines
    /// (the last one far wider than the cut); `nobuild` does not build and has an earlier run's stdout and stderr.
    fn fake_conformance_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-conf-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("conformance").join("build")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        let script = r#"b=conformance/build; fail=0; pass=0
for n in "$@"; do
  case $n in
  good) pass=$((pass + 1)) ;;
  bad) i=1; while [ $i -le 5 ]; do echo "out-$i"; i=$((i + 1)); done > $b/bad.log.stdout
       i=1; while [ $i -le 49 ]; do echo "err-$i"; i=$((i + 1)); done > $b/bad.log.stderr
       printf 'FAIL: strlen returned 3 %0500d\n' 7 >> $b/bad.log.stderr
       echo "conformance: bad: status 134, expected 0"; echo "conformance: bad: stderr lacks: wanted text"
       echo "conformance: FAIL bad (stderr: conformance/build/bad.log.stderr)"; fail=$((fail + 1)) ;;
  nobuild) echo STALE-STDOUT > $b/nobuild.log.stdout; echo STALE-STDERR > $b/nobuild.log.stderr
       echo "x.c:1: error: boom" > $b/nobuild.log
       echo "conformance: FAIL nobuild (does not build; see conformance/build/nobuild.log)"; fail=$((fail + 1)) ;;
  esac
done
echo "conformance: $pass passed, $fail failed"; [ $fail -eq 0 ]
"#;
        fs::write(root.join("conformance").join("run.sh"), script).unwrap();
        root
    }

    // Trace: no spec clause (test-harness reporting, D-X.27): a failed conformance program's name, its end (signal by
    // name, the missing stderr text) and the bounded tails of its stdout and stderr reach the report `affected` prints;
    // a program that did not build shows its build log, not an earlier run's output; a passing program adds nothing.
    #[test]
    fn a_failed_conformance_program_reports_its_end_and_output_tails() {
        let root = fake_conformance_root("fail");
        let log = root.join("target").join("affected.log");
        assert!(step(&root, &log, "earlier step", Command::new("sh").args(["-c", "echo 'conformance: FAIL earlier (stderr: x)'"])));
        let (ok, report) = run_conformance(&root, &log, &["good", "bad", "nobuild"]);
        assert!(!ok, "{report}");
        for want in [
            "conformance FAILED: bad: killed by signal 6 SIGABRT (status 134), expected 0; stderr lacks: wanted text\n",
            "  stdout (5 lines):\n    | out-1\n",
            "    | out-5\n",
            "  stderr: last 40 of 50 lines (10 cut; whole file ",
            "    | err-11\n",
            "    | FAIL: strlen returned 3 000",
            " ... (224 more characters cut)\n",
            "conformance FAILED: nobuild: does not build; see conformance/build/nobuild.log\n  build log (1 lines):\n    | x.c:1: error: boom\n",
        ] {
            assert!(report.contains(want), "missing {want:?} in:\n{report}");
        }
        for not in ["| err-10\n", "STALE-", "good", "earlier"] {
            assert!(!report.contains(not), "unexpected {not:?} in:\n{report}");
        }
        let (ok, report) = run_conformance(&root, &log, &["good"]);
        assert!(ok && report.is_empty(), "a passing program printed: {report:?}");
        let _ = fs::remove_dir_all(&root);
    }

    // Trace: no spec clause (test-harness reporting, D-X.27): a failed step naming no program still shows how it ended.
    #[test]
    fn a_conformance_step_failing_without_a_program_shows_its_end() {
        let report = conformance_failures(Path::new("/nonexistent"), "== conformance (1 programs)\nrun.sh: 3: cc: not found\n");
        assert!(report.contains("named no failed program") && report.contains("    | run.sh: 3: cc: not found\n"), "{report}");
        assert_eq!(describe_status("status 3, expected 0"), "exit status 3, expected 0");
        assert_eq!(describe_status("status 139, expected signal 6"), "killed by signal 11 SIGSEGV (status 139), expected signal 6");
        assert_eq!(describe_status("stderr lacks: x"), "stderr lacks: x");
    }

    // Trace: no spec clause (D-X.27): the report reads `run.sh`'s own lines and files; this fails when their format moves.
    #[test]
    fn the_conformance_report_matches_the_runner_formats() {
        let run = fs::read_to_string(root().join("conformance").join("run.sh")).unwrap();
        for want in [
            r#"log="$here/build/$name.log""#,
            r#"2>"$log.stderr" >"$log.stdout""#,
            r#"echo "conformance: $name: status $rc, expected $want_exit""#,
            r#"echo "conformance: $name: stderr lacks: $text""#,
            r#"echo "conformance: FAIL $name (stderr: conformance/build/$name.log.stderr)""#,
            r#"echo "conformance: FAIL $name (does not build; see conformance/build/$name.log)""#,
        ] {
            assert!(run.contains(want), "conformance/run.sh no longer has {want:?}: update conformance_failures");
        }
    }

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn repo() -> &'static Repo {
        static REPO: std::sync::OnceLock<Repo> = std::sync::OnceLock::new();
        // The sync's `.changed.hdr` (present in a VM tree) must not decide the unit tests: no header diff here.
        REPO.get_or_init(|| Repo { hdr_diff: None, xtask_diff: None, ..Repo::load(&root()) })
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
            "crates/core/src/unicode/tables.rs",
            // XTASK-AFFECTED-MATH(-2): the math generated files outside the mapped set still escalate.
            "crates/math/fits/fast32_tables_gen.py",
            "crates/math/fits/sub/x.fit",
            "crates/math/fits/removed_in_this_change.fit",
            "crates/mathtools/src/sub/tables.rs",
            "crates/math/src/coeffs/sub/x.rs",
            "csys/src/adapter/math_libm.h",
            "csys/src/adapter/other.c",
            "crates/core/src/foo_gen.rs",
            "crates/libc-so/src/lib.rs",
            "scripts/anything.sh",
            "somewhere/unknown.bin",
        ] {
            let (_, s) = sel(&[f]);
            assert!(!s.full.is_empty(), "{f} must escalate to full");
        }
    }

    // XTASK-AFFECTED-MATH (ruling #3188), no spec clause: a change of the math export wiring or of a generated
    // coefficient file selects the math export tests and the generator's check, not FULL, in the direct and wide tiers.
    #[test]
    fn math_wiring_and_coefficients_select_the_math_suites_not_full() {
        let math_set: &[(&str, &str)] = &[
            ("liella-boundary", "s10_math_symbols"),
            ("liella-boundary", "math_fma_paths"),
            ("liella-boundary", "libm_fma_select"),
            ("liella-math", "s10_math_clauses"),
            ("liella-math", "special_products"),
            ("liella-math", "elem_products"),
            ("liella-math", "fast64"),
        ];
        for (f, cmd) in [
            (MATH_TSV, "math-gen --check"),
            (MATH_GEN, "math-gen --check"),
            ("csys/src/adapter/math_libm.c", "math-gen --check"),
            ("crates/math/src/coeffs/trig.rs", "tables --check"),
            ("crates/math/src/coeffs/fast32_sin.rs", "tables --check"),
        ] {
            for tier in [Tier::Direct, Tier::Wide] {
                let s = select_tier(repo(), &[f.to_string()], tier);
                assert!(s.full.is_empty(), "{f} ({tier:?}) must not escalate: {:?}", s.full);
                assert!(s.xtask_cmds.contains(cmd), "{f} ({tier:?}): `cargo xtask {cmd}` not selected: {:?}", s.xtask_cmds);
                for (c, t) in math_set {
                    assert!(s.tests.contains_key(&(c.to_string(), t.to_string())), "{f} ({tier:?}): {c}::{t} not selected");
                }
                // Every liella-math suite, and every target whose own source names the math exports or libm.
                for t in repo().targets.iter().filter(|t| {
                    let own = &t.text[..t.own.min(t.text.len())];
                    t.krate == MATH_CRATE || own.contains("s10_math") || own.contains("libm")
                }) {
                    assert!(s.tests.contains_key(&(t.krate.clone(), t.name.clone())), "{f} ({tier:?}): {}::{} not selected", t.krate, t.name);
                }
                assert!(s.libs.contains_key("liella-boundary") && s.libs.contains_key("liella-math"), "{f} ({tier:?}): unit tests {:?}", s.libs.keys());
                // Targeted, not everything: a test of another section stays out.
                assert!(!s.tests.contains_key(&("liella-core".to_string(), "s04_strings_clauses".to_string())), "{f} ({tier:?}) selected a string test");
            }
        }
        // The coefficient family's users come from the `coeffs` module: its one-hop users' targets are a subset.
        let c = select_tier(repo(), &["crates/math/src/coeffs/trig.rs".to_string()], Tier::Direct);
        let m = select_tier(repo(), &["crates/math/src/coeffs.rs".to_string()], Tier::Direct);
        for k in m.tests.keys() {
            assert!(c.tests.contains_key(k), "coeffs/trig.rs misses {k:?}, which a change of coeffs.rs selects");
        }
    }

    // D-X.39, no spec clause: each generator input under crates/mathtools/src/ selects its own mapping and the
    // check of every generated set it feeds (remez: all fits in one `remez --check`; tables: `tables --check` and
    // `kernel-index --check`); the comparison tool's sources select neither. Not FULL, in the direct and wide tiers.
    #[test]
    fn mathtools_generator_inputs_select_their_checks() {
        let all_fits = format!("remez --check {}", repo().fits.iter().map(String::as_str).collect::<Vec<_>>().join(" "));
        assert!(repo().fits.len() > 1, "the fit files of the tree: {:?}", repo().fits);
        let both: &[&str] = &[&all_fits, "tables --check", "kernel-index --check"];
        let remez: &[&str] = &[&all_fits];
        let tables: &[&str] = &["tables --check", "kernel-index --check"];
        let cases: &[(&str, &[&str])] = &[
            ("crates/mathtools/src/remez.rs", remez),
            ("crates/mathtools/src/bin/remez.rs", remez),
            ("crates/mathtools/src/bessel_tables.rs", tables),
            ("crates/mathtools/src/gamma_tables.rs", tables),
            ("crates/mathtools/src/special_tables.rs", tables),
            ("crates/mathtools/src/bin/tables.rs", tables),
            ("crates/mathtools/src/lib.rs", both),
            ("crates/mathtools/src/mp/func.rs", both),
            ("crates/mathtools/src/mp/nat.rs", both),
            ("crates/mathtools/src/cmp.rs", &[]),
            ("crates/mathtools/src/cplx.rs", &[]),
            ("crates/mathtools/src/bin/mathcmp.rs", &[]),
        ];
        for tier in [Tier::Direct, Tier::Wide] {
            for (f, want) in cases {
                assert!(root().join(f).is_file(), "{f} is not in the tree");
                let s = select_tier(repo(), &[f.to_string()], tier);
                assert!(s.full.is_empty(), "{f} ({tier:?}) must not escalate: {:?}", s.full);
                assert!(s.libs.contains_key("liella-mathtools"), "{f} ({tier:?}): its own unit tests {:?}", s.libs.keys());
                let checks: BTreeSet<&str> = s.xtask_cmds.iter().map(String::as_str).filter(|c| c.starts_with("remez") || c.starts_with("tables") || c.starts_with("kernel-index")).collect();
                assert_eq!(checks, want.iter().copied().collect::<BTreeSet<_>>(), "{f} ({tier:?})");
            }
        }
    }

    // XTASK-AFFECTED-MATH-2 (ruling #3204), no spec clause: a fit file selects its family's math set (everything its
    // generated coefficient file selects) plus `remez --check <fit>`; the tables generator selects its own mapping as a
    // liella-mathtools source, the whole math set, `tables --check` and `kernel-index --check`. Not FULL, in the direct
    // and wide tiers.
    #[test]
    fn math_fits_and_tables_generator_select_the_math_set_not_full() {
        let fit = "crates/math/fits/fast32_sin.fit";
        assert!(repo().fits.contains(fit), "the fit files of the tree: {:?}", repo().fits);
        for tier in [Tier::Direct, Tier::Wide] {
            let coeffs = select_tier(repo(), &["crates/math/src/coeffs/fast32_sin.rs".to_string()], tier);
            for (f, cmds) in [
                (fit, &["remez --check crates/math/fits/fast32_sin.fit", "tables --check"][..]),
                (MATH_TABLES_GEN, &["tables --check", "kernel-index --check"][..]),
            ] {
                let s = select_tier(repo(), &[f.to_string()], tier);
                assert!(s.full.is_empty(), "{f} ({tier:?}) must not escalate: {:?}", s.full);
                for cmd in cmds {
                    assert!(s.xtask_cmds.contains(*cmd), "{f} ({tier:?}): `cargo xtask {cmd}` not selected: {:?}", s.xtask_cmds);
                }
                // Everything the family's coefficient file selects (the math export suites, every liella-math suite).
                for k in coeffs.tests.keys() {
                    assert!(s.tests.contains_key(k), "{f} ({tier:?}) misses {k:?}, which coeffs/fast32_sin.rs selects");
                }
                for k in coeffs.libs.keys().chain(coeffs.clippy.keys()) {
                    assert!(s.libs.contains_key(k) || s.clippy.contains_key(k), "{f} ({tier:?}) misses the crate {k}");
                }
                assert!(!s.tests.contains_key(&("liella-core".to_string(), "s04_strings_clauses".to_string())), "{f} ({tier:?}) selected a string test");
            }
            // The generator is also a liella-mathtools source: its own crate's unit tests and its ordinary mapping.
            let s = select_tier(repo(), &[MATH_TABLES_GEN.to_string()], tier);
            assert!(s.libs.contains_key("liella-mathtools"), "{MATH_TABLES_GEN} ({tier:?}): unit tests {:?}", s.libs.keys());
            let mut own = Selection { direct: tier == Tier::Direct, ..Selection::default() };
            ordinary_mapping(repo(), MATH_TABLES_GEN, &mut own);
            assert!(own.libs.contains_key("liella-mathtools"), "the ordinary mapping of {MATH_TABLES_GEN} ({tier:?}): {:?}", own.libs.keys());
            for k in own.libs.keys() {
                assert!(s.libs.contains_key(k), "{MATH_TABLES_GEN} ({tier:?}) misses the unit tests of {k} of its own mapping");
            }
            for k in own.tests.keys() {
                assert!(s.tests.contains_key(k), "{MATH_TABLES_GEN} ({tier:?}) misses {k:?} of its own mapping");
            }
            for k in own.conf.keys() {
                assert!(s.conf.contains_key(k), "{MATH_TABLES_GEN} ({tier:?}) misses the program {k} of its own mapping");
            }
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

    // XTASK-AFFECTED-DOCS-PATHS (D-X.35); no spec clause. A results table alone: `cargo xtask static`, no tests, in
    // every tier but full (it used to escalate: "not a path the selection knows"). Paths are built at run time so this
    // test's own text does not name them.
    #[test]
    fn docs_paths_select_static_only() {
        let made_up = ["results", "spec/security", "spec"].map(|d| format!("{d}/{}.md", "docs-paths-made-up"));
        for f in made_up.iter().map(String::as_str).chain(["results/timing-sweep-1105.md", "spec/ERRATA.md", "docs/benchmarks.md"]) {
            for t in [Tier::Direct, Tier::Wide] {
                let s = tier(&[f], t);
                assert!(s.full.is_empty() && s.tests.is_empty() && s.conf.is_empty() && s.libs.is_empty() && s.clippy.is_empty() && !s.xtask_tests, "{f} ({t:?}): {s:?}");
                assert_eq!(s.xtask_cmds, BTreeSet::from(["static".to_string()]), "{f} ({t:?})");
            }
        }
        // A path cited only in comments (the tests that cite the threat model) is no read.
        let s = tier(&["spec/security/threat-model.md"], Tier::Direct);
        assert!(s.tests.is_empty() && s.libs.is_empty() && !s.xtask_tests, "{s:?}");
    }

    // XTASK-AFFECTED-DOCS-PATHS; no spec clause. A docs file that code reads selects its readers and nothing wider.
    #[test]
    fn docs_read_by_code_select_their_readers() {
        let has = |s: &Selection, k: &str, n: &str| s.tests.contains_key(&(k.to_string(), n.to_string()));
        for t in [Tier::Direct, Tier::Wide] {
            let s = tier(&["spec/behaviour/aio.glibc.txt"], t);
            assert!(s.full.is_empty() && has(&s, "liella", "aio_products") && s.xtask_cmds.contains("static"), "{t:?}: {s:?}");
            assert!(!has(&s, "liella", "syslog_products"), "only the readers: {s:?}");
            // Read by a test target and by a crate's unit-test module (`pattern::regex_oracle_diff`).
            let s = tier(&["spec/behaviour/patterns.glibc.txt.gz"], t);
            assert!(has(&s, "liella", "regex_record") && has(&s, "liella", "patterns_products") && s.libs.contains_key("liella"), "{t:?}: {s:?}");
            // Read by this command (`Repo::load`), so by the xtask unit tests.
            let s = tier(&["docs/codemap/symbols.tsv"], t);
            assert!(s.full.is_empty() && s.xtask_tests, "{t:?}: {s:?}");
        }
    }

    // XTASK-AFFECTED-DOCS-PATHS; no spec clause. Mixed with code: the code's selection as before, plus `static`.
    #[test]
    fn docs_mixed_with_code_select_as_before() {
        let doc = format!("results/{}.md", "docs-paths-made-up");
        assert!(!tier(&[doc.as_str(), "crates/abi/src/lib.rs"], Tier::Direct).full.is_empty(), "code still escalates");
        for code in ["crates/kernel/src/lib.rs", "crates/core/tests/aio_products.rs", "conformance/c/fpu_control_own.c"] {
            for t in [Tier::Direct, Tier::Wide] {
                let (a, b) = (tier(&[code], t), tier(&[code, doc.as_str()], t));
                assert!(a.full == b.full && a.tests.keys().eq(b.tests.keys()) && a.conf.keys().eq(b.conf.keys()) && a.libs.keys().eq(b.libs.keys()) && a.xtask_tests == b.xtask_tests, "{code} ({t:?})");
                assert!(b.xtask_cmds.contains("static"), "{code} ({t:?})");
            }
        }
    }

    #[test]
    fn names_path_needs_a_quoted_path_outside_comments() {
        assert!(names_path("let p = root.join(\"spec/behaviour/a.txt\");", "spec/behaviour/a.txt"));
        assert!(names_path("include_bytes!(\"../../spec/behaviour/a.txt\")", "spec/behaviour/a.txt"));
        for no in ["//! see spec/behaviour/a.txt", "    /// \"spec/behaviour/a.txt\"", " * \"spec/behaviour/a.txt\"", "let p = \"xspec/behaviour/a.txt\";"] {
            assert!(!names_path(no, "spec/behaviour/a.txt"), "{no}");
        }
        // A top-level file: a bare name in a list is no path; a joined or `/`-prefixed one is.
        assert!(!names_path("const SKIP: &[&str] = &[\"Makefile\", \"README.md\"];", "README.md"));
        assert!(names_path("root.join(\"README.md\")", "README.md") && names_path("\"../../README.md\"", "README.md"));
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
        let mut r = Repo { hdr_diff: None, xtask_diff: None, ..Repo::load(&root()) };
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
        assert_eq!(csys_symbols("a.S", ".macro M\n.endm\n"), Some(vec![]));
        assert_eq!(csys_symbols("a.S", ".macro M b\n.Lx\\@:\n\tjmp liella_y+\\b\n.endm\n#define SLOT 4032\nliella_z:\n"), Some(vec!["liella_z".into()]));
        assert_eq!(csys_symbols("a.S", ".macro M\n\t.globl liella_a\n.endm\n"), None);
        assert_eq!(csys_symbols("a.S", ".macro M\nliella_a:\n.endm\n"), None);
        assert_eq!(csys_symbols("a.S", "#define memset liella_a\n"), None);
        assert_eq!(csys_symbols("a.S", "#define SLOT(x) x\n"), None);
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
    fn a_bin_only_package_runs_its_unit_tests_through_its_bins() {
        // no spec clause: XTASK-AFFECTED-XTASK-TESTS (D-X.41). xtask has no library target, so `--lib` fails the build;
        // a library crate keeps `--lib`. xtask is reached through `wide` of a crate it depends on (liella-ld).
        let r = repo();
        assert_eq!(unit_tests_flag(r, "xtask"), "--bins");
        assert_eq!(unit_tests_flag(r, "liella"), "--lib");
        assert_eq!(unit_tests_flag(r, "liella-ld"), "--lib");
        let mut s = Selection::default();
        s.wide(r, "liella-ld", "probe");
        assert!(s.libs.contains_key("xtask"), "xtask depends on liella-ld: {:?}", s.libs.keys());
    }

    const MODULE: &str = "fn run() {\n    work();\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn t() {\n        run();\n    }\n}\n";

    fn diff_of(file: &str, hunks: &str) -> String {
        format!("diff --git a/{file} b/{file}\nindex 1111111..2222222 100644\n--- a/{file}\n+++ b/{file}\n{hunks}")
    }

    #[test]
    fn a_change_inside_the_test_module_is_test_code() {
        // no spec clause: D-X.41. MODULE's test body is lines 7..=12; changed lines there (or a deletion between two of
        // them) touch test code only.
        let f = "xtask/src/x.rs";
        assert_eq!(test_module_interior(MODULE), Some((7, 12)));
        assert!(test_module_change(&diff_of(f, "@@ -10,0 +11 @@ fn t() {\n+        run();\n"), f, MODULE));
        assert!(test_module_change(&diff_of(f, "@@ -9,2 +9,2 @@\n-    #[test]\n-    fn u() {\n+    #[test]\n+    fn t() {\n"), f, MODULE));
        assert!(test_module_change(&diff_of(f, "@@ -11 +10,0 @@\n-        gone();\n"), f, MODULE));
        assert!(test_module_change(&diff_of(f, "@@ -6,0 +7 @@\n+    use std::fs;\n"), f, MODULE));
    }

    #[test]
    fn a_change_outside_the_test_module_stays_full() {
        // no spec clause: D-X.41. Executed code, the module's boundaries, a column-0 line, a new or renamed file, a
        // second section, no diff for the file, or a file without a trailing test module: not test-only.
        let f = "xtask/src/x.rs";
        let no = |d: &str, text: &str| assert!(!test_module_change(d, f, text), "{d}");
        no(&diff_of(f, "@@ -2 +2 @@\n-    work();\n+    other();\n"), MODULE);
        no(&diff_of(f, "@@ -2 +2 @@\n-    work();\n+    other();\n@@ -11 +11 @@\n-        run();\n+        run2();\n"), MODULE);
        no(&diff_of(f, "@@ -5 +5 @@\n-#[cfg(not(test))]\n+#[cfg(test)]\n"), MODULE);
        no(&diff_of(f, "@@ -13 +13 @@\n-}\n+}\n"), MODULE);
        no(&diff_of(f, "@@ -10,0 +11 @@\n+}\n"), MODULE);
        // A removed column-0 line `--- x` inside a hunk is a body line, not a header to skip.
        no(&diff_of(f, "@@ -11 +10,0 @@\n---- x\n"), MODULE);
        no(&format!("diff --git a/{f} b/{f}\nnew file mode 100644\n--- /dev/null\n+++ b/{f}\n@@ -0,0 +11 @@\n+        run();\n"), MODULE);
        no(&diff_of("xtask/src/y.rs", "@@ -10,0 +11 @@\n+        run();\n"), MODULE);
        no(&format!("{}\n{}", diff_of(f, "@@ -10,0 +11 @@\n+        run();"), diff_of(f, "@@ -2 +2 @@\n-    a();\n+    b();\n")), MODULE);
        no(&diff_of(f, "@@ -10,0 +11 @@\n+        run();\n"), &format!("{MODULE}fn after() {{}}\n"));
        no(&diff_of(f, "@@ -10,0 +11 @@\n+        run();\n"), "fn run() {}\n");
        no(&diff_of(f, ""), MODULE);
    }

    #[test]
    fn xtask_test_code_selects_the_xtask_tests_and_static_not_full() {
        // no spec clause: D-X.41. xtask/tests/ always; a source only with a diff confined to its test module (the
        // real file and its own text); without a diff, or for executed code, FULL as before. --tier full stays FULL.
        let (_, s) = sel(&["xtask/tests/affected_mode.rs"]);
        assert!(s.full.is_empty() && s.xtask_tests && s.xtask_cmds.contains("static") && s.clippy.contains_key("xtask") && s.tests.is_empty() && s.conf.is_empty() && s.libs.is_empty(), "{s:?}");
        assert!(!select_tier(repo(), &["xtask/tests/affected_mode.rs".to_string()], Tier::Full).full.is_empty());
        let f = "xtask/src/so.rs";
        let text = &repo().xtask_texts.iter().find(|(p, _)| p == f).expect("so.rs").1;
        let (lo, hi) = test_module_interior(text).expect("so.rs ends with its test module");
        let mut r = Repo { hdr_diff: None, xtask_diff: None, ..Repo::load(&root()) };
        assert!(!select_tier(&r, &[f.to_string()], Tier::Direct).full.is_empty(), "no diff: FULL");
        r.xtask_diff = Some(diff_of(f, &format!("@@ -{lo},0 +{} @@\n+        let _ = 1;\n", lo + 1)));
        let s = select_tier(&r, &[f.to_string()], Tier::Direct);
        assert!(s.full.is_empty() && s.xtask_tests && s.xtask_cmds.contains("static"), "{s:?}");
        assert!(!select_tier(&r, &[f.to_string()], Tier::Full).full.is_empty(), "--tier full stays FULL");
        r.xtask_diff = Some(diff_of(f, &format!("@@ -{},0 +{} @@\n+        let _ = 1;\n", lo - 3, lo - 2)));
        assert!(!select_tier(&r, &[f.to_string()], Tier::Direct).full.is_empty(), "executed code: FULL");
        assert!(hi > lo);
    }

    #[test]
    fn a_plants_change_selects_exactly_the_xtask_tests_and_clippy() {
        // no spec clause: ruling #2993, plants.rs is a leaf like the audit modules, not FULL.
        let (_, s) = sel(&["xtask/src/plants.rs"]);
        assert!(s.full.is_empty(), "plants.rs must not escalate to FULL: {s:?}");
        assert!(s.xtask_tests && s.xtask_cmds.is_empty() && s.tests.is_empty() && s.conf.is_empty(), "{s:?}");
        assert!(s.clippy.len() == 1 && s.clippy.contains_key("xtask"), "only the xtask clippy: {s:?}");
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

    // XTASK-PLANTS-TRACKED-COPY red-before-green: legacy::intl has no section and no `posix/sNN` one-hop user (its users
    // are boundary::catalog and boundary::data); the gettext entries of posix/s07_locale name boundary::catalog. The
    // products test diffing the catalogs names no module, only §7.7/§7.8. Before: not selected for an intl.rs change.
    #[test]
    fn direct_tier_selects_the_products_tests_of_the_c_entries_a_helper_reaches() {
        let repo = repo();
        let t = repo.targets.iter().find(|t| t.name == "catalogs_products").unwrap_or_else(|| panic!("catalogs_products"));
        assert!(t.products && t.sections.contains(&7), "catalogs_products diffs the products and carries §7");
        let d = tier(&["crates/legacy/src/intl.rs"], Tier::Direct);
        assert!(d.full.is_empty() && d.direct, "{:?}", d.full);
        let k = (t.krate.clone(), t.name.clone());
        assert!(d.tests.contains_key(&k), "intl.rs must select catalogs_products: {:?}", d.tests.keys().collect::<Vec<_>>());
        assert!(d.tests[&k].contains("C entry the change reaches"), "{}", d.tests[&k]);
        let w = tier(&["crates/legacy/src/intl.rs"], Tier::Wide);
        assert!(d.tests.keys().all(|k| w.tests.contains_key(k)), "direct stays inside wide");
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
