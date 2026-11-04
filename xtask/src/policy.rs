//! Mechanical enforcement of the architectural rules (implementation-plan.md §2.1, §2.5, §8.2).
//!
//! These are token-level checks, deliberately strict and simple: they are a tripwire, not a
//! proof. Code review remains the primary control.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Remove comments, string and char literals, keeping line structure.
pub(crate) fn strip(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                if b[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2;
        } else if c == b'r' && (b.get(i + 1) == Some(&b'#') || b.get(i + 1) == Some(&b'"')) && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) != Some(&b'"') {
                out.push('r');
                i += 1;
                continue;
            }
            j += 1;
            loop {
                if j >= b.len() {
                    break;
                }
                if b[j] == b'"' && b[j + 1..].iter().take(hashes).filter(|&&h| h == b'#').count() == hashes {
                    j += 1 + hashes;
                    break;
                }
                if b[j] == b'\n' {
                    out.push('\n');
                }
                j += 1;
            }
            out.push_str("\"\"");
            i = j;
        } else if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                }
                if b.get(i) == Some(&b'\n') {
                    out.push('\n');
                }
                i += 1;
            }
            i += 1;
            out.push_str("\"\"");
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    out
}

fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty())
}

/// The files where the `allocator` category (`unsafe impl GlobalAlloc` and its host calls) may appear.
const ALLOCATOR_FILES: &[&str] = &["src/heap.rs", "src/selftest/failalloc.rs"];
const CATEGORIES: [&str; 6] = ["extern", "export", "slice", "allocator", "tb-field", "hot-kernel"];

/// Whether the (stripped) `unsafe {` block at `lines[n]` is an inline-assembly block: the
/// invocation on that line or within the next two.
fn is_asm_block(lines: &[&str], n: usize) -> bool {
    lines.iter().skip(n).take(3).any(|l| l.contains("asm!"))
}

/// The native Rust facade's one category (D-X.10): its declarations of the native C ABI (and
/// the calls, made safe by `safe fn`), its `GlobalAlloc` over the native heap (with the one
/// copy a move of an over-aligned block needs), the two byte views of the secure heap block
/// its `SecureBlock` owns (D-X.11), the `entry!` macro's unmangled `main`, and
/// for the Rust dylib the unmangled `LIELLA_RUST_ABI` and the one named section
/// [`NATIVE_SECTION`] (its ABI note). It is permitted only in this file of `crates/native`,
/// which is not part of the C library: the facade is a library for programs.
/// Coordinator ruling #406 (2026-10-21, PTHREAD-ONCE-UNWIND-2): category `export` is permitted
/// outside export.rs/data.rs for exactly these unmangled helpers, which csys/src/machine/once.S
/// calls (`liella_once_begin`, `liella_once_end`) or names in its call-frame information
/// (`liella_once_personality`). The category is not widened anywhere else.
const ONCE_HELPER_FILE: &str = "src/posix/s22_threads.rs";
const ONCE_HELPERS: [&str; 3] = ["liella_once_begin", "liella_once_end", "liella_once_personality"];

/// The name of the first `fn` declared within six lines after line `n` (attributes and doc
/// comments may sit between an attribute and its item).
fn next_fn_name<'a>(lines: &[&'a str], n: usize) -> Option<&'a str> {
    lines.iter().skip(n).take(7).find_map(|l| {
        let rest = l.split_once("fn ")?.1;
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
        Some(&rest[..end])
    })
}

const NATIVE_CATEGORY: &str = "native";
const NATIVE_FILE: &str = "src/ffi.rs";
/// The only `#[unsafe(link_section = ...)]` category *native* permits, exactly as written: the
/// Rust dylib's ABI note (D-X.10, spec/abi/rust-abi.toml). Any other section is rejected.
const NATIVE_SECTION: &str = "#[unsafe(link_section = \".note.liella.rustabi\")]";

/// The `UNSAFE(<category>)` tag documenting a site, searched in the `window` source lines
/// above `line` (inclusive).
fn tag_above(src_lines: &[&str], line: usize, window: usize) -> Option<String> {
    if src_lines.is_empty() {
        return None;
    }
    let hi = line.min(src_lines.len() - 1);
    let lo = hi.saturating_sub(window);
    for l in src_lines[lo..=hi].iter().rev() {
        if let Some(i) = l.find("UNSAFE(") {
            let rest = &l[i + 7..];
            if let Some(j) = rest.find(')') {
                return Some(rest[..j].to_string());
            }
        }
    }
    None
}

