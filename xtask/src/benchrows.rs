//! `cargo xtask bench-rows [--base <ref>] [--range <a>..<b>] [--files <path>,...] [--out <file>] [--all-rows]`:
//! the bench rows a change can move (BENCH-AFF; owner 2026-10-10: "more targeted time tests on shiki, only
//! testing the things that are different in the merge").
//!
//! The selection, CONSERVATIVE like `affected` (an extra row costs time, a missed row hides a regression):
//!
//! 1. Changed files: as `affected` (`--base`, default `origin/main`; the `.changed` list in a VM tree without
//!    git history), or `--files`, or `--range a..b` (the names `git diff --name-only a b` lists: replaying a
//!    past merge for validation).
//! 2. Per file, in order:
//!    * NO TIMING EFFECT: docs, plans, handoffs, `*.md`, `spec/behaviour`, the operator tools, validation,
//!      conformance programs, fuzz harnesses, test files and non-bench examples of the crates, the crates no
//!      bench links (`mathtools`, `spectest`, `thread-db`), `bench/host_limits.tsv`, `bench/parity-measure.tsv`,
//!      `bench/sentinels.tsv`, xtask modules other than the bench and build ones.
//!    * FULL (every row; `FULL_TRIGGERS`): build configuration (`Cargo.*`, the toolchain, `.cargo/`, build
//!      scripts), `csys/include`, link and export lists, the allocator, startup and the export machinery, the
//!      bench runner and product build (`xtask/src/{bench,so,csys,benchrows}.rs`), the product crates
//!      `libc-so`/`libm-so`. (`bench/budgets.tsv` changes rules, not timings: a new row comes with its source.)
//!    * A bench header (`bench/c/*.h`): every row of the benches that include it.
//!    * A bench source (`bench/c/<b>.c`, `crates/boundary/examples/<b>.rs`): every row of that bench.
//!    * Anything else goes through `affected`'s model: its FULL escalations are FULL here; a crate it selects
//!      WIDE is FULL here (but the three crates above); a narrowed change gives the reached modules.
//!      The runtime crate (`crates/rt`, WIDE in `affected`) is narrowed here: its module `m` stands for the
//!      files of the narrowed crates that name `liella_rt::m` (`start` and its root stay FULL).
//!      `--narrow-allocator` (env `BENCH_NARROW_ALLOC=1` for `bench --affected`; RULING NEEDED, off by default):
//!      the allocator is not FULL; its entry points (`malloc`, `free`, ...) and the module graph select rows.
//! 3. Reached modules -> exported symbols: in each boundary file with `export!` blocks whose module is reached,
//!    the blocks that name a reached module (or a file-local helper that does, closed over helpers; a `use`
//!    of a reached module's items counts those items). A file that changed itself, an occurrence outside every
//!    block and helper, or a reached file where no block names anything: every export of the file. A symbol
//!    the blocks do not define (macro-generated, the math tables) is reached when its file's module is
//!    (`docs/codemap/symbols.tsv`). The benches' timer (`clock_gettime`) is not a trigger: it runs twice
//!    per round of millions of operations, outside the per-operation figure.
//! 4. Symbols -> rows, from the bench sources at run time (generated, never hand-kept): each MEASURE site
//!    (and each macro that wraps MEASURE) names its row (a literal, or a literal prefix for wrappers that
//!    append a size tag); its symbols are the exported names in its arguments, closed over the file's own
//!    functions and over the right-hand sides of assignments to the names it uses (function pointers like
//!    `strlen_fn = strlen`). A row no site names, or with a symbol whose file is unknown, is UNMAPPED and
//!    runs on every non-docs change. The Rust benches (`bench_*`) run whole on any product source change.
//! 5. Plus the SENTINEL rows (`bench/sentinels.tsv`), always: in one fat-LTO library any change can move an
//!    untouched function by placement alone (MEM-K5c: memset_16 0.94 -> 1.13), so the hottest rows run on
//!    every targeted bench. A row a `ratio_to` row names is kept with it.
//!
//! What a targeted bench can miss (the nightly full baseline on main is the net): placement and alignment
//! effects on rows that are neither selected nor sentinels; a C header macro that calls a symbol the bench
//! source does not name (the ctype macros are mapped by `ALIASES`); trait calls `affected`'s name graph does
//! not see; state a skipped earlier row would have left (heap shape, caches) - both libraries skip alike.
//!
//! `--out <file>` writes `bench<TAB>row<TAB>reason` lines (`*<TAB>*<TAB>reason` for FULL), the input of
//! `cargo xtask bench --rows-file`; `cargo xtask bench --affected` runs this selection itself.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::affected::{self, Repo};

/// The selected rows `(bench, row) -> reason`; `None` = every row (FULL).
pub type Selected = Option<BTreeMap<(String, String), String>>;

/// A top-level item of a C source: name, start and end offsets.
type Span = (String, usize, usize);

/// The sentinel rows (always timed by a targeted bench).
pub const SENTINELS: &str = "bench/sentinels.tsv";

/// Paths whose change can move every row (prefix match; `*` = suffix match on the file name).
const FULL_TRIGGERS: &[(&str, &str)] = &[
    ("Cargo.toml", "build configuration"),
    ("Cargo.lock", "build configuration"),
    ("rust-toolchain.toml", "build configuration (the toolchain)"),
    (".cargo/", "build configuration"),
    ("*build.rs", "a build script"),
    ("*Cargo.toml", "a crate manifest (features, profiles, dependencies)"),
    ("csys/include/", "C headers of the products"),
    ("*.ld", "a linker script"),
    ("*.lds", "a linker script"),
    ("*.map", "a version script / export list"),
    ("*.ver", "a version script"),
    ("crates/boundary/src/heap.rs", "the allocator"),
    ("crates/boundary/src/native/heap.rs", "the allocator"),
    ("crates/legacy/src/malloc_compat", "the allocator"),
    ("crates/mem/", "the allocator"),
    ("crates/rt/src/heap.rs", "the allocator"),
    ("crates/rt/build.rs", "a build script"),
    ("crates/boundary/src/runtime.rs", "startup"),
    ("crates/boundary/src/posix/s01_startup.rs", "startup"),
    ("crates/boundary/src/lib.rs", "the export machinery (crate root)"),
    ("crates/boundary/src/export.rs", "the export machinery"),
    ("crates/libc-so/", "the shared-object product"),
    ("crates/libm-so/", "the shared-object product"),
    ("xtask/src/bench.rs", "the bench runner"),
    ("xtask/src/benchrows.rs", "the bench row selector"),
    ("xtask/src/so.rs", "the product build"),
    ("xtask/src/csys.rs", "the product build"),
];

/// Paths with no timing effect (prefix; `*` = suffix).
const NO_TIMING: &[&str] = &[
    "docs/", "plan/", "handoff/", "*.md", "spec/behaviour/", "spec/schedule.tsv", "tools/", "validation/", "conformance/", "fuzz/", "benchmarks/",
    "bench/host_limits.tsv", "bench/parity-measure.tsv", "bench/sentinels.tsv", "bench/budgets.tsv", "crates/mathtools/", "crates/spectest/", "crates/thread-db/", "xtask/",
];

/// Crates no bench links: `affected` selecting them WIDE does not make a bench FULL.
const NOT_LINKED: &[&str] = &["liella-mathtools", "spectest", "liella-thread-db"];

/// C header macros that call an exported symbol the bench source does not name.
const ALIASES: &[(&str, &[&str])] = &[
    ("isalnum", &["__ctype_b_loc"]),
    ("isalpha", &["__ctype_b_loc"]),
    ("isblank", &["__ctype_b_loc"]),
    ("iscntrl", &["__ctype_b_loc"]),
    ("isdigit", &["__ctype_b_loc"]),
    ("isgraph", &["__ctype_b_loc"]),
    ("islower", &["__ctype_b_loc"]),
    ("isprint", &["__ctype_b_loc"]),
    ("ispunct", &["__ctype_b_loc"]),
    ("isspace", &["__ctype_b_loc"]),
    ("isupper", &["__ctype_b_loc"]),
    ("isxdigit", &["__ctype_b_loc"]),
    ("tolower", &["__ctype_tolower_loc"]),
    ("toupper", &["__ctype_toupper_loc"]),
    ("putc", &["_IO_putc"]),
    ("getc", &["_IO_getc"]),
];

fn matches(path: &str, pat: &str) -> bool {
    match pat.strip_prefix('*') {
        Some(suffix) => path.rsplit('/').next().is_some_and(|f| f.ends_with(suffix)),
        None => path == pat || (pat.ends_with('/') && path.starts_with(pat)) || (!pat.ends_with('/') && path.starts_with(pat)),
    }
}

// ---------------------------------------------------------------------------------------------
// The bench rows (bench/budgets.tsv)

/// `(bench, row, rule, bound)` of every budgets row, in file order.
pub fn budget_rows(root: &Path) -> Result<Vec<[String; 4]>, String> {
    let text = fs::read_to_string(root.join("bench/budgets.tsv")).map_err(|e| format!("bench/budgets.tsv: {e}"))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 4).then(|| [f[0].to_string(), f[1].to_string(), f[2].to_string(), f[3].to_string()])
        })
        .collect())
}

/// The sentinel rows `(bench, row)` of `bench/sentinels.tsv`.
pub fn sentinels(root: &Path) -> Result<Vec<(String, String)>, String> {
    let text = fs::read_to_string(root.join(SENTINELS)).map_err(|e| format!("{SENTINELS}: {e}"))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?.to_string(), f.next()?.to_string()))
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// The C bench sources: row -> symbols