/// Translation-layer purity: conversions and one call only (implementation-plan.md §8.2).
pub fn purity(root: &Path) -> Result<(), String> {
    const KEYWORDS: &[&str] = &[
        "if", "else", "match", "loop", "while", "for", "let", "return", "break", "continue",
        "static", "struct", "enum", "impl", "trait", "const", "mut", "as", "unreachable",
    ];
    const OPERATORS: &[&str] = &["==", "!=", "&&", "||", "+=", "-=", "*=", "/=", "<=", ">=", " + ", " - ", " / ", " % ", "?"];
    let mut files = Vec::new();
    rust_files(&root.join("crates/boundary/src/posix"), &mut files);
    // The native C ABI's exports follow the same discipline (D-X.10); its helpers (`abi.rs`,
    // `types.rs`) hold no exports, so only export bodies are checked there too.
    rust_files(&root.join("crates/boundary/src/native"), &mut files);
    let mut errors = Vec::new();
    for f in &files {
        let src = fs::read_to_string(f).map_err(|e| e.to_string())?;
        let src_lines: Vec<&str> = src.lines().collect();
        let code = strip(&src);
        // Only export bodies are checked: signatures (types such as `*mut c_void`) and the
        // `export!` framing are not logic. A body runs from the line after the one ending the
        // signature with `{` to the line holding the closing `}` of the export.
        let mut in_sig = false;
        let mut in_body = false;
        for (n, line) in code.lines().enumerate() {
            let l = line.trim();
            if l.starts_with("pub extern \"C\" fn") {
                in_sig = !l.ends_with('{');
                in_body = l.ends_with('{');
                continue;
            }
            if in_sig {
                if l.ends_with('{') {
                    in_sig = false;
                    in_body = true;
                }
                continue;
            }
            if !in_body {
                continue;
            }
            if l == "}" || l == "}}" || l.starts_with("}}") {
                in_body = false;
                continue;
            }
            if l.starts_with("#[") || l.starts_with("#![") || l.starts_with("use ") || l.starts_with("pub use ") || l.starts_with("mod ") || l.starts_with("pub mod ") {
                continue;
            }
            for w in words(l) {
                if KEYWORDS.contains(&w) {
                    errors.push(format!("{}:{}: `{w}` is logic; move it into liella/liella-legacy/liella-abi", f.display(), n + 1));
                }
            }
            for op in OPERATORS {
                if l.contains(op) {
                    errors.push(format!("{}:{}: operator `{}` is logic", f.display(), n + 1, op.trim()));
                }
            }
            let mut rest = l;
            while let Some(i) = rest.find("unsafe {") {
                let after = rest[i + 8..].trim_start();
                if !(after.starts_with("conv::") || after.starts_with("strz::") || after.starts_with("crate::conv::") || after.starts_with("crate::strz::") || after.is_empty()) {
                    errors.push(format!("{}:{}: `unsafe` block must wrap a single conv::/strz:: conversion", f.display(), n + 1));
                }
                if tag_above(&src_lines, n, 2).as_deref() != Some("slice") {
                    errors.push(format!("{}:{}: pointer conversion lacks its `// UNSAFE(slice) SAFETY: ...` comment", f.display(), n + 1));
                }
                rest = &rest[i + 8..];
            }
        }
    }
    report("purity", files.len(), errors)
}

/// The unsafe policy (implementation-plan.md §2.1):
/// * every crate except liella-boundary and liella-native (and xtask) declares
///   `#![forbid(unsafe_code)]` and contains no `unsafe`;
/// * liella-boundary declares `#![deny(unsafe_code)]`; each `unsafe` site belongs to one of the
///   five categories (*tb-field*: the one inline `%fs` load, owner ruling 2026-10-06), sits in a
///   file where that category is permitted, and carries an
///   `// UNSAFE(<category>)` comment (plus clippy's `SAFETY:` text for blocks);
/// * liella-native (D-X.10) declares `#![deny(unsafe_code)]` at its root and
///   `#![forbid(unsafe_code)]` in every other file (modules and examples) except `src/ffi.rs`,
///   the only file with `unsafe`, each site documented as `// UNSAFE(native)`.
/// * no Rust file contains inline assembly except that *tb-field* block in boundary/src/tb.rs.
pub fn unsafe_audit(root: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    let mut checked = 0;
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?
        .flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.push(root.join("xtask"));
    dirs.sort();
    for dir in &dirs {
        let is_boundary = dir.ends_with("boundary");
        let is_native = dir.ends_with("native");
        let lib = if dir.ends_with("xtask") { dir.join("src/main.rs") } else { dir.join("src/lib.rs") };
        if let Ok(text) = fs::read_to_string(&lib) {
            let want = if is_boundary || is_native { "#![deny(unsafe_code)]" } else { "#![forbid(unsafe_code)]" };
            if !text.contains(want) {
                errors.push(format!("{}: missing {want}", lib.display()));
            }
        }
        let mut files = Vec::new();
        rust_files(dir, &mut files);
        if let Ok(build) = fs::read_to_string(dir.join("build.rs")) {
            if words(&strip(&build)).any(|w| w == "unsafe") {
                errors.push(format!("{}: `unsafe` in a build script", dir.join("build.rs").display()));
            }
        }
        for f in &files {
            checked += 1;
            let src = fs::read_to_string(f).map_err(|e| e.to_string())?;
            let src_lines: Vec<&str> = src.lines().collect();
            let code = strip(&src);
            let rel = f.strip_prefix(dir).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if rel == "build.rs" {
                continue;
            }
            // liella-native: every file but the root and the declarations forbids unsafe code
            // itself, so a site added anywhere else fails to compile.
            if is_native && rel != "src/lib.rs" && rel != NATIVE_FILE && !code.lines().any(|l| l.trim() == "#![forbid(unsafe_code)]") {
                errors.push(format!("{}: missing #![forbid(unsafe_code)] (liella-native allows unsafe code only in {NATIVE_FILE})", f.display()));
            }
            let code_lines: Vec<&str> = code.lines().collect();
            for (n, line) in code.lines().enumerate() {
                if !words(line).any(|w| w == "unsafe") {
                    continue;
                }
                if line.contains("forbid(unsafe_code)") || line.contains("deny(unsafe_code)") || line.contains("allow(unsafe_code)") {
                    continue;
                }
                if is_native {
                    let l = line.trim();
                    // A named section only as NATIVE_SECTION (the stripped line has lost the
                    // section's name, so the source line is compared).
                    let section = l == "#[unsafe(link_section = \"\")]" && src_lines.get(n).is_some_and(|s| s.trim() == NATIVE_SECTION);
                    let form = section || ["unsafe extern \"\"", "unsafe impl GlobalAlloc", "unsafe fn ", "unsafe {", "#[unsafe(no_mangle)]"].iter().any(|f| l.starts_with(f));
                    if rel != NATIVE_FILE {
                        errors.push(format!("{}:{}: liella-native permits `unsafe` only in {NATIVE_FILE}", f.display(), n + 1));
                    } else if !form {
                        errors.push(format!("{}:{}: `unsafe` form not in category `{NATIVE_CATEGORY}`", f.display(), n + 1));
                    } else {
                        match tag_above(&src_lines, n, 6) {
                            Some(t) if t == NATIVE_CATEGORY => *counts.entry(t).or_default() += 1,
                            Some(t) => errors.push(format!("{}:{}: site is `{NATIVE_CATEGORY}` but documented as UNSAFE({t})", f.display(), n + 1)),
                            None => errors.push(format!("{}:{}: undocumented unsafe site (needs `// UNSAFE({NATIVE_CATEGORY}): why`)", f.display(), n + 1)),
                        }
                    }
                    continue;
                }
                if !is_boundary {
                    errors.push(format!("{}:{}: `unsafe` outside liella-boundary is forbidden", f.display(), n + 1));
                    continue;
                }
                if rel.starts_with("tests/") {
                    errors.push(format!("{}:{}: tests may not use `unsafe`", f.display(), n + 1));
                    continue;
                }
                let l = line.trim();
                let (cat, allowed_in): (&str, &[&str]) = if l.contains("unsafe extern") {
                    ("extern", &["src/ffi.rs", "src/ffi_sys.rs", "src/ffi_sys_aarch64.rs", "src/caller_alloc.rs"])
                } else if l.contains("unsafe(no_mangle)") || l.contains("unsafe(export_name") {
                    ("export", &["src/export.rs", "src/data.rs"])
                } else if l.contains("unsafe impl") {
                    // ALLOC-FAIL-INJECT-4 (#2101): the test-only `FailAlloc` (`selftest` feature, never
                    // in a product) lives outside every product-compiled file.
                    ("allocator", ALLOCATOR_FILES)
                } else if l.contains("unsafe {") && is_asm_block(&code_lines, n) {
                    // Owner ruling 2026-10-06 (plan S2): the one `%fs`-relative field load.
                    ("tb-field", &["src/tb.rs"])
                } else if (l.contains("unsafe fn") || l.contains("unsafe {")) && rel == "src/hot.rs" {
                    // Owner ruling 2026-10-08 (plan S2, D-4.22): the small copy and fill kernels.
                    ("hot-kernel", &["src/hot.rs"])
                } else if l.contains("unsafe fn") || l.contains("unsafe {") {
                    ("slice", &["src/conv.rs", "src/strz.rs", "src/heap.rs", "src/tb.rs", "src/runtime.rs", "src/posix/", "src/native/", "src/selftest/failalloc.rs"])
                } else {
                    errors.push(format!("{}:{}: `unsafe` form not in any permitted category", f.display(), n + 1));
                    continue;
                };
                let once_helper = cat == "export" && rel == ONCE_HELPER_FILE && next_fn_name(&code_lines, n).is_some_and(|f| ONCE_HELPERS.contains(&f));
                if !once_helper && !allowed_in.iter().any(|a| rel.starts_with(a)) {
                    errors.push(format!("{}:{}: category `{cat}` is not permitted in {rel}", f.display(), n + 1));
                }
                match tag_above(&src_lines, n, 6) {
                    Some(t) if CATEGORIES.contains(&t.as_str()) => {
                        let ok = t == cat || (cat == "slice" && t == "allocator" && ALLOCATOR_FILES.contains(&rel.as_str()));
                        if !ok {
                            errors.push(format!("{}:{}: site is `{cat}` but documented as UNSAFE({t})", f.display(), n + 1));
                        }
                        *counts.entry(t).or_default() += 1;
                    }
                    Some(t) => errors.push(format!("{}:{}: unknown unsafe category `{t}`", f.display(), n + 1)),
                    None => errors.push(format!("{}:{}: undocumented unsafe site (needs `// UNSAFE(<category>): why`)", f.display(), n + 1)),
                }
            }
            // Every `#[allow(unsafe_code)]` must be documented too, with a known category.
            for (n, raw) in code.lines().enumerate() {
                if !raw.contains("allow(unsafe_code)") {
                    continue;
                }
                if is_native {
                    if rel != NATIVE_FILE {
                        errors.push(format!("{}:{}: #[allow(unsafe_code)] outside {NATIVE_FILE}", f.display(), n + 1));
                    } else if tag_above(&src_lines, n, 4).as_deref() != Some(NATIVE_CATEGORY) {
                        errors.push(format!("{}:{}: #[allow(unsafe_code)] without an UNSAFE({NATIVE_CATEGORY}) comment", f.display(), n + 1));
                    }
                    continue;
                }
                if !is_boundary {
                    errors.push(format!("{}:{}: #[allow(unsafe_code)] outside liella-boundary", f.display(), n + 1));
                    continue;
                }
                match tag_above(&src_lines, n, 4) {
                    Some(t) if CATEGORIES.contains(&t.as_str()) => {}
                    _ => errors.push(format!("{}:{}: #[allow(unsafe_code)] without an UNSAFE(<category>) comment", f.display(), n + 1)),
                }
            }
        }
    }
    // Sealed constructors (decision D-X.9): values that stand for kernel-owned memory may be
    // minted only by the `Sys` implementation. A forged `Region` would let safe code unmap or
    // view memory it does not own. Defined in crates/kernel/src; exercised as pure bookkeeping
    // by crates/kernel/tests (which cannot reach a kernel).
    const SEALED: &[&str] = &["__from_kernel", "__kernel_update", "__set_len"];
    const SEALED_OK: &[&str] = &["crates/kernel/src/", "crates/kernel/tests/", "crates/boundary/src/sys.rs"];
    let mut asm_sites = 0usize;
    for dir in &dirs {
        let mut files = Vec::new();
        rust_files(dir, &mut files);
        for f in &files {
            let rel = f.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let code = strip(&fs::read_to_string(f).map_err(|e| e.to_string())?);
            // Inline assembly (plan S2/S3): only the one *tb-field* block in boundary/src/tb.rs;
            // everything else that needs the machine is in csys/.
            let lines: Vec<&str> = code.lines().collect();
            for (n, line) in lines.iter().enumerate() {
                if !line.contains("asm!") {
                    continue;
                }
                let site = rel == "crates/boundary/src/tb.rs" && (n.saturating_sub(2)..=n).any(|k| lines[k].contains("unsafe {"));
                if site {
                    asm_sites += 1;
                } else {
                    errors.push(format!("{rel}:{}: inline assembly outside the tb-field site (plan S2/S3: machine code belongs in csys/)", n + 1));
                }
            }
            if SEALED_OK.iter().any(|ok| rel.starts_with(ok)) {
                continue;
            }
            for (n, line) in code.lines().enumerate() {
                for name in SEALED {
                    if words(line).any(|w| w == *name) {
                        errors.push(format!("{rel}:{}: sealed constructor `{name}` used outside the Sys implementation (D-X.9)", n + 1));
                    }
                }
            }
        }
    }
    if asm_sites > 1 || counts.get("tb-field").copied().unwrap_or(0) > 1 {
        errors.push(format!("{asm_sites} inline-assembly sites: category tb-field permits exactly one (crates/boundary/src/tb.rs `read`)"));
    }
    if errors.is_empty() {
        let summary: Vec<String> = counts.iter().map(|(k, v)| format!("{k}={v}")).collect();
        println!("unsafe-audit: ok ({checked} files; unsafe sites by category: {})", if summary.is_empty() { "none".into() } else { summary.join(", ") });
        Ok(())
    } else {
        report("unsafe-audit", checked, errors)
    }
}