/// `text` with comments replaced by spaces (newlines kept); string and character literals kept.
fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    out.push(b' ');
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                out.extend_from_slice(b"  ");
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    out.push(if b[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
                out.extend_from_slice(b"  ");
                i = (i + 2).min(b.len());
            }
            q @ (b'"' | b'\'') => {
                out.push(q);
                i += 1;
                while i < b.len() && b[i] != q && b[i] != b'\n' {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        out.push(b[i]);
                        i += 1;
                    }
                    out.push(b[i]);
                    i += 1;
                }
                if i < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `s` with the contents of string and character literals blanked (identifiers only from code).
fn blank_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    let mut esc = false;
    for c in s.chars() {
        match quote {
            Some(q) => {
                if esc {
                    esc = false;
                    out.push(' ');
                } else if c == '\\' {
                    esc = true;
                    out.push(' ');
                } else if c == q {
                    quote = None;
                    out.push(c);
                } else {
                    out.push(' ');
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

fn idents(s: &str) -> BTreeSet<String> {
    blank_strings(s).split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')).map(str::to_string).collect()
}

/// The code without preprocessor lines (blanked, continuations included) and the function-like macros it
/// defines, `(name, body)`.
fn split_pp(code: &str) -> (String, Vec<(String, String)>) {
    let mut out = String::with_capacity(code.len());
    let mut macros = Vec::new();
    let mut cont = false;
    let mut cur: Option<(String, String)> = None;
    for l in code.split('\n') {
        if cont || l.trim_start().starts_with('#') {
            cont = l.trim_end().ends_with('\\');
            if let Some(rest) = l.trim_start().strip_prefix('#').map(str::trim_start).and_then(|r| r.strip_prefix("define")) {
                let rest = rest.trim_start();
                let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if rest[name.len()..].starts_with('(') {
                    cur = Some((name, String::new()));
                }
            }
            if let Some((_, body)) = cur.as_mut() {
                body.push_str(l);
                body.push('\n');
            }
            if !cont {
                macros.extend(cur.take());
            }
            out.push('\n');
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    macros.extend(cur.take());
    (out, macros)
}

/// The index of the `)` matching the `(` at `open`, skipping literals.
fn matching(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The top-level comma-separated arguments of `s` (the text between a call's parentheses).
fn split_args(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut quote, mut esc) = (Vec::new(), String::new(), 0i32, None::<char>, false);
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// Top-level items of the code: functions `name -> body` and initialized globals `name -> initializer`, and
/// their spans `(name, start, end)` in `code`.
fn top_items(code: &str) -> (BTreeMap<String, String>, Vec<Span>) {
    let b = code.as_bytes();
    let mut items = BTreeMap::new();
    let mut spans = Vec::new();
    let (mut start, mut i) = (0usize, 0usize);
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => {
                let head = &code[start..i];
                let name = if let Some(eq) = head.find('=') {
                    head[..eq].trim_end().trim_end_matches(']').rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '[')).next().unwrap_or("").split('[').next().unwrap_or("").to_string()
                } else if let Some(p) = last_top_paren(head) {
                    // The parameter list is the last top-level `(` (an `__attribute__((...))` comes before it).
                    head[..p].trim_end().rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("").to_string()
                } else {
                    String::new()
                };
                let mut d = 0i32;
                let mut j = i;
                while j < b.len() {
                    match b[j] {
                        b'{' => d += 1,
                        b'}' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        q @ (b'"' | b'\'') => {
                            j += 1;
                            while j < b.len() && b[j] != q {
                                if b[j] == b'\\' {
                                    j += 1;
                                }
                                j += 1;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if !name.is_empty() {
                    spans.push((name.clone(), i, j.min(b.len())));
                    items.entry(name).or_insert_with(String::new).push_str(&code[i..j.min(b.len())]);
                }
                i = j + 1;
                start = i;
                continue;
            }
            b';' | b'}' => start = i + 1,
            _ => {}
        }
        i += 1;
    }
    (items, spans)
}

/// The position of the last `(` of `head` at parenthesis depth 0.
fn last_top_paren(head: &str) -> Option<usize> {
    let (mut depth, mut last) = (0i32, None);
    for (i, c) in head.char_indices() {
        match c {
            '(' => {
                if depth == 0 {
                    last = Some(i);
                }
                depth += 1;
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    last
}

/// The right-hand sides of every assignment to `name` in `code` (`name ... = rhs;`, declarations and
/// function pointers included; `==` is not an assignment).
fn assigned(code: &str, name: &str) -> String {
    let b = code.as_bytes();
    let mut out = String::new();
    let mut from = 0;
    while let Some(p) = code[from..].find(name) {
        let at = from + p;
        from = at + name.len();
        let before_ok = at == 0 || !(b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_');
        let after_ok = b.get(from).is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'_'));
        if !before_ok || !after_ok {
            continue;
        }
        // Up to `=`, past a declarator's parameter list (`(*f)(int, int) = g`); a `,` outside parentheses
        // opened after the name ends the search (a call argument).
        let (mut j, mut opened) = (from, 0u32);
        while j < b.len() && !matches!(b[j], b'=' | b';' | b'{' | b'}') && !(b[j] == b',' && opened == 0) {
            match b[j] {
                b'(' => opened += 1,
                b')' => opened = opened.saturating_sub(1),
                _ => {}
            }
            j += 1;
        }
        // Not `==`, `!=`, `<=`, `>=` nor a compound assignment (`acc += f(x)`: an accumulator's other terms
        // say nothing about what the name holds).
        if j + 1 < b.len() && b[j] == b'=' && b[j + 1] != b'=' && !matches!(b[j - 1], b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^') {
            let end = code[j..].find(';').map_or(b.len(), |e| j + e);
            out.push_str(&code[j + 1..end]);
            out.push('\n');
        }
    }
    out
}

/// One MEASURE site: the row name (a literal; a wrapper may append a size tag: matched as a prefix) and the
/// names its arguments reach.
#[derive(Debug)]
struct Site {
    /// Its offset in the code (to place it in a function).
    at: usize,
    name: Option<String>,
    reach: BTreeSet<String>,
    /// The names reached without entering a function body (the arguments and what they are assigned).
    direct: BTreeSet<String>,
}

/// The basic memory and string kernels: reached only inside a helper's body (buffer setup, name building)
/// they do not select a row by themselves - their own rows and the sentinels time them, and a composite row
/// (a syscall, a directory scan) is not moved 3% by them. Named in the MEASURE arguments they select it.
const INCIDENTAL: &[&str] = &[
    "memcpy", "memmove", "memset", "memcmp", "memchr", "strlen", "strnlen", "strcmp", "strncmp", "strcpy", "strncpy", "strcat", "strchr", "strrchr", "bzero",
    "explicit_bzero", "mempcpy", "stpcpy",
];

fn sites(text: &str) -> Vec<Site> {
    let (code, macros) = split_pp(&strip_comments(text));
    // `put(name, ns)` prints a row computed outside MEASURE (a cross-thread or cold-start row).
    let mut row_macros: BTreeSet<String> = BTreeSet::from(["MEASURE".to_string(), "MEASURE_PER".to_string(), "put".to_string()]);
    loop {
        let before = row_macros.len();
        for (n, body) in &macros {
            if !row_macros.contains(n) && n != "put" && idents(&body[body.find('(').unwrap_or(0)..]).iter().any(|w| row_macros.contains(w) && w != n) {
                row_macros.insert(n.clone());
            }
        }
        if row_macros.len() == before {
            break;
        }
    }
    let (items, _) = top_items(&code);
    let b = code.as_bytes();
    let mut out = Vec::new();
    for m in &row_macros {
        let mut from = 0;
        while let Some(p) = code[from..].find(m.as_str()) {
            let at = from + p;
            from = at + m.len();
            if (at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_')) || b.get(from).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                continue;
            }
            let open = from + code[from..].len() - code[from..].trim_start().len();
            if b.get(open) != Some(&b'(') {
                continue;
            }
            let Some(close) = matching(b, open) else { continue };
            let args = split_args(&code[open + 1..close]);
            let first = args[0].trim();
            let name = first.strip_prefix('"').and_then(|s| s.strip_suffix('"')).map(str::to_string);
            let mut seed = BTreeSet::new();
            for a in &args[1..] {
                seed.extend(idents(a));
            }
            if m == "put" {
                // A value measured elsewhere (`fresh_process(env, 0, 40, t); put("first_x", t[0]);`): the
                // nearest statement before the `put` calls that is not itself a `put`.
                let mut end = code[..at].rfind(';');
                while let Some(e) = end {
                    let s = code[..e].rfind([';', '{', '}']).map_or(0, |x| x + 1);
                    let stmt = code[s..e].trim();
                    if !stmt.starts_with("put") {
                        seed.extend(idents(stmt));
                        break;
                    }
                    end = (s > 0).then(|| s - 1).filter(|&x| code.as_bytes()[x] == b';');
                }
            }
            // Closure over the file's own functions and assignments (`direct`: assignments only).
            let close = |bodies: bool| {
                let mut reach = BTreeSet::new();
                let mut work: Vec<String> = seed.iter().cloned().collect();
                while let Some(w) = work.pop() {
                    if !reach.insert(w.clone()) {
                        continue;
                    }
                    let mut more = idents(&assigned(&code, &w));
                    if let Some(body) = items.get(&w).filter(|_| bodies) {
                        more.extend(idents(body));
                    }
                    work.extend(more.into_iter().filter(|x| !reach.contains(x)));
                }
                reach
            };
            out.push(Site { at, name, reach: close(true), direct: close(false) });
        }
    }
    out
}

/// Table-driven rows: a row name that only appears as a literal (`{"re_grep_alt", ...}`) belongs to the
/// non-literal sites of the function that holds the literal, or of the functions naming the table that does.
fn table_sites<'a>(text: &str, row: &str, sites: &'a [Site]) -> Vec<&'a Site> {
    let (code, _) = split_pp(&strip_comments(text));
    let (_, spans) = top_items(&code);
    let lit = format!("\"{row}\"");
    let mut fns: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut from = 0;
    while let Some(p) = code[from..].find(&lit) {
        let at = from + p;
        from = at + lit.len();
        for (name, s, e) in spans.iter().filter(|(_, s, e)| *s <= at && at < *e) {
            let is_fn = code[..*s].trim_end().ends_with(')');
            if is_fn {
                fns.insert((*s, *e));
            } else {
                // A table: the functions that name it.
                for (_, s2, e2) in spans.iter().filter(|(_, s2, e2)| idents(&code[*s2..*e2]).contains(name)) {
                    fns.insert((*s2, *e2));
                }
            }
        }
    }
    sites.iter().filter(|s| s.name.is_none() && fns.iter().any(|(a, b)| *a <= s.at && s.at < *b)).collect()
}

/// The `__name` exports of the names in `syms`.
fn underscored(syms: &BTreeSet<String>, exported: &BTreeSet<String>) -> Vec<String> {
    syms.iter().map(|s| format!("__{s}")).filter(|u| exported.contains(u)).collect()
}

/// Per row of `rows`: the exported symbols its MEASURE site reaches (`None`: no site names the row).
fn row_symbols(text: &str, rows: &[String], exported: &BTreeSet<String>) -> BTreeMap<String, Option<BTreeSet<String>>> {
    let sites = sites(text);
    let mut out = BTreeMap::new();
    for r in rows {
        let exact: Vec<&Site> = sites.iter().filter(|s| s.name.as_deref() == Some(r.as_str())).collect();
        let chosen = if !exact.is_empty() {
            exact
        } else {
            let best = sites.iter().filter_map(|s| s.name.as_deref().filter(|n| r.starts_with(&format!("{n}_")))).map(str::len).max();
            let prefixed: Vec<&Site> = sites.iter().filter(|s| best.is_some_and(|l| s.name.as_ref().is_some_and(|n| n.len() == l && r.starts_with(&format!("{n}_"))))).collect();
            if prefixed.is_empty() { table_sites(text, r, &sites) } else { prefixed }
        };
        if chosen.is_empty() {
            out.insert(r.clone(), None);
            continue;
        }
        let mut syms: BTreeSet<String> = BTreeSet::new();
        for s in chosen {
            syms.extend(s.reach.iter().filter(|w| exported.contains(*w) && (s.direct.contains(*w) || !INCIDENTAL.contains(&w.as_str()))).cloned());
            // A header macro may rename a call to its `__` form (`res_mkquery` -> `__res_mkquery`).
            syms.extend(underscored(&s.reach, exported));
        }
        for (k, extra) in ALIASES {
            if syms.contains(*k) {
                syms.extend(extra.iter().map(|e| e.to_string()));
            }
        }
        out.insert(r.clone(), Some(syms));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The exported symbols: module -> symbols

/// One `export!` block: the C names it defines and its line span.
struct Block {
    names: Vec<String>,
    start: usize,
    end: usize,
}

fn blocks_of(lines: &[&str]) -> Vec<Block> {
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.trim_start().starts_with("export!")).map(|(i, _)| i).collect();
    let mut out = Vec::new();
    for (k, &s) in starts.iter().enumerate() {
        let limit = starts.get(k + 1).copied().unwrap_or(lines.len());
        let end = (s..limit).find(|&i| lines[i].trim_end().ends_with("}}")).unwrap_or(limit - 1);
        let mut names = Vec::new();
        for l in &lines[s..=end] {
            if let Some(p) = l.find("fn ") {
                if l[..p].contains("extern") || l.trim_start().starts_with("pub") {
                    let n: String = l[p + 3..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    if !n.is_empty() {
                        names.push(n);
                    }
                }
            }
        }
        out.push(Block { names, start: s, end });
    }
    out
}

/// The export blocks of `lines` a change reaching `words` (module names) touches; `None` when unsure.
fn blocks_hit(lines: &[&str], blocks: &[Block], words: &BTreeSet<String>) -> Option<BTreeSet<usize>> {
    let in_block = |i: usize| blocks.iter().position(|b| b.start <= i && i <= b.end);
    // Helper functions at column 0 outside the blocks.
    let mut helpers: Vec<(String, usize, usize)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if in_block(i).is_some() || l.starts_with(' ') || l.trim_end().ends_with(';') {
            continue;
        }
        let Some(p) = l.find("fn ") else { continue };
        if !l[..p].split_whitespace().all(|w| matches!(w, "pub" | "pub(crate)" | "pub(super)" | "const" | "unsafe" | "extern" | "\"C\"" | "#[inline]")) {
            continue;
        }
        let n: String = l[p + 3..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let end = if l.trim_end().ends_with('}') { i } else { (i + 1..lines.len()).find(|&k| lines[k].starts_with('}'))? };
        helpers.push((n, i, end));
    }
    let word_in = |l: &str, ws: &BTreeSet<String>| l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|w| ws.contains(w));
    let mut words = words.clone();
    let mut hit = BTreeSet::new();
    loop {
        let before = (words.len(), hit.len());
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i].trim_start();
            if l.starts_with("//") {
                i += 1;
                continue;
            }
            if l.starts_with("use ") || l.starts_with("pub use ") || l.starts_with("pub(crate) use ") {
                // A `use` statement: the items imported from a reached module are reached.
                let mut stmt = String::new();
                while i < lines.len() {
                    stmt.push_str(lines[i]);
                    stmt.push(' ');
                    if lines[i].contains(';') {
                        break;
                    }
                    i += 1;
                }
                let toks: Vec<&str> = stmt.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':')).filter(|t| !t.is_empty()).collect();
                for t in toks {
                    let parts: Vec<&str> = t.split("::").collect();
                    if let Some(k) = parts.iter().position(|p| words.contains(*p)) {
                        if k + 1 < parts.len() {
                            words.extend(parts[k + 1..].iter().filter(|p| !p.is_empty()).map(|p| p.to_string()));
                        } else if let Some(brace) = stmt.find(&format!("{}::{{", parts[k])) {
                            // `m::{a, b}`: the braced items.
                            let rest = &stmt[brace + parts[k].len() + 3..];
                            let inner = &rest[..rest.find('}').unwrap_or(rest.len())];
                            words.extend(inner.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty() && *w != "self").map(str::to_string));
                        }
                    }
                }
                i += 1;
                continue;
            }
            if word_in(lines[i], &words) {
                if let Some(b) = in_block(i) {
                    hit.insert(b);
                } else if let Some(h) = helpers.iter().find(|h| h.1 <= i && i <= h.2) {
                    words.insert(h.0.clone());
                } else if !(l.starts_with("mod ") || l.starts_with("pub mod ") || l.starts_with("#[") || l.starts_with("//")) {
                    return None;
                }
            }
            i += 1;
        }
        if (words.len(), hit.len()) == before {
            break;
        }
    }
    Some(hit)
}

/// Every Rust file under `dir` (relative paths with `/`).
fn rust_files(root: &Path, dir: &str, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(root.join(dir)) else { return };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        let rel = format!("{dir}/{}", p.file_name().and_then(|n| n.to_str()).unwrap_or(""));
        if p.is_dir() {
            rust_files(root, &rel, out);
        } else if rel.ends_with(".rs") {
            out.push(rel);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The selection

#[derive(Default, Debug)]
pub struct Plan {
    /// Non-empty: every row (the reasons).
    pub full: Vec<String>,
    /// `(bench, row) -> reason`.
    pub rows: BTreeMap<(String, String), String>,
    pub notes: Vec<String>,
    /// Rows no MEASURE site names or with a symbol of unknown place (run on any non-docs change).
    pub unmapped: Vec<(String, String)>,
}

/// The selection for the changed `files`; sentinels and `ratio_to` bases are added by the caller (`with_extras`).
/// The source files of the narrowed crates that name module `m` of the runtime crate.
fn rt_users(root: &Path, m: &str) -> Vec<String> {
    let mut all = Vec::new();
    for c in ["crates/boundary/src", "crates/core/src", "crates/legacy/src", "crates/math/src"] {
        rust_files(root, c, &mut all);
    }
    let path = format!("liella_rt::{m}");
    all.into_iter()
        .filter(|f| {
            let text = fs::read_to_string(root.join(f)).unwrap_or_default();
            text.contains(&format!("{path}::")) || text.contains(&format!("{path};")) || text.lines().any(|l| l.contains("liella_rt::{") && idents(l).contains(m))
        })
        .collect()
}

/// `narrow_alloc`: the allocator is not a FULL trigger (RULING NEEDED, BENCH-AFF).
pub fn select_with(root: &Path, repo: &Repo, files: &[String], narrow_alloc: bool) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut alloc_reached = false;
    let budget = budget_rows(root)?;
    let benches: BTreeSet<String> = budget.iter().map(|r| r[0].clone()).collect();
    let mut product_change = false;
    let mut through: Vec<String> = Vec::new();
    let mut whole: BTreeMap<String, String> = BTreeMap::new();
    for f in files {
        let f = f.as_str();
        let stem = f.rsplit('/').next().unwrap_or(f).split('.').next().unwrap_or("");
        if (f.starts_with("bench/c/") && f.ends_with(".c")) || (f.starts_with("crates/boundary/examples/") && f.ends_with(".rs")) {
            if benches.contains(stem) {
                whole.insert(stem.to_string(), format!("{f}: the bench source changed"));
            } else {
                plan.notes.push(format!("{f}: not a bench of bench/budgets.tsv (no rows)"));
            }
            continue;
        }
        if let Some(h) = f.strip_prefix("bench/c/").filter(|h| h.ends_with(".h")) {
            // A harness header: every row of the benches that include it (none found: FULL).
            let users: Vec<&String> = benches.iter().filter(|b| fs::read_to_string(root.join(format!("bench/c/{b}.c"))).is_ok_and(|s| s.contains(&format!("#include \"{h}\"")))).collect();
            if users.is_empty() {
                plan.full.push(format!("{f}: a bench header no bench includes by name"));
            }
            for b in users {
                whole.insert(b.clone(), format!("{f}: the bench includes the changed header"));
            }
            continue;
        }
        let full = FULL_TRIGGERS.iter().find(|(p, _)| matches(f, p));
        let quiet = f.contains("/tests/") || f.contains("/examples/") || f.contains("/benches/") || NO_TIMING.iter().any(|p| matches(f, p) && !(*p == "xtask/" && full.is_some()));
        if quiet {
            plan.notes.push(format!("{f}: no timing effect"));
            continue;
        }
        let alloc = full.is_some_and(|(_, why)| *why == "the allocator");
        if let Some((_, why)) = full.filter(|_| !(alloc && narrow_alloc)) {
            plan.full.push(format!("{f}: {why}"));
            continue;
        }
        product_change = true;
        if alloc {
            // --narrow-allocator: the allocator's entry points select the rows that call them; the module graph
            // follows the rest.
            alloc_reached = true;
            plan.notes.push(format!("{f}: the allocator, narrowed (--narrow-allocator)"));
        }
        if let Some(m) = f.strip_prefix("crates/rt/src/").and_then(|m| m.strip_suffix(".rs")).filter(|m| !m.contains('/')) {
            // The runtime crate is WIDE in `affected`; here its module `m` stands for the files of the narrowed
            // crates that name `liella_rt::m` (`start` and the crate root stay FULL: startup).
            if matches!(m, "lib" | "start") {
                plan.full.push(format!("{f}: startup"));
                continue;
            }
            let users = rt_users(root, m);
            if users.is_empty() {
                plan.full.push(format!("{f}: no narrowed module names `liella_rt::{m}`"));
            }
            plan.notes.push(format!("{f}: through its users {}", users.join(", ")));
            through.extend(users);
            continue;
        }
        through.push(f.to_string());
    }
    let sel = affected::select_opts(repo, &through, true);
    plan.full.extend(sel.full.iter().cloned());
    for c in &sel.wide_crates {
        if !NOT_LINKED.contains(&c.as_str()) {
            plan.full.push(format!("crate `{c}` is selected wide by `affected` (cannot narrow it to rows)"));
        }
    }
    if !plan.full.is_empty() {
        return Ok(plan);
    }
    // Reached modules -> exported symbols.
    let module_names: BTreeSet<String> = sel.modules.iter().map(|(_, m)| m.clone()).collect();
    let mut reached: BTreeMap<String, String> = BTreeMap::new();
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut files_rs = Vec::new();
    rust_files(root, "crates/boundary/src", &mut files_rs);
    let mut block_names: BTreeSet<String> = BTreeSet::new();
    for rs in &files_rs {
        let text = fs::read_to_string(root.join(rs)).unwrap_or_default();
        if !text.contains("export!") {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        let blocks = blocks_of(&lines);
        for b in &blocks {
            block_names.extend(b.names.iter().cloned());
        }
        known.extend(blocks.iter().flat_map(|b| b.names.iter().cloned()));
        // Exports made by a file-local macro (`res_query_fns!("GLIBC_2.34", res_query, ...)` at column 0): every
        // identifier argument, reached with the file's module.
        let mut macro_syms = Vec::new();
        for l in &lines {
            if let Some((mac, rest)) = l.split_once("!(") {
                if !l.starts_with(' ') && mac.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && mac != "export" && mac != "macro_rules" {
                    macro_syms.extend(idents(rest).into_iter().filter(|w| !w.starts_with("GLIBC")));
                }
            }
        }
        known.extend(macro_syms.iter().cloned());
        let Some(m) = repo.module_of(rs) else { continue };
        if !sel.modules.contains(&m) {
            continue;
        }
        for s in &macro_syms {
            reached.entry(s.clone()).or_insert_with(|| format!("{rs}: made by a macro of a reached module ({})", m.1));
        }
        let all = |why: String, reached: &mut BTreeMap<String, String>| {
            for b in &blocks {
                for n in &b.names {
                    reached.entry(n.clone()).or_insert_with(|| why.clone());
                }
            }
        };
        if files.iter().any(|f| f == rs) {
            all(format!("{rs} changed"), &mut reached);
            continue;
        }
        match blocks_hit(&lines, &blocks, &module_names) {
            Some(hit) if !hit.is_empty() => {
                for k in hit {
                    for n in &blocks[k].names {
                        reached.entry(n.clone()).or_insert_with(|| format!("{rs}: its export names a reached module ({})", m.1));
                    }
                }
            }
            Some(_) => all(format!("{rs}: module `{}` reached, no export names it (every export of the file)", m.1), &mut reached),
            None => all(format!("{rs}: module `{}` reached outside an export (every export of the file)", m.1), &mut reached),
        }
    }
    // Symbols the blocks do not define: by the module of their file (the code map).
    let codemap = fs::read_to_string(root.join("docs/codemap/symbols.tsv")).unwrap_or_default();
    for l in codemap.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        let (Some(name), Some(at)) = (f.first(), f.get(2)) else { continue };
        known.insert(name.to_string());
        if block_names.contains(*name) {
            continue;
        }
        let path = at.split(':').next().unwrap_or("");
        let hit = files.iter().any(|x| x == path) || repo.module_of(path).is_some_and(|m| sel.modules.contains(&m));
        if hit {
            reached.entry(name.to_string()).or_insert_with(|| format!("{path}: its module is reached (code map)"));
        }
    }
    // The libm C forwarders (`long double` and `_Float128` forms, csys/src/adapter/math_libm.c): `MATH_FWD(expl)`
    // calls the private export `liella_expl_bits`, so the C name is known and is reached when its export is.
    let fwd = fs::read_to_string(root.join("csys/src/adapter/math_libm.c")).unwrap_or_default();
    for l in fwd.lines() {
        let Some((_, rest)) = l.split_once("MATH_FWD(") else { continue };
        let Some((name, body)) = rest.split_once(')') else { continue };
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let Some(target) = idents(body).into_iter().find(|w| w.starts_with("liella_") && known.contains(w)) else { continue };
        known.insert(name.to_string());
        if let Some(why) = reached.get(&target).cloned() {
            reached.entry(name.to_string()).or_insert_with(|| format!("C forwarder of `{target}` ({why})"));
        }
    }
    if alloc_reached {
        for s in ["malloc", "free", "calloc", "realloc", "reallocarray", "posix_memalign", "aligned_alloc", "memalign", "valloc", "pvalloc", "malloc_usable_size", "cfree"] {
            reached.entry(s.to_string()).or_insert_with(|| "the allocator changed".into());
        }
    }
    // Symbols -> rows.
    let mut by_bench: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in &budget {
        by_bench.entry(r[0].clone()).or_default().push(r[1].clone());
    }
    for (bench, rows) in &by_bench {
        if let Some(why) = whole.get(bench) {
            for r in rows {
                plan.rows.insert((bench.clone(), r.clone()), why.clone());
            }
            continue;
        }
        if !bench.starts_with("cbench_") {
            if product_change {
                for r in rows {
                    plan.rows.insert((bench.clone(), r.clone()), "a Rust bench: runs whole on any product source change".into());
                }
            }
            continue;
        }
        let text = fs::read_to_string(root.join(format!("bench/c/{bench}.c"))).map_err(|e| format!("bench/c/{bench}.c: {e}"))?;
        for (r, syms) in row_symbols(&text, rows, &known) {
            let key = (bench.clone(), r.clone());
            match syms {
                None => {
                    plan.unmapped.push(key.clone());
                    if product_change {
                        plan.rows.insert(key, "UNMAPPED row (no MEASURE site names it): runs on any product change".into());
                    }
                }
                Some(s) if s.is_empty() => {
                    plan.unmapped.push(key.clone());
                    if product_change {
                        plan.rows.insert(key, "UNMAPPED row (its site names no exported symbol): runs on any product change".into());
                    }
                }
                Some(s) => {
                    if let Some(sym) = s.iter().find(|x| reached.contains_key(*x)) {
                        plan.rows.insert(key, format!("calls `{sym}`: {}", reached[sym]));
                    }
                }
            }
        }
    }
    plan.notes.push(format!("{} exported symbols reached; {} modules reached", reached.len(), sel.modules.len()));
    Ok(plan)
}

/// The plan's rows plus the sentinels; `None` = every row (FULL). The base a `ratio_to` row names is run by
/// `xtask bench` itself (it may be an output row without a budget line).
pub fn with_extras(root: &Path, plan: &Plan) -> Result<Selected, String> {
    if !plan.full.is_empty() {
        return Ok(None);
    }
    let budget = budget_rows(root)?;
    let mut rows = plan.rows.clone();
    for (b, r) in sentinels(root)? {
        if !budget.iter().any(|x| x[0] == b && x[1] == r) {
            return Err(format!("{SENTINELS}: {b}/{r} is not a row of bench/budgets.tsv"));
        }
        rows.entry((b, r)).or_insert_with(|| "sentinel".into());
    }
    Ok(Some(rows))
}

/// `bench<TAB>row<TAB>reason` lines (`*<TAB>*<TAB>reason` for every row).
pub fn render(plan: &Plan, rows: &Option<BTreeMap<(String, String), String>>) -> String {
    let mut out = String::from("# cargo xtask bench-rows: the rows a change can move (+ sentinels)\n");
    match rows {
        None => {
            for why in &plan.full {
                out.push_str(&format!("*\t*\tFULL: {}\n", why.replace('\t', " ")));
            }
        }
        Some(r) => {
            for ((b, n), why) in r {
                out.push_str(&format!("{b}\t{n}\t{}\n", why.replace('\t', " ")));
            }
        }
    }
    out
}

/// Parse a rows file: `None` = every row.
pub fn parse_rows_file(text: &str) -> Option<BTreeSet<(String, String)>> {
    let mut set = BTreeSet::new();
    for l in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let mut f = l.split('\t');
        let (b, r) = (f.next().unwrap_or(""), f.next().unwrap_or(""));
        if b == "*" {
            return None;
        }
        set.insert((b.to_string(), r.to_string()));
    }
    Some(set)
}

/// The changed files for `--base`, `--range` or `--files`.
pub fn changed(root: &Path, base: &str, range: Option<&str>, files: Option<Vec<String>>) -> Result<Vec<String>, String> {
    if let Some(f) = files {
        return Ok(f);
    }
    if let Some(r) = range {
        let (a, b) = r.split_once("..").ok_or("--range needs <a>..<b>")?;
        let out = std::process::Command::new("git").current_dir(root).args(["diff", "--name-only", a, b]).output().map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(format!("git diff --name-only {a} {b}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        return Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).filter(|l| !l.is_empty()).collect());
    }
    affected::changed_files(root, base)
}

/// The rows file for a targeted bench: `(rows or None = every row, rendered text)`.
pub fn plan_for(root: &Path, files: &[String]) -> Result<(Selected, String, Plan), String> {
    plan_with(root, &Repo::load(root), files)
}

fn plan_with(root: &Path, repo: &Repo, files: &[String]) -> Result<(Selected, String, Plan), String> {
    plan_opts(root, repo, files, std::env::var("BENCH_NARROW_ALLOC").is_ok_and(|v| v == "1"))
}

fn plan_opts(root: &Path, repo: &Repo, files: &[String], narrow_alloc: bool) -> Result<(Selected, String, Plan), String> {
    let plan = select_with(root, repo, files, narrow_alloc)?;
    let rows = with_extras(root, &plan)?;
    let text = render(&plan, &rows);
    Ok((rows, text, plan))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut base = std::env::var("AFFECTED_BASE").ok().filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "origin/main".into());
    let (mut range, mut files, mut out, mut list_all, mut narrow_alloc) = (None, None, None, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => base = it.next().ok_or("--base needs a ref")?.clone(),
            "--range" => range = Some(it.next().ok_or("--range needs <a>..<b>")?.clone()),
            "--files" => files = Some(it.next().ok_or("--files needs a comma list")?.split(',').filter(|f| !f.is_empty()).map(str::to_string).collect()),
            "--out" => out = Some(it.next().ok_or("--out needs a file")?.clone()),
            "--all-rows" => list_all = true,
            "--narrow-allocator" => narrow_alloc = true,
            "-h" | "--help" => {
                println!("usage: cargo xtask bench-rows [--base <ref> | --range <a>..<b> | --files <path>,...] [--out <file>] [--all-rows] [--narrow-allocator]");
                return Ok(());
            }
            other => return Err(format!("bench-rows: unknown argument `{other}`")),
        }
    }
    let files = changed(root, &base, range.as_deref(), files)?;
    let (rows, text, plan) = plan_opts(root, &Repo::load(root), &files, narrow_alloc)?;
    let total = budget_rows(root)?.len();
    for n in &plan.notes {
        println!("note: {n}");
    }
    match &rows {
        None => {
            println!("bench-rows: {} changed files: FULL ({} rows)", files.len(), total);
            for w in &plan.full {
                println!("  FULL: {w}");
            }
        }
        Some(r) => {
            let sentinel = r.values().filter(|w| *w == "sentinel").count();
            let bins: BTreeSet<&str> = r.keys().map(|(b, _)| b.as_str()).collect();
            println!("bench-rows: {} changed files: {} of {total} rows ({} selected + {sentinel} sentinels) in {} binaries; {} unmapped rows", files.len(), r.len(), r.len() - sentinel, bins.len(), plan.unmapped.len());
            for ((b, n), why) in r {
                if list_all || why != "sentinel" {
                    println!("  {b}/{n}: {why}");
                }
            }
        }
    }
    if let Some(o) = out {
        if let Some(d) = Path::new(&o).parent() {
            fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        fs::write(&o, text).map_err(|e| format!("{o}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn plan(files: &[&str]) -> (Selected, Plan) {
        let f: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        static REPO: std::sync::OnceLock<Repo> = std::sync::OnceLock::new();
        let (rows, _, p) = plan_with(&root(), REPO.get_or_init(|| Repo::load(&root())), &f).expect("plan");
        (rows, p)
    }

    #[test]
    fn sites_follow_helpers_and_function_pointers() {
        let src = "#include <string.h>\n#define MEASURE(name, iters, expr) do { put(name, 1.0); } while (0)\n#define ROW(fn, ...) do { MEASURE(name, __VA_ARGS__); } while (0)\n\
static size_t (*len_fn)(const char *) = strlen;\nstatic int pair(int n) { void *p = malloc(n); memset(p, 0, n); free(p); return 0; }\n\
int main(void) { /* MEASURE(\"gone\", 1, memset(0,0,0)) */\n    MEASURE(\"a\", 10, len_fn(\"x\"));\n    MEASURE(\"b\", 10, pair(16));\n    ROW(\"memcpy\", memcpy(d, s, n));\n    return 0; }\n";
        let exported: BTreeSet<String> = ["strlen", "malloc", "free", "memcpy", "memset", "put"].iter().map(|s| s.to_string()).collect();
        let rows: Vec<String> = ["a", "b", "memcpy_64", "gone", "c"].iter().map(|s| s.to_string()).collect();
        let m = row_symbols(src, &rows, &exported);
        let set = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>());
        assert_eq!(m["a"], set(&["strlen"]));
        assert_eq!(m["b"], set(&["free", "malloc"]));
        assert_eq!(m["memcpy_64"], set(&["memcpy"]));
        assert_eq!(m["gone"], None, "a site inside a comment is not a site");
        assert_eq!(m["c"], None);
    }

    #[test]
    fn every_cbench_row_is_mapped() {
        let root = root();
        let budget = budget_rows(&root).expect("budgets");
        let (_, p) = plan(&["crates/boundary/src/hot.rs"]);
        assert!(budget.iter().any(|r| r[0].starts_with("cbench_")));
        assert!(p.unmapped.is_empty(), "rows no MEASURE site names or that call no exported symbol: {:?}", p.unmapped);
    }

    #[test]
    fn sentinels_are_budget_rows() {
        let root = root();
        let s = sentinels(&root).expect("sentinels");
        assert!((15..=30).contains(&s.len()), "{} sentinels", s.len());
        let budget = budget_rows(&root).expect("budgets");
        for (b, r) in s {
            assert!(budget.iter().any(|x| x[0] == b && x[1] == r && x[2] == "ratio_host"), "{b}/{r} is not a ratio_host row");
        }
    }

    #[test]
    fn docs_select_only_the_sentinels_and_build_files_everything() {
        let (rows, _) = plan(&["docs/codemap/README.md", "handoff/x.md", "bench/host_limits.tsv"]);
        let rows = rows.expect("not full");
        assert!(rows.values().all(|w| w == "sentinel"), "{rows:?}");
        let (rows, _) = plan(&["bench/c/cbench_counters.h"]);
        let rows = rows.expect("a header is not FULL");
        assert!(rows.contains_key(&("cbench_hot".into(), "strncmp_eq16".into())) && !rows.keys().any(|(b, _)| b == "cbench_aio"));
        for f in ["Cargo.toml", "crates/boundary/Cargo.toml", "crates/boundary/src/heap.rs", "xtask/src/bench.rs", "csys/include/x.h"] {
            assert!(plan(&[f]).0.is_none(), "{f} must be FULL");
        }
    }

    #[test]
    fn a_bench_source_selects_its_own_rows() {
        let (rows, _) = plan(&["bench/c/cbench_thread.c"]);
        let rows = rows.expect("not full");
        assert!(rows.contains_key(&("cbench_thread".into(), "gettid".into())));
        assert!(!rows.keys().any(|(b, _)| b == "cbench_malloc"));
    }

    #[test]
    fn the_copy_kernel_narrows_and_selects_the_copy_rows() {
        let (rows, _) = plan(&["crates/boundary/src/hot.rs"]);
        let rows = rows.expect("hot.rs must narrow, not go FULL");
        assert!(rows.get(&("cbench_memref".into(), "memcpy_64k".into())).is_some_and(|w| w != "sentinel"), "{rows:?}");
        // No negative row any more: since WL-HOT-MAKE calloc zeroes through hot.rs, and the module-level
        // reach (heap -> s11/s18 exports) then takes in nearly every row, the name service included.
    }

    #[test]
    fn a_test_only_module_is_not_followed() {
        let repo = Repo::load(&root());
        assert!(repo.test_only_has("liella-boundary", "selftest"), "selftest is declared under cfg(feature = \"selftest\")");
        let files = ["crates/boundary/src/hot.rs".to_string()];
        let all = affected::select_opts(&repo, &files, false).modules;
        let prod = affected::select_opts(&repo, &files, true).modules;
        assert!(prod.is_subset(&all) && !prod.iter().any(|(_, m)| m == "selftest"), "{prod:?}");
        assert!(all.iter().any(|(_, m)| m == "selftest"), "the test view still follows it: {all:?}");
    }
}