/// The C syscall layer's rules (csys/README.md, implementation-plan.md §2.5).
pub fn csys_audit(root: &Path) -> Result<(), String> {
    const FORBIDDEN: &[&str] = &["if", "else", "switch", "for", "while", "do", "goto", "errno", "case", "default"];
    const INCLUDES: &[&str] = &["\"liella_csys.h\"", "\"liella_sys.h\"", "\"liella_sysnr.h\"", "\"liella_raw.h\"", "<stddef.h>", "<stdint.h>", "<stdarg.h>"];
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out) } else { out.push(p) }
        }
    }
    let mut files = Vec::new();
    walk(&root.join("csys/src"), &mut files);
    walk(&root.join("csys/include"), &mut files);
    files.sort();
    let mut errors = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        match f.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "S" => {
                if !rel.starts_with("csys/src/machine/") {
                    errors.push(format!("{rel}: assembly is permitted only under csys/src/machine/"));
                }
                continue;
            }
            "c" | "h" => {}
            _ => {
                errors.push(format!("{rel}: unexpected file type in the C layer"));
                continue;
            }
        }
        let raw = fs::read_to_string(f).map_err(|e| e.to_string())?;
        for (n, l) in raw.lines().enumerate() {
            if let Some(inc) = l.trim().strip_prefix("#include") {
                if !INCLUDES.contains(&inc.trim()) {
                    errors.push(format!("{rel}:{}: include {} not permitted (freestanding layer)", n + 1, inc.trim()));
                }
            }
        }
        let code = strip(&raw);
        let adapter = rel.starts_with("csys/src/adapter/");
        let mut depth = 0usize;
        let mut statements = 0usize;
        for (n, line) in code.lines().enumerate() {
            let l = line.trim();
            if l.starts_with('#') {
                continue;
            }
            for w in words(l) {
                if FORBIDDEN.contains(&w) {
                    errors.push(format!("{rel}:{}: `{w}` is logic; the C layer holds single-statement wrappers only", n + 1));
                }
            }
            if l.contains('?') {
                errors.push(format!("{rel}:{}: `?:` is logic", n + 1));
            }
            for ch in l.chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        if depth == 1 {
                            statements = 0;
                        }
                    }
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 && statements > 1 && !adapter {
                            errors.push(format!("{rel}:{}: function body has {statements} statements; wrappers are one statement", n + 1));
                        }
                    }
                    ';' if depth == 1 => statements += 1,
                    _ => {}
                }
            }
        }
    }
    report("csys-audit", files.len(), errors)
}

/// Names that write the process environment through the library (or its `environ`).
const ENV_WRITERS: &[&str] = &["setenv", "putenv", "unsetenv", "clearenv"];

/// Calls that run their closure in a child process of its own (a re-executed test or a fork).
fn is_child_runner(name: &str) -> bool {
    name.starts_with("in_child") || name.starts_with("alone")
}

/// `(name, byte offset)` of each `name(` call on a stripped line; `fn name(` definitions and
/// `environ::name(` calls (the legacy functions on a caller-owned `EnvState`, never on the
/// process's `environ`) are left out.
fn call_names(line: &str) -> Vec<(&str, usize)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let before = line[..s].trim_end();
            if b.get(i) == Some(&b'(') && !before.ends_with("fn") && !before.ends_with("environ::") {
                out.push((&line[s..i], s));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// In-process environment writes in test binaries that also start processes (FL-1: the
/// library's `environ` IS the process's, so a `setenv` racing a `cc` spawn from another test
/// thread tears the array the child is started with). A write must run inside an `in_child(..)` /
/// `alone(..)` closure (a process of its own), or carry `// env-ok: <why it cannot race>` on its
/// line or the line above. A helper whose body writes is itself a writer: its callers are checked.
pub fn env_audit(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for e in fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?.flatten() {
        rust_files(&e.path().join("tests"), &mut files);
    }
    files.sort();
    let mut errors = Vec::new();
    let mut scanned = 0;
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else { continue };
        let stripped = strip(&src);
        let spawns = ["Command::new", "current_exe", "posix_spawn", "fork(", "in_child(", "alone("].iter().any(|k| stripped.contains(k));
        if !spawns {
            continue;
        }
        scanned += 1;
        let orig: Vec<&str> = src.lines().collect();
        let lines: Vec<&str> = stripped.lines().collect();
        // Function spans (name, first line, last line), by brace depth.
        let mut spans: Vec<(String, usize, usize)> = Vec::new();
        {
            let mut depth = 0i32;
            let mut open: Vec<(String, usize, i32)> = Vec::new();
            let mut pending: Option<(String, usize)> = None;
            for (n, l) in lines.iter().enumerate() {
                if let Some(p) = l.find("fn ") {
                    let name: String = l[p + 3..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    if !name.is_empty() && (p == 0 || !l.as_bytes()[p - 1].is_ascii_alphanumeric()) {
                        pending = Some((name, n));
                    }
                }
                for c in l.chars() {
                    match c {
                        '{' => {
                            if let Some((nm, st)) = pending.take() {
                                open.push((nm, st, depth));
                            }
                            depth += 1;
                        }
                        '}' => {
                            depth -= 1;
                            if open.last().is_some_and(|o| o.2 == depth) {
                                let (nm, st, _) = open.pop().unwrap();
                                spans.push((nm, st, n));
                            }
                        }
                        ';' if !l.contains('{') => pending = None, // `fn x();` has no body
                        _ => {}
                    }
                }
            }
        }
        // Writers: the base names plus every function whose body calls one (to a fixpoint).
        // The innermost function around a line decides which body a call belongs to.
        let innermost = |n: usize| spans.iter().filter(|s| s.1 <= n && n <= s.2).min_by_key(|s| s.2 - s.1);
        let is_test = |s: &(String, usize, usize)| (s.1.saturating_sub(3)..=s.1).any(|k| orig[k].contains("#[test]"));
        // Child runners: `in_child*`/`alone*`, and any helper that re-executes this binary.
        let runners: Vec<String> = spans.iter().filter(|s| !is_test(s) && (s.1..=s.2).any(|n| lines[n].contains("current_exe"))).map(|s| s.0.clone()).collect();
        let is_runner = |c: &str| is_child_runner(c) || runners.iter().any(|r| r == c);
        // Lines whose environment write lands in a child: inside a runner's closure (or its
        // arguments), or excused by `// env-ok:`.
        let protected: Vec<bool> = {
            let (mut depth, mut open, mut pend, mut par) = (0i32, Vec::<i32>::new(), false, 0i32);
            lines.iter().enumerate().map(|(n, l)| {
                // A runner call, or the branch only the re-executed child takes (`if .. probe() {`).
                let mut start = call_names(l).into_iter().find(|(c, _)| is_runner(c)).map(|(_, p)| p);
                let probe_branch = start.is_none() && l.contains("if ") && l.contains("probe()");
                if probe_branch {
                    start = l.find("if ");
                }
                // A held `env_lock()` guard serialises the whole function against every spawn.
                let locked = innermost(n).is_some_and(|s| (s.1..n).any(|m| call_names(lines[m]).iter().any(|(c, _)| c.starts_with("env_lock"))));
                let inside = locked || !open.is_empty() || pend || start.is_some() || orig[n].contains("env-ok:") || (n > 0 && orig[n - 1].contains("env-ok:"));
                for (k, c) in l.char_indices() {
                    if start == Some(k) {
                        pend = true;
                        par = i32::from(probe_branch);
                    }
                    match c {
                        '(' if pend => par += 1,
                        ')' if pend => {
                            par -= 1;
                            if par <= 0 {
                                pend = false;
                            }
                        }
                        '{' => {
                            if pend {
                                open.push(depth);
                                pend = false;
                            }
                            depth += 1;
                        }
                        '}' => {
                            depth -= 1;
                            if open.last() == Some(&depth) {
                                open.pop();
                            }
                        }
                        _ => {}
                    }
                }
                inside
            }).collect()
        };
        let mut writers: Vec<String> = ENV_WRITERS.iter().map(|s| s.to_string()).collect();
        loop {
            let before = writers.len();
            for n in 0..lines.len() {
                let hit = call_names(lines[n]).iter().any(|(c, _)| writers.iter().any(|w| w == c)) || lines[n].contains("environ.store(");
                if hit && !protected[n] && let Some(s) = innermost(n) && !is_test(s) && !writers.contains(&s.0) && n > s.1 {
                    writers.push(s.0.clone());
                }
            }
            if writers.len() == before {
                break;
            }
        }
        let rel = f.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        for (n, l) in lines.iter().enumerate() {
            // The body of a helper that is itself a writer is not checked (its callers are).
            let in_writer_helper = innermost(n).is_some_and(|s| n > s.1 && !is_test(s) && writers.contains(&s.0));
            let writes = call_names(l).iter().any(|(c, _)| writers.iter().any(|w| w == c)) || l.contains("environ.store(") || l.contains("env::set_var(") || l.contains("env::remove_var(");
            if writes && !protected[n] && !in_writer_helper {
                errors.push(format!("{rel}:{}: in-process environment write in a test binary that spawns processes (run it in in_child/alone, or add `// env-ok: <why>`): {}", n + 1, orig[n].trim()));
            }
        }
    }
    report("env-audit", scanned, errors)
}

pub(crate) fn report(what: &str, files: usize, errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        println!("{what}: ok ({files} files)");
        Ok(())
    } else {
        for e in &errors {
            eprintln!("{e}");
        }
        Err(format!("{} violations", errors.len()))
    }
}

/// Signals whose default action dumps core (signal(7)): a test that expects one of these must not start the core
/// handler for it.
const CORE_SIGNALS: [&str; 21] = [
    "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGIOT", "SIGBUS", "SIGFPE", "SIGSEGV", "SIGXCPU", "SIGXFSZ", "SIGSYS", "3", "4", "5", "6", "7", "8", "11", "24", "25", "31",
];
/// Signals whose default action ends the process without a core dump.
const PLAIN_SIGNALS: [&str; 18] =
    ["SIGKILL", "SIGTERM", "SIGINT", "SIGHUP", "SIGUSR1", "SIGUSR2", "SIGALRM", "SIGPIPE", "SIGVTALRM", "_", "1", "2", "9", "10", "12", "13", "14", "15"];

/// The argument text after each `pat` up to the closing parenthesis, trimmed.
fn args_after<'a>(text: &'a str, pat: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = text[from..].find(pat) {
        let at = from + p + pat.len();
        let end = text[at..].find(')').map_or(text.len(), |e| at + e);
        out.push(text[at..end].trim());
        from = at;
    }
    out
}

/// (expected-crash sites, expect_crash helper calls) in one test source. Rust: a `.signal(), Some(<core signal>)`
/// assertion or a `Signaled(<x>)` with x not a plain (non-dumping) signal, against `expect_crash()` and
/// `in_child_quiet(` calls. C: `EXPECT-SIGNAL: <core signal>` or `WTERMSIG(..) == <core signal>`, against
/// `expect_crash();` calls.
fn crash_counts(src: &str, c: bool) -> (usize, usize) {
    if c {
        let mut sites = src.lines().filter_map(|l| l.split_once("EXPECT-SIGNAL:")).filter(|(_, r)| CORE_SIGNALS.contains(&r.trim().trim_end_matches("*/").trim())).count();
        for l in src.lines().filter(|l| l.contains("WTERMSIG(")) {
            // One site per line: `WTERMSIG(st) == SIGSEGV ? "SIGSEGV" : WTERMSIG(st) == SIGABRT ? ...` names one child.
            sites += usize::from(l.split("== ").skip(1).any(|t| CORE_SIGNALS.iter().any(|s| t.starts_with(s) && !t[s.len()..].starts_with(|ch: char| ch.is_ascii_alphanumeric()))));
        }
        (sites, src.matches("expect_crash();").count())
    } else {
        let text = strip(src);
        let signal = args_after(&text, ".signal(), Some(").iter().filter(|a| CORE_SIGNALS.contains(a)).count();
        let signaled = args_after(&text, "Signaled(").iter().filter(|a| !PLAIN_SIGNALS.contains(a)).count();
        (signal + signaled, text.matches("expect_crash()").count() + text.matches("in_child_quiet(").count())
    }
}

/// CORE-STORM (owner: "we shouldn't be trying to dump core on crashes we are [causing]"): every test that expects a
/// child to die of a core-dumping signal makes that child non-dumpable first, through
/// `liella_boundary::selftest::expect_crash` / `in_child_quiet` (Rust) or `conformance/c/expect_crash.h` (C). With a
/// pipe `core_pattern` the kernel starts the core helper for every such crash, even under `RLIMIT_CORE` 0: a storm of
/// them made a test VM unreachable. Per file, the helper calls must be at least the expected-crash sites (a removed
/// call is flagged where the counts were equal; a file whose sites share one helper call, or that probes crashes it
/// does not assert, says why in a `dump-ok: <why>` comment). Scans `crates/*/tests` (Rust and `oracle/*.c`) and
/// `conformance/c`.
pub fn crash_audit(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    let c_in = |dir: &Path, out: &mut Vec<PathBuf>| {
        if let Ok(rd) = fs::read_dir(dir) {
            out.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "c")));
        }
    };
    for e in fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?.flatten() {
        rust_files(&e.path().join("tests"), &mut files);
        c_in(&e.path().join("tests/oracle"), &mut files);
    }
    c_in(&root.join("conformance/c"), &mut files);
    files.sort();
    let (mut errors, mut total) = (Vec::new(), 0);
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else { continue };
        if src.contains("dump-ok:") {
            continue;
        }
        let (sites, helpers) = crash_counts(&src, f.extension().is_some_and(|x| x == "c"));
        total += sites;
        if helpers < sites {
            errors.push(format!(
                "{}: {sites} expected-crash site(s), {helpers} expect_crash call(s): call the helper in the child right before each deliberate crash (or `dump-ok: <why>`)",
                f.strip_prefix(root).unwrap_or(f).display()
            ));
        }
    }
    if errors.is_empty() {
        println!("crash-audit: ok ({total} expected-crash sites in {} files)", files.len());
        Ok(())
    } else {
        for e in &errors {
            eprintln!("{e}");
        }
        Err(format!("crash-audit: {} file(s) without the expect_crash helper", errors.len()))
    }
}

/// GATE-SPEED-5: the allocator family (and the `_chk` copies) of the boundary crate carry their C names behind the
/// `export` feature, and an optimising build treats a direct call to such a name as the standard function (it deletes
/// an unused `malloc`, folds `calloc(1 << 33, 1 << 33)`, drops a double `free`, assumes `__memcpy_chk` operands valid).
/// A test file under `crates/*/tests` therefore calls these through `liella_boundary::selftest::opaque::<name>`, or
/// carries one `// opaque-ok: <why>` line saying why its direct calls are safe to fold (results used, no UB case).
/// Not covered (labelled limit): `memcpy`/`memmove`/`memset`/`strdup`, too many benign direct uses to ban.
pub fn opaque_audit(root: &Path) -> Result<(), String> {
    const NAMES: [&str; 8] = ["malloc", "calloc", "realloc", "free", "aligned_alloc", "posix_memalign", "__memcpy_chk", "__memset_chk"];
    let mut files = Vec::new();
    for e in fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?.flatten() {
        rust_files(&e.path().join("tests"), &mut files);
    }
    files.sort();
    let mut errors = Vec::new();
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else { continue };
        if src.contains("// opaque-ok:") {
            continue;
        }
        for (n, l) in strip(&src).lines().enumerate() {
            for name in NAMES {
                let pat = format!("::{name}(");
                let mut from = 0;
                while let Some(p) = l[from..].find(&pat) {
                    let at = from + p;
                    from = at + pat.len();
                    let seg: String = l[..at].chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
                    if !["opaque", "libc", "glibc", "sys", "c"].contains(&seg.as_str()) {
                        errors.push(format!("{}:{}: direct call to exported `{name}` (use selftest::opaque::{name}, or `// opaque-ok: <why>` in the file)", f.strip_prefix(root).unwrap_or(f).display(), n + 1));
                    }
                }
            }
        }
    }
    errors.extend(builtin_calls(root, &files));
    if errors.is_empty() {
        println!("opaque-audit: ok ({} test files)", files.len());
        Ok(())
    } else {
        for e in &errors {
            eprintln!("{e}");
        }
        Err(format!("opaque-audit: {} direct call(s)", errors.len()))
    }
}

/// The C library names LLVM's `TargetLibraryInfo` knows (LLVM `TargetLibraryInfo.def`, plus the math names its constant
/// folder evaluates). With feature `export` the boundary crate defines these under their C names, so a test calling one
/// with known arguments may get LLVM's answer instead of Liella's: `labs(-5)` is folded to 5 and a planted bug in `labs`
/// survives (TOOLS-10).
pub const LLVM_BUILTINS: &[&str] = &[
    // integers, bits, ctype
    "abs", "labs", "llabs", "imaxabs", "ffs", "ffsl", "ffsll", "fls", "flsl", "flsll", "isdigit", "isascii", "toascii",
    // strings and memory
    "strlen", "strnlen", "strcmp", "strncmp", "strcasecmp", "strncasecmp", "strcoll", "strxfrm", "strchr", "strrchr",
    "strstr", "strpbrk", "strspn", "strcspn", "strcpy", "strncpy", "stpcpy", "stpncpy", "strcat", "strncat", "strlcpy",
    "strlcat", "strdup", "strndup", "strtok", "strtok_r", "memchr", "memrchr", "memcmp", "bcmp", "bcopy", "bzero",
    "memcpy", "mempcpy", "memmove", "memset", "memccpy", "wcslen", "wmemchr", "wcschr",
    "__strcpy_chk", "__stpcpy_chk", "__strncpy_chk", "__stpncpy_chk", "__strcat_chk", "__strncat_chk", "__strlcpy_chk",
    "__strlcat_chk", "__strlen_chk", "__memmove_chk", "__mempcpy_chk", "__memccpy_chk", "__snprintf_chk",
    "__sprintf_chk", "__vsnprintf_chk", "__vsprintf_chk",
    // conversions
    "atoi", "atol", "atoll", "atof", "strtol", "strtoul", "strtoll", "strtoull", "strtod", "strtof", "strtold",
    // stdio (printf -> puts/putchar/fwrite style rewrites)
    "printf", "sprintf", "snprintf", "fprintf", "vprintf", "vsprintf", "vsnprintf", "vfprintf", "puts", "putchar",
    "putc", "fputc", "fputs", "fwrite", "fputc_unlocked", "fputs_unlocked", "fwrite_unlocked", "putc_unlocked",
    "putchar_unlocked", "fiprintf", "siprintf",
    // math (each with its f and l forms)
    "fabs", "fabsf", "fabsl", "sqrt", "sqrtf", "sqrtl", "floor", "floorf", "floorl", "ceil", "ceilf", "ceill", "trunc",
    "truncf", "truncl", "round", "roundf", "roundl", "roundeven", "roundevenf", "roundevenl", "rint", "rintf", "rintl",
    "nearbyint", "nearbyintf", "nearbyintl", "lround", "lroundf", "lroundl", "llround", "llroundf", "llroundl", "lrint",
    "lrintf", "lrintl", "llrint", "llrintf", "llrintl", "fmin", "fminf", "fminl", "fmax", "fmaxf", "fmaxl", "copysign",
    "copysignf", "copysignl", "fmod", "fmodf", "fmodl", "remainder", "remainderf", "remainderl", "ldexp", "ldexpf",
    "ldexpl", "frexp", "frexpf", "frexpl", "modf", "modff", "modfl", "logb", "logbf", "logbl", "ilogb", "ilogbf",
    "ilogbl", "scalbn", "scalbnf", "scalbnl", "scalbln", "scalblnf", "scalblnl", "nextafter", "nextafterf",
    "nextafterl", "nexttoward", "nexttowardf", "nexttowardl", "exp", "expf", "expl", "exp2", "exp2f", "exp2l", "exp10",
    "exp10f", "exp10l", "expm1", "expm1f", "expm1l", "log", "logf", "logl", "log2", "log2f", "log2l", "log10", "log10f",
    "log10l", "log1p", "log1pf", "log1pl", "pow", "powf", "powl", "cbrt", "cbrtf", "cbrtl", "hypot", "hypotf", "hypotl",
    "sin", "sinf", "sinl", "cos", "cosf", "cosl", "tan", "tanf", "tanl", "sincos", "sincosf", "sincosl", "asin", "asinf",
    "asinl", "acos", "acosf", "acosl", "atan", "atanf", "atanl", "atan2", "atan2f", "atan2l", "sinh", "sinhf", "sinhl",
    "cosh", "coshf", "coshl", "tanh", "tanhf", "tanhl", "asinh", "asinhf", "asinhl", "acosh", "acoshf", "acoshl", "atanh",
    "atanhf", "atanhl", "erf", "erff", "erfl", "fdim", "fdimf", "fdiml", "fma", "fmaf", "fmal", "cabs", "cabsf", "cabsl",
];

/// TOOLS-10 / OPAQUE-AUDIT: a test file of a crate that links `liella_boundary` and calls an [`LLVM_BUILTINS`] name
/// directly (`<path>::labs(`) must be compiled `#![no_builtins]` (then LLVM treats every such call as an unknown
/// function and calls Liella's), call it through `selftest::opaque`, or say `// opaque-ok: <why>`. Calls through
/// `libc::` / `glibc::` / `sys::` (the reference) and Rust's own numeric methods (`f64::sqrt(`) are not ours.
/// Labelled limit: unit tests inside `src/` are not scanned (a library crate cannot be `no_builtins` per test).
fn builtin_calls(root: &Path, files: &[PathBuf]) -> Vec<String> {
    const NOT_OURS: [&str; 19] = ["opaque", "libc", "glibc", "sys", "c", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize"];
    let mut errors = Vec::new();
    for f in files {
        let Ok(src) = fs::read_to_string(f) else { continue };
        if src.contains("// opaque-ok:") || src.contains("#![no_builtins]") || !src.contains("liella_boundary") {
            continue;
        }
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        let mut hits = Vec::new();
        for (n, l) in strip(&src).lines().enumerate() {
            let mut from = 0;
            while let Some(p) = l[from..].find("::") {
                let at = from + p;
                from = at + 2;
                let name: String = l[from..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if !l[from + name.len()..].starts_with('(') || !LLVM_BUILTINS.contains(&name.as_str()) {
                    continue;
                }
                let seg: String = l[..at].chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
                if !NOT_OURS.contains(&seg.as_str()) {
                    hits.push(format!("{}:{name}", n + 1));
                }
            }
        }
        if !hits.is_empty() {
            let shown: Vec<&str> = hits.iter().take(5).map(String::as_str).collect();
            errors.push(format!("{rel}: {} direct call(s) to LLVM-builtin libc names ({}{}) - add `#![no_builtins]` to the file (or call selftest::opaque, or `// opaque-ok: <why>`)", hits.len(), shown.join(", "), if hits.len() > 5 { ", ..." } else { "" }));
        }
    }
    errors
}
