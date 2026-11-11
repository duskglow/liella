//! `cargo xtask so`: build the products (implementation-plan.md §6 phase 4).
//!
//! 1. `crates/libc-so` as a static archive: `target/liella/libc.a`.
//!    Before both products are made from it, the library's own calls to `memcpy`, `memmove`
//!    and `memset` are pointed at its internal entries and made direct (D-X.15, `copies.rs`).
//! 2. The version script: every symbol the archive defines that `spec/abi/versions.tsv` gives a
//!    default version in `libc.so.6`, grouped by version node; then the native C ABI's symbols
//!    (`spec/abi/native.tsv`, D-X.10) in their nodes `LIELLA_NATIVE_<n>`, each of which the
//!    archive must define; everything else is local (the C layer's `liella_sys_*` and the other
//!    internal `liella_*` names among it). Compat versions (`sym@VER` without `@@`) come from
//!    the generated `.symver` stubs of `csys/src/machine/compat_aliases.S` (`compat.rs`, LG-1),
//!    assembled here and linked into libc.so.6 only (R15-2); each is checked as exported.
//! 3. The shared object, linked from that archive with the script and soname `libc.so.6`:
//!    `target/liella/libc.so.6`. Its dynamic symbol table is then checked: every native symbol
//!    is exported as `name@@LIELLA_NATIVE_<n>`, and no other `liella_` name is exported.
//! 4. The start object from `csys/src/machine/entry.S`: `target/liella/liella-crt1.o`, and from the
//!    same file the static-PIE start object `target/liella/liella-rcrt1.o` (D-27.10); the profiling
//!    start object for `-pg` programs from `csys/start/gcrt1.S`: `target/liella/liella-gcrt1.o`.
//! 5. Placeholders (`placeholders.rs`, D-27.9): `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `libutil.so.1`,
//!    `libanl.so.1`, `libc_malloc_debug.so.0` from versions.tsv, their empty archives, and an
//!    empty `libresolv.a` (D-16.6: libresolv's symbols are in `libc.a`).
//!
//! 7. The libm products (`run_libm`, D-10.7; compat versions from `crates/libm-so/compat_aliases.S`, LG-4): `target/liella/libm.a` (one object of the math exports,
//!    everything else local) and `libm.so.6` (linked against `libc.so.6`; it cannot be loaded until
//!    phase 14), both checked with `nm`/`readelf`.
//!
//! One product build runs per tree at a time: `run_so_as` takes an exclusive `flock` on
//! `target/product/.lock` (a second `cargo xtask so` says so and waits). Every product is written
//! under a temporary name (`.<name>.tmp`) and renamed into place, so a reader never sees a
//! half-written archive, and the archives are checked (every member is ELF) before they are
//! published (XTASK-SO-ATOMIC; two concurrent builds had left `libc.a` with zeroed members).
//!
//! The product crate lives outside the workspace (a `no_std` cdylib), so it builds in its own
//! target directory, `target/product`. The products go to `target/liella`, never to a directory
//! Cargo puts on `LD_LIBRARY_PATH` (`target/release`): a `libc.so.6` there would replace the
//! host's C library for every build script and test Cargo runs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

pub(crate) fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

/// The exclusive lock of one product directory (`<product>/.lock`), held until dropped. Two
/// product builds in one tree used to post-process the same archive at once and left `libc.a`
/// with zeroed members (XTASK-SO-ATOMIC); the second now waits for the first.
pub(crate) struct ProductLock(#[allow(dead_code)] fs::File); // held for its lifetime: closing the file releases the lock

/// Take the exclusive `flock` of `<product>/.lock`, saying so when it has to wait.
// xtask is host tooling built with the pinned stable toolchain (rust-toolchain.toml), not the
// MSRV of the library crates; `File::lock` needs 1.89.
#[allow(clippy::incompatible_msrv)]
pub(crate) fn lock_product_dir(product: &Path) -> Result<ProductLock, String> {
    fs::create_dir_all(product).map_err(|e| format!("{}: {e}", product.display()))?;
    let path = product.join(".lock");
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            eprintln!("so: another product build holds {}; waiting for it to finish", path.display());
            file.lock().map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Err(fs::TryLockError::Error(e)) => return Err(format!("{}: {e}", path.display())),
    }
    Ok(ProductLock(file))
}

/// The temporary name a product is written under before [`publish`] renames it into place
/// (same directory, so the rename is atomic): `<dir>/.<name>.tmp`. The product lock is held
/// while it exists, so one fixed name per product is enough and nothing piles up.
pub(crate) fn tmp_name(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.tmp"))
}

/// Rename the finished `tmp` over `dst`: a reader sees the old product or the new one, never a
/// half-written file.
pub(crate) fn publish(tmp: &Path, dst: &Path) -> Result<(), String> {
    fs::rename(tmp, dst).map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), dst.display()))
}

/// Every member of the `ar` archive `path` must be an ELF object (`\x7fELF`). The symbol-table
/// and long-name members are skipped. A zeroed member means the archive was written to while it
/// was being rebuilt (two product builds at once); it is reported here, not as a later
/// `readelf: wrong magic bytes`.
fn check_archive_elf(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    archive_elf_errors(&data).map_err(|e| format!("{}: {e}", path.display()))
}

fn archive_elf_errors(data: &[u8]) -> Result<(), String> {
    if !data.starts_with(b"!<arch>\n") {
        return Err("not an ar archive (bad magic); the product build was interrupted or corrupted - was another build running in this tree?".into());
    }
    let (mut pos, mut members, mut bad, mut first) = (8usize, 0usize, 0usize, String::new());
    while pos + 60 <= data.len() {
        let header = &data[pos..pos + 60];
        let name = String::from_utf8_lossy(&header[..16]).trim_end().to_string();
        let size = String::from_utf8_lossy(&header[48..58]).trim().parse::<usize>().map_err(|_| format!("bad member header at byte {pos} (size field not a number)"))?;
        let body = pos + 60;
        if &header[58..60] != b"`\n" || body + size > data.len() {
            return Err(format!("bad member header at byte {pos}: the archive is truncated or corrupt - was another build running in this tree?"));
        }
        if !matches!(name.as_str(), "/" | "//" | "/SYM64/" | "__.SYMDEF" | "__.SYMDEF SORTED") {
            members += 1;
            if !data[body..body + size].starts_with(b"\x7fELF") {
                bad += 1;
                if first.is_empty() {
                    first = name;
                }
            }
        }
        pos = body + size + (size & 1);
    }
    if bad > 0 {
        return Err(format!("{bad} of {members} members are not ELF objects (first: member {first:?}); the archive was corrupted, most likely by two product builds running in one tree at once"));
    }
    Ok(())
}

/// The function the static-PIE start calls before the image is relocated (D-27.10), and the
/// linker-defined symbol it must reach PC-relatively.
const BOOT_ANCHOR: &str = "liella_link_image_header";
const BOOT_ANCHOR_TARGET: &str = "__ehdr_start";

/// D-27.10: the archive member defining [`BOOT_ANCHOR`] (`csys/src/adapter/image_anchor.c`, alone
/// in its file) references [`BOOT_ANCHOR_TARGET`] and has no GOT-typed relocation. A static PIE
/// runs that function before its GOT is relocated: a GOT-indirect read there returns the
/// link-time address (a weak declaration of the symbol makes the compiler emit one).
fn boot_anchor_pc_relative(archive: &Path) -> Result<(), String> {
    let out = Command::new("readelf").args(["-rsW"]).arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf -rs {} failed", archive.display()));
    }
    boot_anchor_errors(&String::from_utf8_lossy(&out.stdout)).map_err(|e| format!("{}: {e}", archive.display()))
}

/// The check of [`boot_anchor_pc_relative`] on `readelf -rsW` text of an archive (one
/// `File: archive(member)` block per member).
pub fn boot_anchor_errors(readelf: &str) -> Result<(), String> {
    let defines = |l: &str| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() == 8 && f[0].ends_with(':') && f[3] == "FUNC" && f[6] != "UND" && f[7] == BOOT_ANCHOR
    };
    let Some(member) = readelf.split("\nFile: ").find(|m| m.lines().any(defines)) else {
        return Err(format!("no member defines {BOOT_ANCHOR} (D-27.10: the static-PIE start's anchor)"));
    };
    let name = member.lines().next().unwrap_or("?");
    // A relocation line: offset, info, type, value, name (+ addend).
    let relocs = member.lines().filter(|l| l.split_whitespace().nth(2).is_some_and(|t| t.starts_with("R_")));
    let got: Vec<&str> = relocs.clone().filter(|l| l.split_whitespace().nth(2).is_some_and(|t| t.contains("GOT"))).collect();
    if let Some(first) = got.first() {
        return Err(format!(
            "{name}: {BOOT_ANCHOR} is reached through the GOT ({} GOT-typed relocations; first: {}): a static PIE reads it before relocation (D-27.10); declare {BOOT_ANCHOR_TARGET} hidden and not weak",
            got.len(),
            first.trim()
        ));
    }
    if !relocs.clone().any(|l| l.split_whitespace().nth(4) == Some(BOOT_ANCHOR_TARGET)) {
        return Err(format!("{name}: {BOOT_ANCHOR} does not reference {BOOT_ANCHOR_TARGET} (D-27.10)"));
    }
    Ok(())
}

/// D-27.10: the static-PIE start object (`liella-rcrt1.o`) runs before the executable is
/// relocated, so it has no GOT-typed relocation (a GOT slot still holds a link-time address).
fn start_object_pc_relative(object: &Path) -> Result<(), String> {
    let out = Command::new("readelf").args(["-rW"]).arg(object).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf -r {} failed", object.display()));
    }
    start_object_errors(&String::from_utf8_lossy(&out.stdout)).map_err(|e| format!("{}: {e}", object.display()))
}

/// The check of [`start_object_pc_relative`] on `readelf -rW` text of one object: no relocation
/// whose type names the GOT, and the call of the relocating start is there.
pub fn start_object_errors(readelf: &str) -> Result<(), String> {
    let relocs: Vec<&str> = readelf.lines().filter(|l| l.split_whitespace().nth(2).is_some_and(|t| t.starts_with("R_"))).collect();
    if let Some(first) = relocs.iter().find(|l| l.split_whitespace().nth(2).is_some_and(|t| t.contains("GOT"))) {
        return Err(format!("the static-PIE start object reaches a symbol through the GOT ({}): it runs before relocation (D-27.10)", first.trim()));
    }
    if !relocs.iter().any(|l| l.split_whitespace().nth(4) == Some(STATIC_PIE_START)) {
        return Err(format!("the static-PIE start object does not call {STATIC_PIE_START} (D-27.10)"));
    }
    Ok(())
}

/// The relocating start that `liella-rcrt1.o`'s `_start` calls (D-27.10).
const STATIC_PIE_START: &str = "liella_start_main_static_pie";

/// Symbols defined (globally) in an archive or object, by `nm`.
fn defined(path: &Path) -> Result<BTreeSet<String>, String> {
    let out = Command::new("nm").args(["-g", "--defined-only", "-P"]).arg(path).output().map_err(|e| format!("nm: {e}"))?;
    if !out.status.success() {
        return Err(format!("nm {} failed", path.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_whitespace().next()).filter(|s| !s.ends_with(':')).map(str::to_string).collect())
}

/// Every generated compat version exported by `so` as `name@VERSION`, and no compat version that
/// `spec/abi/versions.tsv` lacks for `lib` (LG-1; `libm.so.6`: LG-4). The number of compat versions.
fn check_compat_exports(root: &Path, so: &Path, lib: &str, compat: &[crate::compat::Entry]) -> Result<usize, String> {
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf {} failed", so.display()));
    }
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let rows: Vec<(String, String)> = table
        .lines()
        .skip(1)
        .filter_map(|l| match l.split('\t').collect::<Vec<_>>().as_slice() {
            [l, sym, ver, "compat", ..] if *l == lib => Some(((*sym).to_string(), (*ver).to_string())),
            _ => None,
        })
        .collect();
    let problems = crate::compat::export_problems(lib, &String::from_utf8_lossy(&out.stdout), compat, &rows);
    if problems.is_empty() { Ok(compat.len()) } else { Err(format!("compat versions of {lib}:\n  {}", problems.join("\n  "))) }
}

/// The version script for the symbols in `have` (`libc.so.6`, with the native C ABI's nodes).
pub fn version_script(root: &Path, have: &BTreeSet<String>) -> Result<(String, usize), String> {
    lib_version_script(root, have, "libc.so.6", true)
}

/// The version script of library `lib_name` for the symbols in `have`: their default versions in
/// `spec/abi/versions.tsv`, then (with `native_nodes`) the native C ABI's nodes.
fn lib_version_script(root: &Path, have: &BTreeSet<String>, lib_name: &str, native_nodes: bool) -> Result<(String, usize), String> {
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let mut nodes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut compat_nodes: BTreeSet<String> = BTreeSet::new();
    for line in table.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [lib, sym, ver, binding, ..] = f.as_slice() {
            if *lib == lib_name && *binding == "default" && have.contains(*sym) {
                nodes.entry((*ver).to_string()).or_default().insert((*sym).to_string());
            }
            // A compat version exists when the inputs define the `.symver` alias `sym@VER`
            // itself (single `@`: a non-default version, D-27.4; `compat.rs`). Its node must
            // exist; a compat-only name (no default definition) is also listed in it. A name
            // that has a default version is not: listing it twice would move the default.
            if *lib == lib_name && *binding == "compat" && have.contains(&format!("{sym}@{ver}")) {
                let node = nodes.entry((*ver).to_string()).or_default();
                compat_nodes.insert((*ver).to_string());
                if !have.contains(*sym) {
                    node.insert((*sym).to_string());
                }
            }
        }
    }
    // Node order: GLIBC_2.2.5 < GLIBC_2.3 < ... numerically, GLIBC_PRIVATE last.
    let key = |v: &str| -> (u32, Vec<u32>) {
        match v.strip_prefix("GLIBC_") {
            Some("PRIVATE") | None => (1, Vec::new()),
            Some(n) => (0, n.split('.').map(|p| p.parse().unwrap_or(0)).collect()),
        }
    };
    let mut order: Vec<&String> = nodes.keys().collect();
    order.sort_by_key(|v| key(v));
    // `local: *` goes in the first node that holds no compat alias: ld hides a `sym@VER` alias
    // whose own node's `local: *` matches its base name (checked with GNU ld 2.42, LG-1).
    let local_node = order.iter().position(|v| !compat_nodes.contains(*v)).ok_or_else(|| format!("{lib_name}: every version node holds a compat alias; no node can carry `local: *`"))?;
    let mut script = String::from("/* @generated by `cargo xtask so` from spec/abi/versions.tsv and spec/abi/native.tsv. */\n");
    let mut count = 0;
    let mut prev: Option<&String> = None;
    for (i, v) in order.iter().enumerate() {
        // A node that only carries compat aliases lists nothing: `global:` with no names is a
        // syntax error to ld.
        script.push_str(&format!("{v} {{\n"));
        if !nodes[*v].is_empty() {
            script.push_str("  global:\n");
        }
        for s in &nodes[*v] {
            script.push_str(&format!("    {s};\n"));
            // The count is of default versions (`name@@VER`); a compat-only name listed for its
            // `.symver` alias (LG-1, LG-4) exports `name@VER` and is checked on its own.
            if have.contains(s) {
                count += 1;
            }
        }
        if i == local_node {
            script.push_str("  local:\n    *;\n");
        }
        match prev {
            Some(p) => script.push_str(&format!("}} {p};\n")),
            None => script.push_str("};\n"),
        }
        prev = Some(v);
    }
    if !native_nodes {
        return Ok((script, count));
    }
    // The native C ABI (D-X.10): its own nodes, from its own table; each node extends the
    // previous one.
    let mut native: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (sym, node) in crate::native::symbols(root)? {
        if !have.contains(&sym) {
            return Err(format!("libc.a does not define the native symbol {sym} ({node})"));
        }
        native.entry(node).or_default().insert(sym);
    }
    let mut prev: Option<String> = None;
    for node in crate::native::node_order(&native.keys().cloned().collect()) {
        script.push_str(&format!("{node} {{\n  global:\n"));
        for s in &native[&node] {
            script.push_str(&format!("    {s};\n"));
            count += 1;
        }
        match &prev {
            Some(p) => script.push_str(&format!("}} {p};\n")),
            None => script.push_str("};\n"),
        }
        prev = Some(node);
    }
    // The private start interface with Liella's `ld.so` (design §14, R14-12, D-27.22): its own node.
    script.push_str("LIELLA_PRIVATE {\n  global:\n");
    for s in LIBC_PRIVATE {
        if !have.contains(*s) {
            return Err(format!("libc.a does not define the private symbol {s} (LIELLA_PRIVATE)"));
        }
        script.push_str(&format!("    {s};\n"));
        count += 1;
    }
    script.push_str("};\n");
    Ok((script, count))
}

/// `libc.so.6`'s exports at `LIELLA_PRIVATE` (design §14, D-27.22): what Liella's `ld.so` calls or reads.
const LIBC_PRIVATE: &[&str] = &["__liella_build_tag", "__liella_libc_early_init"];

/// The build tag `ld.so` and `libc.so.6` both carry (L14-10, D-27.22, `liella_ld::start::private::TAG`):
/// FNV-1a 64 over the product sources (`crates/`, `csys/`, `include/`, `Cargo.lock`; paths sorted,
/// each path then its bytes), so products built from different sources refuse each other, in a git
/// checkout or a synced copy alike. About 13 MB read per run.
fn build_tag(root: &Path) -> String {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target") {
                    walk(&p, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut files = vec![root.join("Cargo.lock")];
    for d in ["crates", "csys", "include"] {
        walk(&root.join(d), &mut files);
    }
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for f in &files {
        eat(f.strip_prefix(root).unwrap_or(f).to_string_lossy().as_bytes());
        eat(&fs::read(f).unwrap_or_default());
    }
    format!("src-{h:016x}")
}

/// Check `libc.so.6`'s dynamic symbol table: every native symbol at its node as the default
/// version (`name@@NODE`), and no other `liella_` name exported. The number of native symbols.
fn check_native_exports(root: &Path, so: &Path) -> Result<usize, String> {
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf {} failed", so.display()));
    }
    let mut exported: BTreeMap<String, String> = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (Some(sym), Some(ndx)) = (f.get(7), f.get(6)) {
            if *ndx != "UND" {
                if let Some((name, ver)) = sym.split_once('@') {
                    exported.insert(name.to_string(), ver.to_string());
                }
            }
        }
    }
    let want: BTreeMap<String, String> = crate::native::symbols(root)?.into_iter().collect();
    let mut errors = Vec::new();
    for (sym, node) in &want {
        match exported.get(sym) {
            Some(v) if *v == format!("@{node}") => {}
            Some(v) => errors.push(format!("{sym}@{v} in libc.so.6, expected {sym}@@{node}")),
            None => errors.push(format!("{sym} is not exported by libc.so.6")),
        }
    }
    for sym in exported.keys().filter(|s| s.starts_with("liella_") && !want.contains_key(*s)) {
        errors.push(format!("{sym}: exported by libc.so.6 but not a native symbol (spec/abi/native.tsv)"));
    }
    if errors.is_empty() { Ok(want.len()) } else { Err(errors.join("\n")) }
}

/// glibc's binding of every `libc.so.6` export, from the `scope` column of `spec/abi/versions.tsv`
/// (WEAK or GLOBAL; the reference system's `readelf --dyn-syms`): `(symbol, version) -> scope`.
pub fn libc_scopes(root: &Path) -> Result<BTreeMap<(String, String), String>, String> {
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let mut m = BTreeMap::new();
    for line in table.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [lib, sym, ver, _binding, _ty, scope, ..] = f.as_slice() {
            if *lib == "libc.so.6" && (*scope == "WEAK" || *scope == "GLOBAL") {
                m.insert(((*sym).to_string(), (*ver).to_string()), (*scope).to_string());
            }
        }
    }
    Ok(m)
}

/// The names glibc exports WEAK from `libc.so.6` (D-27.4 binding). A name whose versions disagree
/// cannot be satisfied by weakening one definition: an error.
pub fn weak_names(scopes: &BTreeMap<(String, String), String>) -> Result<BTreeSet<String>, String> {
    let mut by_name: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for ((sym, _), scope) in scopes {
        by_name.entry(sym).or_default().insert(scope);
    }
    let mixed: Vec<&str> = by_name.iter().filter(|(_, s)| s.len() > 1).map(|(n, _)| *n).collect();
    if !mixed.is_empty() {
        return Err(format!("versions.tsv: symbols with both WEAK and GLOBAL versions in libc.so.6: {}", mixed.join(" ")));
    }
    Ok(by_name.into_iter().filter(|(_, s)| s.contains("WEAK")).map(|(n, _)| n.to_string()).collect())
}

/// Compare the binding of the dynamic symbols in `readelf --dyn-syms -W` text with `scopes`; a
/// message per symbol (with a row in the table) whose binding differs, and the number checked.
pub fn binding_errors(readelf: &str, scopes: &BTreeMap<(String, String), String>) -> (Vec<String>, usize) {
    let (mut errors, mut checked) = (Vec::new(), 0);
    for line in readelf.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Num: Value Size Type Bind Vis Ndx Name@Version
        if f.len() < 8 || !f[0].ends_with(':') || f[6] == "UND" || f[6] == "ABS" {
            continue;
        }
        let Some((name, ver)) = f[7].split_once('@') else { continue };
        let ver = ver.trim_start_matches('@');
        if let Some(want) = scopes.get(&(name.to_string(), ver.to_string())) {
            checked += 1;
            if f[4] != want {
                errors.push(format!("{name}@{ver}: {} in libc.so.6, glibc has {want}", f[4]));
            }
        }
    }
    (errors, checked)
}

/// Check `libc.so.6`'s symbol binding against glibc's (owner ruling 2026-10-10, WEAK-EXPORTS).
fn check_binding(root: &Path, so: &Path) -> Result<usize, String> {
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf {} failed", so.display()));
    }
    let (errors, checked) = binding_errors(&String::from_utf8_lossy(&out.stdout), &libc_scopes(root)?);
    if errors.is_empty() { Ok(checked) } else { Err(format!("binding differs from glibc's (spec/abi/versions.tsv scope):\n{}", errors.join("\n"))) }
}

/// Every version of every name library `lib` exports in `spec/abi/versions.tsv`:
/// `name -> version -> default?` (`binding` column: `default` is `name@@VER`, `compat` is `name@VER`).
pub fn table_versions(root: &Path, lib: &str) -> Result<BTreeMap<String, BTreeMap<String, bool>>, String> {
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let mut m: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for line in table.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [l, sym, ver, binding, ..] = f.as_slice() {
            if *l == lib && (*binding == "default" || *binding == "compat") {
                m.entry((*sym).to_string()).or_default().insert((*ver).to_string(), *binding == "default");
            }
        }
    }
    Ok(m)
}

/// The defined, versioned dynamic symbols in `readelf --dyn-syms -W` text:
/// `name -> version -> default?` (`name@@VER` is the default, `name@VER` a non-default version).
pub fn exported_versions(readelf: &str) -> BTreeMap<String, BTreeMap<String, bool>> {
    let mut m: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for line in readelf.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Num: Value Size Type Bind Vis Ndx Name@Version
        if f.len() < 8 || !f[0].ends_with(':') || f[6] == "UND" || f[6] == "ABS" {
            continue;
        }
        let Some((name, ver)) = f[7].split_once('@') else { continue };
        let default = ver.starts_with('@');
        m.entry(name.to_string()).or_default().insert(ver.trim_start_matches('@').to_string(), default);
    }
    m
}

/// Compare the exported version set of every name with the table, exactly: each name the library
/// exports, and each name the archive (`have`) gives a version the table lists, must have exactly
/// the table's versions, the table's default (`@@`) and no other. Names in `skip` (the native C
/// ABI, checked by [`check_native_exports`]) are not compared. One message per name, and the
/// number of names checked.
pub fn version_set_errors(
    lib: &str,
    exported: &BTreeMap<String, BTreeMap<String, bool>>,
    table: &BTreeMap<String, BTreeMap<String, bool>>,
    have: &BTreeSet<String>,
    skip: &BTreeSet<String>,
    known: &BTreeSet<(String, String)>,
) -> (Vec<String>, usize) {
    let mut names: BTreeSet<&String> = exported.keys().collect();
    for (name, vers) in table {
        // A compat-only name is always compared, so one dropped from the build entirely (its
        // `.symver` lost) still fails unless compat-missing.tsv lists it.
        let compat_only = vers.values().all(|d| !*d);
        if compat_only || (have.contains(name) && vers.values().any(|d| *d)) || vers.iter().any(|(v, d)| !*d && have.contains(&format!("{name}@{v}"))) {
            names.insert(name);
        }
    }
    let show = |n: &str, v: &str, d: bool| format!("{n}{}{v}", if d { "@@" } else { "@" });
    let none = BTreeMap::new();
    let (mut errors, mut checked) = (Vec::new(), 0);
    for name in names.into_iter().filter(|n| !skip.contains(*n)) {
        checked += 1;
        let got = exported.get(name).unwrap_or(&none);
        let Some(want) = table.get(name) else {
            let vs: Vec<String> = got.iter().map(|(v, d)| show(name, v, *d)).collect();
            errors.push(format!("{name}: exported by {lib} as {} but has no {lib} row in versions.tsv", vs.join(" ")));
            continue;
        };
        let mut what = Vec::new();
        for (v, d) in want {
            match got.get(v) {
                None if *d => what.push(format!("default {} missing", show(name, v, true))),
                None if known.contains(&(name.clone(), v.clone())) => {}
                None => what.push(format!("compat {} missing", show(name, v, false))),
                Some(g) if g == d => {}
                Some(_) if *d => what.push(format!("lost its default: {} exported, versions.tsv has {}", show(name, v, false), show(name, v, true))),
                Some(_) => what.push(format!("gained a default: {} exported, versions.tsv has only {}", show(name, v, true), show(name, v, false))),
            }
        }
        for (v, d) in got.iter().filter(|(v, _)| !want.contains_key(*v)) {
            what.push(format!("extra version {} (not in versions.tsv)", show(name, v, *d)));
        }
        if !what.is_empty() {
            errors.push(format!("{name}: {}", what.join("; ")));
        }
    }
    (errors, checked)
}

/// The list of compat versions library `lib` does not export yet (binary-compatibility phase 15):
/// `spec/abi/compat-missing.tsv` for libc.so.6 (ruling #1176), `spec/abi/compat-missing-libm.tsv`
/// for libm.so.6 (ruling #1187). Any other library has no list, so every gap fails.
pub fn compat_missing_list(lib: &str) -> Option<&'static str> {
    match lib {
        "libc.so.6" => Some("spec/abi/compat-missing.tsv"),
        "libm.so.6" => Some("spec/abi/compat-missing-libm.tsv"),
        _ => None,
    }
}

/// The ratchet on a compat-missing list (`list`, rulings #1176 and #1187): the list may only
/// shrink. A listed `(name, version)` that is now exported fails ("remove it"), and so does one
/// that is not a compat (non-default) row of versions.tsv, so the list cannot hide a default or a
/// stray name.
pub fn stale_compat_missing(
    list: &str,
    exported: &BTreeMap<String, BTreeMap<String, bool>>,
    table: &BTreeMap<String, BTreeMap<String, bool>>,
    known: &BTreeSet<(String, String)>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (s, v) in known {
        if exported.get(s).is_some_and(|m| m.contains_key(v)) {
            errors.push(format!("{s}@{v}: exported, remove it from {list}"));
        } else if table.get(s).and_then(|m| m.get(v)) != Some(&false) {
            errors.push(format!("{s}@{v}: listed in {list} but not a compat version in versions.tsv"));
        }
    }
    errors
}

/// Check the exact version set and default version of every export of `so` (library `lib`)
/// against `spec/abi/versions.tsv` (SO-CHECK-DEFAULT-VERSION). The number of names checked.
fn check_version_sets(root: &Path, so: &Path, lib: &str, have: &BTreeSet<String>, skip: &BTreeSet<String>) -> Result<usize, String> {
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(so).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf {} failed", so.display()));
    }
    let exported = exported_versions(&String::from_utf8_lossy(&out.stdout));
    // The compat versions not built yet (the library's compat-missing list): a listed one that
    // is exported makes the list stale, an error too, so it stays exact.
    let mut known = BTreeSet::new();
    let list = compat_missing_list(lib);
    if let Some(list) = list {
        let text = fs::read_to_string(root.join(list)).map_err(|e| format!("{list}: {e}"))?;
        for line in text.lines().filter(|l| !l.starts_with('#')).skip(1) {
            if let Some((s, v)) = line.split_once('\t') {
                known.insert((s.to_string(), v.to_string()));
            }
        }
    }
    let table = table_versions(root, lib)?;
    let (mut errors, checked) = version_set_errors(lib, &exported, &table, have, skip, &known);
    if let Some(list) = list {
        errors.extend(stale_compat_missing(list, &exported, &table, &known));
    }
    if errors.is_empty() {
        Ok(checked)
    } else {
        Err(format!("{lib}: {} names whose exported versions differ from spec/abi/versions.tsv:\n{}", errors.len(), errors.join("\n")))
    }
}

/// A copy of `src` in `dst` whose definitions of `names` are weak (`libc.so.6` is linked from it, so its
/// export binding matches glibc's). `libc.a` is weakened wholesale by [`weaken_archive`] instead.
fn weaken_names_in_archive(src: &Path, work: &Path, dst: &Path, names: &BTreeSet<String>) -> Result<(), String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let ar = std::env::var("AR").unwrap_or_else(|_| "ar".into());
    let objcopy = std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into());
    let src = fs::canonicalize(src).map_err(|e| e.to_string())?;
    let listed = Command::new(&ar).arg("t").arg(&src).output().map_err(|e| format!("ar t: {e}"))?;
    let members: Vec<String> = String::from_utf8_lossy(&listed.stdout).lines().map(str::to_string).collect();
    if members.iter().collect::<BTreeSet<_>>().len() != members.len() {
        return Err(format!("{}: duplicate member names; cannot extract them safely", src.display()));
    }
    run(Command::new(&ar).current_dir(work).arg("x").arg(&src))?;
    let list = work.join("weak.names");
    fs::write(&list, names.iter().map(|n| format!("{n}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    for m in &members {
        run(Command::new(&objcopy).arg(format!("--weaken-symbols={}", list.display())).arg(work.join(m)))?;
    }
    let _ = fs::remove_file(dst);
    let mut pack = Command::new(&ar);
    pack.current_dir(work).arg("rcsD").arg(fs::canonicalize(dst.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?.join(dst.file_name().unwrap_or_default()));
    pack.args(&members);
    run(&mut pack)?;
    let _ = fs::remove_dir_all(work);
    Ok(())
}

/// The functions glibc lets a program replace for the whole library ("Replacing malloc" in the
/// GNU C Library manual): the library's own calls go to the program's definition, so these are
/// made weak in place and never renamed (D-X.14). The library hands caller-owned memory out and
/// takes it back only through them (`boundary::caller_alloc`).
const INTERPOSABLE: &[&str] = &["malloc", "free", "calloc", "realloc", "reallocarray", "aligned_alloc", "malloc_usable_size", "memalign", "posix_memalign", "pvalloc", "valloc", "cfree"];

/// The prefix of the library's own name for a function it defines (D-X.14).
pub const OWN_PREFIX: &str = "__liella_libc_";

/// A defined symbol as `readelf -sW` shows it: name, type, bind, section index, value.
type SymRow = (String, String, String, String, String);

/// `readelf -sW` of `obj`: every defined symbol.
fn symbols_of(obj: &Path) -> Result<Vec<SymRow>, String> {
    let out = Command::new("readelf").args(["-sW"]).arg(obj).output().map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!("readelf -s {} failed", obj.display()));
    }
    let mut v = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Num: Value Size Type Bind Vis Ndx Name
        if f.len() == 8 && f[0].ends_with(':') && f[6] != "UND" && f[6] != "ABS" {
            v.push((f[7].to_string(), f[3].to_string(), f[4].to_string(), f[6].to_string(), f[1].to_string()));
        }
    }
    Ok(v)
}

/// `readelf -SW` of `obj`: section index to name.
pub(crate) fn sections_of(obj: &Path) -> Result<BTreeMap<String, String>, String> {
    let out = Command::new("readelf").args(["-SW"]).arg(obj).output().map_err(|e| format!("readelf: {e}"))?;
    let mut m = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((idx, rest)) = line.trim_start().strip_prefix('[').and_then(|l| l.split_once(']')) else { continue };
        if let Some(name) = rest.split_whitespace().next() {
            m.insert(idx.trim().to_string(), name.to_string());
        }
    }
    Ok(m)
}

/// Whether a symbol is the library's C-visible name for something (not a mangled Rust name or
/// a versioned one).
fn plain(name: &str) -> bool {
    !(name.starts_with("_ZN") || name.starts_with("_R") || name.contains('@') || name.starts_with('.'))
}

/// The `core::`/`alloc::` names `nm -g -C --defined-only` output (`text`) defines: a static
/// `libc.a` must define none globally, or a Rust program's own `core` and `alloc` collide with
/// them ("multiple definition", COPY-FILL-3). Undefined names are ignored, and so are the
/// members of `compiler_builtins` (the toolchain's own objects, archived unchanged: the same
/// objects a Rust program links, so the library adds no definition of its own there).
fn exported_rust_std(text: &str) -> Vec<String> {
    let mut builtins = false;
    text.lines()
        .filter_map(|l| {
            if let Some(member) = l.strip_suffix(':') {
                builtins = member.starts_with("compiler_builtins-");
                return None;
            }
            if builtins {
                return None;
            }
            let mut f = l.splitn(3, ' ');
            let (_, kind, name) = (f.next()?, f.next()?, f.next()?);
            let std = ["core::", "alloc::", "<core::", "<alloc::"].iter().any(|p| name.starts_with(p)) || name.contains(" as core::") || name.contains(" as alloc::");
            (kind != "U" && kind != "w" && std).then(|| name.to_string())
        })
        .collect()
}

/// `exported_rust_std` over the archive at `archive`, as an error naming the first few.
fn rust_symbols_stay_local(archive: &Path) -> Result<(), String> {
    let out = Command::new("nm").args(["-g", "-C", "--defined-only"]).arg(archive).output().map_err(|e| format!("nm: {e}"))?;
    let found = exported_rust_std(&String::from_utf8_lossy(&out.stdout));
    if found.is_empty() {
        return Ok(());
    }
    Err(format!("{}: {} global core::/alloc:: definitions (a Rust program's static link would collide), e.g. {}", archive.display(), found.len(), found.iter().take(3).cloned().collect::<Vec<_>>().join(", ")))
}

/// Make `src`'s definitions overridable by a program's (D-X.14): every global definition becomes
/// weak, so a program's strong definition of the same name wins instead of colliding with it
/// ("multiple definition"), and the library's own calls keep going to the library's functions.
/// For every global function (but [`INTERPOSABLE`]) the definition and every reference to it in
/// every member is renamed `__liella_libc_<name>` (`objcopy --redefine-syms`), and a weak alias
/// `<name>` is added at the same place for programs. Data symbols and the interposable
/// functions are weakened in place, so the library and the program share the winning
/// definition (a program's `obstack_alloc_failed_handler`, its `malloc`). The result goes to
/// `dst`; `work` is a scratch directory. The numbers of renamed functions and of names weakened
/// in place.
fn weaken_archive(src: &Path, work: &Path, dst: &Path) -> Result<(usize, usize), String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let ar = std::env::var("AR").unwrap_or_else(|_| "ar".into());
    let objcopy = std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into());
    let src = fs::canonicalize(src).map_err(|e| e.to_string())?;
    let listed = Command::new(&ar).arg("t").arg(&src).output().map_err(|e| format!("ar t: {e}"))?;
    let members: Vec<String> = String::from_utf8_lossy(&listed.stdout).lines().map(str::to_string).collect();
    let unique: BTreeSet<&String> = members.iter().collect();
    if unique.len() != members.len() {
        return Err(format!("{}: duplicate member names; cannot extract them safely", src.display()));
    }
    run(Command::new(&ar).current_dir(work).arg("x").arg(&src))?;
    // The functions to rename (every member's global functions) and what is weakened in place.
    let mut rename: BTreeSet<String> = BTreeSet::new();
    let mut in_place: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for m in &members {
        for (name, ty, bind, _, _) in symbols_of(&work.join(m))? {
            if bind != "GLOBAL" || !plain(&name) {
                continue;
            }
            if (ty == "FUNC" || ty == "IFUNC") && !INTERPOSABLE.contains(&name.as_str()) {
                rename.insert(name);
            } else {
                in_place.entry(m.clone()).or_default().insert(name);
            }
        }
    }
    let map = work.join("liella-rename.map");
    fs::write(&map, rename.iter().map(|n| format!("{n} {OWN_PREFIX}{n}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    let mut weakened = 0;
    for m in &members {
        let obj = work.join(m);
        let mut pass1 = Command::new(&objcopy);
        pass1.arg(format!("--redefine-syms={}", map.display()));
        for n in in_place.get(m).into_iter().flatten() {
            pass1.arg(format!("--weaken-symbol={n}"));
            weakened += 1;
        }
        run(pass1.arg(&obj))?;
        // The aliases, at the renamed definitions.
        let sections = sections_of(&obj)?;
        let mut pass2 = Command::new(&objcopy);
        let mut any = false;
        for (name, ty, bind, ndx, value) in symbols_of(&obj)? {
            let Some(orig) = name.strip_prefix(OWN_PREFIX) else { continue };
            if bind != "GLOBAL" && bind != "WEAK" {
                continue;
            }
            let Some(sec) = sections.get(&ndx) else { return Err(format!("{m}: {name} in section {ndx}, which has no name")) };
            let kind = if ty == "IFUNC" { "indirect-function" } else { "function" };
            pass2.arg(format!("--add-symbol={orig}={sec}:0x{value},weak,{kind}"));
            any = true;
        }
        if any {
            run(pass2.arg(&obj))?;
        }
    }
    let _ = fs::remove_file(dst);
    let mut pack = Command::new(&ar);
    pack.current_dir(work).arg("rcsD").arg(fs::canonicalize(dst.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?.join(dst.file_name().unwrap_or_default()));
    pack.args(&members);
    run(&mut pack)?;
    let _ = fs::remove_dir_all(work);
    Ok((rename.len(), weakened))
}

/// The cargo profile the products are built with. `Fast` (`[profile.fast]`: thin LTO, 16 codegen
/// units) is for tests and iteration: a one-line change relinks in 20-40 s instead of 77-95 s.
/// `Release` (fat LTO, one codegen unit) is the shipped configuration: every benchmark, and anything
/// that measures speed, uses it, because performance numbers must come from what ships.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Fast,
    Release,
}

impl Profile {
    /// The cargo profile name; also the directory below `target/product` that holds its output.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Fast => "fast",
            Profile::Release => "release",
        }
    }

    /// The profile of the test paths (`cargo xtask so`, `test`, `affected`): `Fast`, unless
    /// `LIELLA_FULL_PROFILE=1` forces the shipped one (a gate that wants fat-LTO products).
    pub fn for_tests() -> Profile {
        if std::env::var("LIELLA_FULL_PROFILE").is_ok_and(|v| !v.is_empty() && v != "0") {
            Profile::Release
        } else {
            Profile::Fast
        }
    }
}

/// Which profile built the products now in `target/liella` (`None`: unknown, or none built). The
/// marker is written last, after every product, and removed first, so a half-built set is unknown.
pub fn built_profile(root: &Path) -> Option<Profile> {
    match fs::read_to_string(root.join("target/liella/.profile")).ok()?.trim() {
        "fast" => Some(Profile::Fast),
        "release" => Some(Profile::Release),
        _ => None,
    }
}

/// `cargo xtask so`: the products with the test profile (see `Profile::for_tests`).
pub fn run_so(root: &Path) -> Result<(), String> {
    run_so_as(root, Profile::for_tests())
}

/// Step 1 of `run_so`: the product crate as a static archive in `product`, its math names
/// localized, its copies bound to the internal entries (D-X.15), weakened (D-X.14) into `out`.
/// Returns the archive as rustc built it (input of the shared object), the weakening counts and
/// the internal-copies report.
/// `rustflags` (measurement builds only, MEASURE-1: `-Ctarget-cpu=x86-64-v3`) applies to every
/// crate of the product build, after the product flags that every build gets (64-byte function
/// alignment, D-4.33); the products themselves are built with `None`. `profile`: the
/// test paths pass `Profile::for_tests()`; every measurement build (`rustflags` or not) passes
/// `Release`. The cargo output lands in `product/<profile>`, so profiles never share an archive.
pub fn build_archive(root: &Path, product: &Path, out: &Path, rustflags: Option<&str>, profile: Profile) -> Result<(std::path::PathBuf, (usize, usize), crate::copies::Stats), String> {
    let _lock = lock_product_dir(product)?;
    build_archive_locked(root, product, out, rustflags, profile)
}

/// [`build_archive`] with the caller holding the lock of `product` (`run_so_as` holds it for the whole run).
fn build_archive_locked(root: &Path, product: &Path, out: &Path, rustflags: Option<&str>, profile: Profile) -> Result<(std::path::PathBuf, (usize, usize), crate::copies::Stats), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = root.join("crates/libc-so/Cargo.toml");
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(root).args(["rustc", "-q", "--profile", profile.name(), "--crate-type", "staticlib", "--manifest-path"]).arg(&manifest).arg("--target-dir").arg(product);
    // Every product crate gets the 64-byte function alignment (D-4.33), then the caller's flags.
    cmd.env("CARGO_ENCODED_RUSTFLAGS", crate::align::encoded_rustflags(root, rustflags));
    cmd.env("LIELLA_BUILD_TAG", build_tag(root));
    run(&mut cmd)?;
    // Cargo's archive stays as cargo built it; the steps below work on a fresh copy, so every run
    // post-processes the same input once (a killed run cannot leave a half-edited archive behind).
    let raw = product.join(profile.name()).join("libc.a");
    let archive = product.join(profile.name()).join("liella-processed.a");
    fs::copy(&raw, &archive).map_err(|e| format!("copy {}: {e}", raw.display()))?;
    // libc's math exports (`copysign`, `frexp`, ...) must not merge with compiler_builtins' copies,
    // and its copies of libm's names (`fmod`, `ceil`, ...) must not be visible in libc.a: glibc's
    // libc.a does not define them, and a configure test that finds one there drops `-lm`
    // (LIBC-A-PLACEMENT). The libm names glibc's libc.a does define stay as they are.
    let (libc_math, both, libm_only) = crate::math::owners(root)?;
    let names = product.join("libc-math.names");
    let localize = libc_math.union(&both).chain(libm_only.iter().filter(|s| !LIBC_A_LIBM_NAMES.contains(&s.as_str()))).map(|s| format!("{s}\n")).collect::<String>();
    fs::write(&names, localize).map_err(|e| e.to_string())?;
    // The library's own calls of such a name (`fma`, from `mul_add`) go to a private name for the
    // builtins copy, so a static program without `-lm` still links.
    let undef = Command::new("nm").args(["-u", "-P"]).arg(&archive).output().map_err(|e| format!("nm: {e}"))?;
    let needs: BTreeSet<String> = String::from_utf8_lossy(&undef.stdout).lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect();
    let private: Vec<&String> = libm_only.iter().filter(|s| needs.contains(*s) && !LIBC_A_LIBM_NAMES.contains(&s.as_str())).collect();
    if !private.is_empty() {
        let mut objcopy = Command::new("objcopy");
        for s in &private {
            objcopy.arg(format!("--redefine-sym={s}=liella_libc_{s}"));
        }
        run(objcopy.arg(&archive))?;
        run(Command::new(std::env::var("AR").unwrap_or_else(|_| "ar".into())).arg("s").arg(&archive))?;
    }
    localize_builtins(&std::env::var("AR").unwrap_or_else(|_| "ar".into()), &archive, &names, &product.join("libc-work"))?;
    // Large zero-initialised tables after the small state words, out of the start-up pages (D-X.22).
    crate::coldbss::move_cold_bss(&std::env::var("AR").unwrap_or_else(|_| "ar".into()), &archive, &product.join("coldbss-work"))?;
    crate::coldbss::move_startup_state(&std::env::var("AR").unwrap_or_else(|_| "ar".into()), &archive, &product.join("state-work"))?;
    // The library's own copies and fills call its internal entries, directly (D-X.15).
    let copies = crate::copies::direct_internal_copies(&std::env::var("AR").unwrap_or_else(|_| "ar".into()), &archive, &product.join("copies-work"))?;
    // (done in the archive as rustc built it, so `libc.so.6` and the weakened `libc.a` both get it)
    // The functions short programs run, grouped into `.text.hot.*` (PAGEFAULTS-2: fewer text faults).
    let (hot, _) = crate::hottext::group(&std::env::var("AR").unwrap_or_else(|_| "ar".into()), &archive, &root.join("crates/libc-so/hot-text.list"), &product.join("hot-work"), rustflags.is_none().then(|| profile.name()))?;
    println!("so: {hot} function sections grouped as hot text");
    let edges = exclude_link_edges(&archive)?;
    println!("so: {edges} link-edge words excluded from every link output (D-22.16)");
    check_archive_elf(&archive)?;
    let staged = tmp_name(out);
    let weak = weaken_archive(&archive, &product.join("weaken"), &staged)?;
    // After the weakening pass, which rebuilds the product crate's objects.
    shared_personality_ref(&std::env::var("AR").unwrap_or_else(|_| "ar".into()), &staged, &product.join("personality-work"))?;
    rust_symbols_stay_local(&staged)?;
    check_archive_elf(&staged)?;
    boot_anchor_pc_relative(&staged)?;
    publish(&staged, out)?;
    Ok((archive, weak, copies))
}

/// The name every link-edge static carries (D-22.16): a `#[used]` Rust static whose only job is
/// an undefined symbol in its object, so that a static link taking the object also takes the
/// archive member defining that symbol (`LIELLA_LINK_EDGE_PTHREAD_ONCE`: the keys pull
/// machine/once.S). Its 8-byte word is never read.
const LINK_EDGE: &str = "LIELLA_LINK_EDGE_";

/// Marks each link-edge static's section (`.data.rel.ro.<mangled name>`) `SHF_EXCLUDE` in
/// `archive`: the linker then puts neither the word nor its relocation in any program or in
/// `libc.so.6` (`#[used]` makes the section `SHF_GNU_RETAIN`, so `--gc-sections` alone keeps it,
/// and in `libc.so.6` it would cost a `R_X86_64_RELATIVE` relocation at every start), while the
/// object's undefined symbol still pulls the member. Returns the number of such sections; an
/// archive with none, or one the flag did not reach, is an error.
fn exclude_link_edges(archive: &Path) -> Result<usize, String> {
    let objcopy = std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into());
    let pattern = format!(".data.rel.ro.*{LINK_EDGE}*");
    run(Command::new(&objcopy).arg(format!("--set-section-flags={pattern}=contents,exclude")).arg(archive))?;
    run(Command::new(std::env::var("AR").unwrap_or_else(|_| "ar".into())).arg("s").arg(archive))?;
    let out = Command::new("readelf").args(["-SW"]).arg(archive).output().map_err(|e| format!("readelf: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut found = 0;
    for l in text.lines().filter(|l| l.contains(LINK_EDGE)) {
        // `[Nr] Name Type Address Off Size ES Flg Lk Inf Al`: after the bracketed index, the
        // name is the first field and the flags the seventh (its `.rela` section is not checked).
        let fields: Vec<&str> = l.split(']').nth(1).unwrap_or("").split_whitespace().collect();
        if !fields.first().is_some_and(|n| n.starts_with(".data.rel.ro.")) {
            continue;
        }
        if !fields.get(6).is_some_and(|f| f.contains('E') && !f.contains('A')) {
            return Err(format!("{}: link-edge section not excluded: {}", archive.display(), l.trim()));
        }
        found += 1;
    }
    if found == 0 {
        return Err(format!("{}: no {LINK_EDGE}* section (renamed static? D-22.16)", archive.display()));
    }
    Ok(found)
}

/// `so` carries no link-edge word (D-22.16): no symbol named `LIELLA_LINK_EDGE_*`.
fn check_no_link_edge(so: &Path) -> Result<(), String> {
    let out = Command::new("nm").arg(so).output().map_err(|e| format!("nm: {e}"))?;
    match String::from_utf8_lossy(&out.stdout).lines().find(|l| l.contains(LINK_EDGE)) {
        Some(l) => Err(format!("{} carries a link-edge word: {l}", so.display())),
        None => Ok(()),
    }
}

pub fn run_so_as(root: &Path, profile: Profile) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let product = root.join("target/product");
    let release = root.join("target/liella");
    // One product build per tree at a time; the lock is held to the end of this function.
    let _lock = lock_product_dir(&product)?;
    fs::create_dir_all(&release).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(release.join(".profile"));
    println!("so: profile {}", profile.name());

    // 1. The archive, with its definitions made overridable (D-X.14).
    let (archive, weak, copies) = build_archive_locked(root, &product, &release.join("libc.a"), None, profile)?;

    // 2. The compat versions (LG-1, `compat.rs`): their stubs are assembled here and linked into
    //    libc.so.6 only, never archived (R15-2); every stub's target must be defined by libc.a.
    let compat_obj = product.join("compat_aliases.o");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    run(Command::new(&cc).arg("-c").arg(root.join(crate::compat::FILE)).arg("-o").arg(&compat_obj))?;
    let compat = crate::compat::entries(root)?;
    let mut have = defined(&archive)?;
    let missing: Vec<String> = compat.iter().filter(|e| !have.contains(&e.target)).map(|e| format!("{}@{} -> {}", e.name, e.version, e.target)).collect();
    if !missing.is_empty() {
        return Err(format!("libc.a does not define the target of {} compat versions: {}", missing.len(), missing.join(", ")));
    }
    have.extend(defined(&compat_obj)?);

    // 3. The version script.
    let (script, count) = version_script(root, &have)?;
    let map = product.join("libc.map");
    fs::write(&map, script).map_err(|e| e.to_string())?;

    // 3. The shared object, linked from the archive as rustc built it (not the weakened copy:
    //    D-X.14 concerns static links only) with only our version script (rustc's own cdylib
    //    script would make every export unversioned). Unreferenced code is dropped.
    //    glibc's WEAK exports (versions.tsv `scope`) are weak definitions in the input, so the
    //    exported binding matches (checked below).
    let so_final = release.join("libc.so.6");
    let so = tmp_name(&so_final);
    let scopes = libc_scopes(root)?;
    let so_input = product.join("libc-so-input.a");
    weaken_names_in_archive(&archive, &product.join("weaken-so"), &so_input, &weak_names(&scopes)?)?;
    run(Command::new(&cc)
        .args(["-shared", "-nostdlib", "-Wl,--gc-sections", "-Wl,-z,now", "-Wl,-z,relro", "-Wl,--hash-style=gnu"])
        .arg(format!("-Wl,--version-script={}", map.display()))
        .arg("-Wl,-soname,libc.so.6")
        .arg("-Wl,--whole-archive")
        .arg(&so_input)
        .arg("-Wl,--no-whole-archive")
        .arg(&compat_obj)
        .arg("-lgcc")
        .arg("-o")
        .arg(&so))?;
    check_no_link_edge(&so)?;
    let native = check_native_exports(root, &so)?;
    let compat_count = check_compat_exports(root, &so, "libc.so.6", &compat)?;
    let objects = crate::compat_objects::check(root, &so)?;
    println!("so: {objects} data objects exported, each at its versions.tsv size (LG-6)");
    println!("so: {compat_count} compat versions exported as name@VERSION ({})", crate::compat::FILE);
    let bound = check_binding(root, &so)?;
    println!("so: binding of {bound} libc.so.6 exports checked against glibc's (spec/abi/versions.tsv scope)");
    let native_names: BTreeSet<String> = crate::native::symbols(root)?.into_iter().map(|(s, _)| s).collect();
    let versioned = check_version_sets(root, &so, "libc.so.6", &have, &native_names)?;
    println!("so: version set and default version of {versioned} libc.so.6 names checked against spec/abi/versions.tsv");
    // The 64-byte entry alignment (D-4.33) is contract FN-ALIGN-EXPORTS, checked with the others at the end.
    publish(&so, &so_final)?;

    // 4. The start object.
    let crt1 = tmp_name(&release.join("liella-crt1.o"));
    run(Command::new(&cc).arg("-c").arg(root.join("csys/src/machine/entry.S")).arg("-o").arg(&crt1))?;
    publish(&crt1, &release.join("liella-crt1.o"))?;
    // The static-PIE start object (D-27.10): the same file, calling the relocating start.
    let rcrt1 = tmp_name(&release.join("liella-rcrt1.o"));
    run(Command::new(&cc).arg("-c").arg("-DLIELLA_STATIC_PIE").arg(root.join("csys/src/machine/entry.S")).arg("-o").arg(&rcrt1))?;
    start_object_pc_relative(&rcrt1)?;
    publish(&rcrt1, &release.join("liella-rcrt1.o"))?;
    // The profiling start object for `-pg` programs (GCRT1-START): the same `_start`, plus
    // `__gmon_start__` in `.preinit_array` (monstartup over the text, atexit(_mcleanup)).
    let gcrt1 = tmp_name(&release.join("liella-gcrt1.o"));
    run(Command::new(&cc).arg("-c").arg(root.join("csys/start/gcrt1.S")).arg("-o").arg(&gcrt1))?;
    publish(&gcrt1, &release.join("liella-gcrt1.o"))?;

    // 5. The placeholder libraries (D-27.9, design plan/ph14/design.md §16; spec §22 notes: since
    //    2.34 libpthread is in libc.so.6): libpthread.so.0, libdl.so.2, librt.so.1, libutil.so.1,
    //    libanl.so.1 and libc_malloc_debug.so.0, generated from versions.tsv, linked against
    //    libc.so.6 and each checked against the table; empty static archives for `-l<x>`.
    let ar = std::env::var("AR").unwrap_or_else(|_| "ar".into());
    println!("so: {}", crate::placeholders::build(root, &release, &product.join("placeholders"), &cc, &ar)?);

    // 6. The static libresolv placeholder (D-16.6): the symbols spec Appendix C places in
    //    `libresolv.so.2` (`inet_net_pton`, `res_*`, `ns_*`, ...) are in `libc.a` like every other
    //    symbol, so `-lresolv` must only resolve. They are left out of `libc.so.6`'s version
    //    script (their rows name `libresolv.so.2`); the shared `libresolv.so.2` comes with the
    //    dynamic products of phase 14.
    empty_archive(&ar, &release.join("libresolv.a"))?;

    let redirected: Vec<String> = copies.redirected.iter().map(|(n, c)| format!("{n} {c}")).collect();
    let kept: usize = copies.kept.values().sum();
    println!("so: internal copies (D-X.15): {} references to the internal entries (redirected now: {}; made relaxable: {}), {kept} other uses of memcpy/memmove/memset kept", copies.internal, redirected.join(", "), copies.relaxable_internal);
    println!("so: GOT calls, jumps and loads made relaxable (D-X.15, PJ-19): {}, {} other GOT uses left as they are", copies.relaxable, copies.unrecognised);
    println!("so: target/liella/libc.a ({} functions overridable with internal calls bound to the library, {} names weak in place), libc.so.6 ({count} versioned symbols, {native} of them native), liella-crt1.o, liella-rcrt1.o, liella-gcrt1.o, libresolv placeholder", weak.0, weak.1);
    run_libm(root, &cargo, &cc, &ar, profile)?;
    run_ld(root, &cargo, &cc, profile)?;
    check_placement(root, &release)?;
    // R14-12: no shared product defines or exports a GLIBC_PRIVATE version.
    crate::placeholders::check_no_private(&release)?;
    // After the last product: every layout contract of layout/contracts.tsv (D-X.48) on the products, among them
    // the 64-byte entries (D-4.33), the classed pages' XREF bias (D-X.34), the hot kernels' jumps off 32-byte
    // boundaries (D-4.67) and no build path in any product (PRODUCT-PATH-REMAP).
    // STARTUP-TEXT-WINDOWS (D-X.49) reads its budget for this profile.
    crate::layout::check_products(root, &release, Some(profile.name()))?;
    // The marker last, so a set that was not finished has none.
    let marker = tmp_name(&release.join(".profile"));
    fs::write(&marker, format!("{}
", profile.name())).map_err(|e| e.to_string())?;
    publish(&marker, &release.join(".profile"))
}

/// The interpreter product (phase 14 DL-5, R14-2, D-27.13): `crates/ld-so` as a static archive,
/// linked with the `ld` variant of `machine/entry.S` into `target/liella/<Arch::INTERP>`, a shared
/// object whose entry point is `liella_machine_ld_entry` and whose every symbol is local but
/// [`LD_EXPORTS`] (R14-12, D-27.20), then checked: [`check_interp`] (L14-1) and [`check_interp_prerelocation`] (design §1).
/// The interpreter's Rust code runs before its image is relocated (design §0a item 9): with the
/// `pie` model every symbol is local to the image, so calls and data references are PC-relative,
/// never through a GOT slot that still holds a link-time address (rustc's default no-PLT calls
/// load the callee from the GOT; the first, `liella_link_image_header`, crashed ld.so). Every
/// symbol is local in the linked object (version script), so the model's assumption holds.
const LD_RUSTFLAGS: &str = "-Crelocation-model=pie";

fn run_ld(root: &Path, cargo: &str, cc: &str, profile: Profile) -> Result<(), String> {
    use liella_ld::arch::Arch as _;
    let name = liella_ld::arch::Native::INTERP;
    let manifest = root.join("crates/ld-so/Cargo.toml");
    let product = root.join("target/product");
    let release = root.join("target/liella");
    run(Command::new(cargo).current_dir(root).args(["rustc", "-q", "--profile", profile.name(), "--crate-type", "staticlib", "--manifest-path"]).arg(&manifest).arg("--target-dir").arg(&product).env("CARGO_ENCODED_RUSTFLAGS", crate::align::encoded_rustflags(root, Some(LD_RUSTFLAGS))).env("LIELLA_BUILD_TAG", build_tag(root)))?;
    let archive = product.join(profile.name()).join("libld_so.a");
    let entry = product.join("ld-entry.o");
    run(Command::new(cc).arg("-c").arg("-DLIELLA_LD").arg(root.join("csys/src/machine/entry.S")).arg("-o").arg(&entry))?;
    let map = product.join("ld.map");
    fs::write(&map, ld_version_script(root, name)?).map_err(|e| e.to_string())?;
    let out = release.join(name);
    let staged = tmp_name(&out);
    run(Command::new(cc)
        .args(["-shared", "-nostdlib", "-Wl,--gc-sections", "-Wl,-z,now", "-Wl,-z,relro", "-Wl,--hash-style=gnu", "-Wl,-z,pack-relative-relocs", "-Wl,-e,liella_machine_ld_entry", "-Wl,-Bsymbolic"])
        .arg(format!("-Wl,--version-script={}", map.display()))
        .arg(format!("-Wl,-soname,{name}"))
        .arg(&entry)
        .arg(&archive)
        .arg("-lgcc")
        .arg("-o")
        .arg(&staged))?;
    check_interp(&staged)?;
    check_interp_prerelocation(&staged)?;
    publish(&staged, &out)?;
    println!("so: target/liella/{name} (the interpreter; L14-1 link check and pre-relocation check passed)");
    Ok(())
}

/// The symbols the interpreter exports (DL-12, D-27.20), each at the version `spec/abi/versions.tsv`
/// gives it for the interpreter's soname; every other symbol stays local.
const LD_EXPORTS: &[&str] = &["__tls_get_addr"];

/// The interpreter's version script: one node per version of [`LD_EXPORTS`], everything else local.
fn ld_version_script(root: &Path, soname: &str) -> Result<String, String> {
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let mut nodes: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for sym in LD_EXPORTS {
        let version = table.lines().map(|l| l.split('\t').collect::<Vec<_>>()).find(|f| f.first() == Some(&soname) && f.get(1) == Some(sym)).and_then(|f| f.get(2).map(|v| (*v).to_string()));
        let version = version.ok_or_else(|| format!("versions.tsv: no row for {soname} {sym}"))?;
        nodes.entry(version).or_default().push(sym);
    }
    let mut out = String::new();
    for (i, (version, syms)) in nodes.iter().enumerate() {
        let local = if i == 0 { "  local: *;\n" } else { "" };
        out.push_str(&format!("{version} {{\n  global: {};\n{local}}};\n", syms.join("; ")));
    }
    if out.is_empty() {
        out.push_str("{\n  local: *;\n};\n");
    }
    Ok(out)
}

/// L14-1 over a linked interpreter: no undefined symbol, no `DT_NEEDED`, relocations only
/// `RelocKind::Relative` and RELR, no text relocations (`liella_ld::link`, design §1).
pub(crate) fn check_interp(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r = liella_ld::link::check::<liella_ld::arch::Native>(&bytes).map_err(|e| format!("{}: {e:?}", path.display()))?;
    if r.holds() {
        return Ok(());
    }
    let first = r.first_undefined.map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
    Err(format!(
        "{}: the interpreter's link invariant (L14-1) does not hold: dynamic {}, {} undefined symbols (first {first:?}), {} DT_NEEDED, {} relocations other than RELATIVE/RELR (first {:?}), text relocations {}",
        path.display(), r.dynamic, r.undefined, r.needed, r.other, r.first_other, r.textrel
    ))
}

/// The entry primitive of `ld.so` (`machine/entry.S` with `-DLIELLA_LD`).
const LD_ENTRY: &str = "liella_machine_ld_entry";
/// The Rust start it calls; only its part up to the call of [`LD_RELOCATOR`] runs unrelocated.
const LD_START: &str = "liella_ld_start";
/// The relocation loop's function (`bootstrap::relocate_image`, a mangled Rust name).
const LD_RELOCATOR: &str = "relocate_image";

/// Design §1 over the linked `ld.so` (D-27.13): no instruction that runs before the relocation loop
/// has finished reads a word a dynamic relocation writes (a GOT slot, a pointer in `.data.rel.ro`):
/// it would still hold its link-time address. Static, over `objdump -d`: the code reached by direct
/// calls and jumps from [`LD_ENTRY`], with [`LD_START`] cut after its call of [`LD_RELOCATOR`].
fn check_interp_prerelocation(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut words = BTreeSet::new();
    liella_ld::link::relocated_words::<liella_ld::arch::Native>(&bytes, |w| {
        words.insert(w);
    })
    .map_err(|e| format!("{}: {e:?}", path.display()))?;
    let out = Command::new("objdump").args(["-d", "--no-show-raw-insn"]).arg(path).output().map_err(|e| format!("objdump: {e}"))?;
    if !out.status.success() {
        return Err(format!("objdump -d {} failed", path.display()));
    }
    prerelocation_errors(&String::from_utf8_lossy(&out.stdout), &words).map_err(|e| format!("{}: {e}", path.display()))
}

/// One function of an `objdump -d` listing: its name, start address and `(address, text)` lines.
struct Func<'a> {
    name: &'a str,
    start: u64,
    insns: Vec<(u64, &'a str)>,
}

/// The functions of an `objdump -d --no-show-raw-insn` listing, in address order.
fn listing(dis: &str) -> Vec<Func<'_>> {
    let mut funcs: Vec<Func<'_>> = Vec::new();
    for line in dis.lines() {
        // A function header starts in column 0 (`0000000000001000 <name>:`); instructions are indented.
        if !line.starts_with(char::is_whitespace) {
            if let Some((Ok(start), Some(name))) = line.split_once(" <").map(|(a, rest)| (u64::from_str_radix(a, 16), rest.strip_suffix(">:"))) {
                funcs.push(Func { name, start, insns: Vec::new() });
            }
            continue;
        }
        let Some((addr, text)) = line.trim_start().split_once(":\t") else { continue };
        if let (Ok(addr), Some(f)) = (u64::from_str_radix(addr, 16), funcs.last_mut()) {
            f.insns.push((addr, text.trim()));
        }
    }
    funcs.sort_by_key(|f| f.start);
    funcs
}

/// The check of [`check_interp_prerelocation`] on a listing and the set of relocated words.
// ARCH(x86_64): the operand syntax read here is objdump's AT&T x86-64 form: `call 1234 <f>`,
// `jcc 1234 <f+0x10>`, `op 0x10(%rip),%reg   # 1234 <sym>`, `call *%reg`; aarch64 needs its own reader.
pub fn prerelocation_errors(dis: &str, relocated: &BTreeSet<u64>) -> Result<(), String> {
    let funcs = listing(dis);
    let find = |name: &str| funcs.iter().position(|f| f.name == name).ok_or_else(|| format!("no function {name} in the listing"));
    let containing = |addr: u64| funcs.iter().rposition(|f| f.start <= addr).filter(|&i| funcs.get(i).is_some_and(|f| f.insns.last().is_some_and(|l| addr <= l.0)));
    let mut todo = vec![find(LD_ENTRY)?];
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    while let Some(i) = todo.pop() {
        let Some(f) = funcs.get(i).filter(|_| seen.insert(i)) else { continue };
        let mut insns = f.insns.as_slice();
        if f.name == LD_START {
            let cut = insns.iter().position(|(_, t)| t.starts_with("call") && t.contains(LD_RELOCATOR));
            let cut = cut.ok_or_else(|| format!("{LD_START} does not call {LD_RELOCATOR} (keep it out of line)"))?;
            insns = insns.get(..=cut).unwrap_or(insns);
        }
        for &(addr, text) in insns {
            let (code, comment) = text.split_once('#').unwrap_or((text, ""));
            // Prefixes objdump prints before the mnemonic (`addr32 call`: a call the linker relaxed).
            let mut words = code.split_whitespace().skip_while(|w| ["addr32", "notrack", "bnd", "data16", "cs", "ds"].contains(w));
            let op = words.next().unwrap_or("");
            let operands = words.next().unwrap_or("");
            let branch = op.starts_with("call") || op.starts_with('j');
            if operands.contains("(%rip)") && !op.starts_with("lea") && !op.starts_with("nop") {
                let target = comment.split_whitespace().next().and_then(|t| u64::from_str_radix(t, 16).ok());
                if target.is_none_or(|t| relocated.range(t.saturating_sub(7)..=t).next().is_some()) {
                    errors.push(format!("{} {addr:x}: `{text}` reads a relocated word before relocation", f.name));
                }
            } else if branch && operands.starts_with('*') {
                // A register-indirect `jmp` is a jump table inside the function (PC-relative
                // entries); a register-indirect call reaches code this check cannot name.
                if op.starts_with("call") {
                    errors.push(format!("{} {addr:x}: `{text}` calls an address the check cannot follow", f.name));
                }
            } else if branch {
                match u64::from_str_radix(operands, 16).ok().map(containing) {
                    Some(Some(j)) => todo.push(j),
                    _ => errors.push(format!("{} {addr:x}: `{text}` branches outside the listed code", f.name)),
                }
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(format!("ld.so reads its data before its relocation loop (design §1, D-27.13):\n{}", errors.join("\n"))) }
}

/// The names `spec/abi/versions.tsv` gives `libm.so.6` only that glibc 2.39's `libc.a` defines all
/// the same (black-box `nm -g --defined-only` of the host's archive, LIBC-A-PLACEMENT, 2026-10-23).
const LIBC_A_LIBM_NAMES: [&str; 28] = [
    "__finitef128", "__isinff128", "__signbitf128", "copysignf128", "copysignf32", "copysignf32x", "copysignf64", "copysignf64x", "frexpf128", "frexpf32", "frexpf32x", "frexpf64", "frexpf64x", "ldexpf128",
    "ldexpf32", "ldexpf32x", "ldexpf64", "ldexpf64x", "modff128", "modff32", "modff32x", "modff64", "modff64x", "scalbnf128", "scalbnf32", "scalbnf32x", "scalbnf64", "scalbnf64x",
];

/// The archive placement of public names (LIBC-A-PLACEMENT): a name in the wrong static archive
/// changes what a configure test finds (`fmod` in `libc.a` made gawk's configure drop `-lm`).
/// From `spec/abi/versions.tsv`: `libc.a` defines no name glibc exports from `libm.so.6` only
/// (except [`LIBC_A_LIBM_NAMES`]) and refers to none it does not define (a static program
/// without `-lm` must link); `libm.a` defines no name glibc exports from `libc.so.6`; `libc.a`
/// defines every name glibc exports from both (LIBC-A-PLACEMENT-2).
fn check_placement(root: &Path, release: &Path) -> Result<(), String> {
    let table = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    let (mut libc, mut libm) = (BTreeSet::new(), BTreeSet::new());
    for line in table.lines().skip(1) {
        match line.split('\t').collect::<Vec<_>>().as_slice() {
            ["libc.so.6", sym, ..] => libc.insert((*sym).to_string()),
            ["libm.so.6", sym, ..] => libm.insert((*sym).to_string()),
            _ => false,
        };
    }
    let libm_only: BTreeSet<&String> = libm.iter().filter(|s| !libc.contains(*s) && !LIBC_A_LIBM_NAMES.contains(&s.as_str())).collect();
    let libc_a = defined(&release.join("libc.a"))?;
    let libm_a = defined(&release.join("libm.a"))?;
    let out = Command::new("nm").args(["-u", "-P"]).arg(release.join("libc.a")).output().map_err(|e| format!("nm: {e}"))?;
    let needs: BTreeSet<String> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_whitespace().next()).filter(|s| !s.ends_with(':')).map(str::to_string).collect();
    let mut errors = Vec::new();
    errors.extend(libm_only.iter().filter(|s| libc_a.contains(**s)).map(|s| format!("libc.a defines {s}, which glibc's libc.a does not (libm.so.6 only)")));
    errors.extend(libm_only.iter().filter(|s| needs.contains(**s) && !libc_a.contains(**s)).map(|s| format!("libc.a refers to {s}, which only libm.a defines")));
    errors.extend(libm_a.iter().filter(|s| libc.contains(*s)).map(|s| format!("libm.a defines {s}, which glibc's libm.a does not (a libc.so.6 name)")));
    // The shared objects follow the same split: libc.so.6 exports no libm.so.6-only name, libm.so.6 none
    // that only libc.so.6 exports (PRE14-LIBC-FMOD). Compat versions (`name@VERSION`) count by their name.
    for (so, wrong, own, other) in [("libc.so.6", &libm, &libc, "libm.so.6"), ("libm.so.6", &libc, &libm, "libc.so.6")] {
        let dynsyms = dynamic_names(&release.join(so))?;
        errors.extend(dynsyms.iter().filter(|s| wrong.contains(*s) && !own.contains(*s)).map(|s| format!("{so} exports {s}, which glibc exports from {other} only")));
    }
    // The names both libraries export (`copysign`, `frexp`, `__finite`, ...: 24) are libc.a's: glibc's
    // libc.a defines every one of them, its libm.a none (LIBC-A-PLACEMENT-2).
    errors.extend(libm.iter().filter(|s| libc.contains(*s) && !libc_a.contains(*s)).map(|s| format!("libc.a does not define {s}, which glibc's libc.a does (libc.so.6 and libm.so.6)")));
    if errors.is_empty() { Ok(()) } else { Err(format!("archive placement differs from glibc's:\n{}", errors.join("\n"))) }
}

/// The names a shared object defines in its dynamic symbol table, without `@VERSION` suffixes.
fn dynamic_names(path: &Path) -> Result<BTreeSet<String>, String> {
    let out = Command::new("nm").args(["-D", "--defined-only", "-P"]).arg(path).output().map_err(|e| format!("nm: {e}"))?;
    if !out.status.success() {
        return Err(format!("nm -D {} failed", path.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_whitespace().next()).map(|s| s.split('@').next().unwrap_or(s).to_string()).collect())
}

/// An empty `ar` archive at `dst`, written under a temporary name and renamed into place.
pub(crate) fn empty_archive(ar: &str, dst: &Path) -> Result<(), String> {
    let staged = tmp_name(dst);
    let _ = fs::remove_file(&staged);
    run(Command::new(ar).arg("rcs").arg(&staged))?;
    publish(&staged, dst)
}

/// Rust's `compiler_builtins` defines weak, hidden copies of some C math names (`ceil`, `fma`,
/// `copysign`, ...) for targets without a C math library. Linked next to Liella's exports of the
/// same names they would make those hidden, so in `archive` they are made local (`names`: one
/// per line). `work` is a scratch directory.
fn localize_builtins(ar: &str, archive: &Path, names: &Path, work: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let listing = Command::new(ar).arg("t").arg(archive).output().map_err(|e| format!("ar: {e}"))?;
    let builtins: Vec<String> = String::from_utf8_lossy(&listing.stdout).lines().filter(|m| m.contains("compiler_builtins")).map(str::to_string).collect();
    if builtins.is_empty() {
        return Ok(());
    }
    run(Command::new(ar).current_dir(work).arg("x").arg(archive).args(&builtins))?;
    for m in &builtins {
        run(Command::new("objcopy").arg(format!("--localize-symbols={}", names.display())).arg(work.join(m)))?;
    }
    run(Command::new(ar).current_dir(work).arg("r").arg(archive).args(&builtins))
}

/// The product crate's own object(s) in `libc.a` carry `DW.ref.rust_eh_personality` (the
/// personality slot `.eh_frame` names) as a LOCAL symbol inside the COMDAT group of that name:
/// the link keeps that group and drops the other members' copies, so their references (weak
/// hidden, the compilers' usual form) find no definition. A non-PIE link resolves them to 0
/// silently; a static PIE link refuses them (D-27.10). The symbol is made weak hidden, the form
/// every other member has, so all references bind to the one kept slot. `work` is a scratch
/// directory.
fn shared_personality_ref(ar: &str, archive: &Path, work: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let listing = Command::new(ar).arg("t").arg(archive).output().map_err(|e| format!("ar: {e}"))?;
    let own: Vec<String> = String::from_utf8_lossy(&listing.stdout).lines().filter(|m| m.starts_with("c-") && m.ends_with(".rcgu.o")).map(str::to_string).collect();
    if own.is_empty() {
        return Ok(());
    }
    run(Command::new(ar).current_dir(work).arg("x").arg(archive).args(&own))?;
    for m in &own {
        run(Command::new("objcopy").arg("--globalize-symbol=DW.ref.rust_eh_personality").arg(work.join(m)))?;
        run(Command::new("objcopy").arg("--weaken-symbol=DW.ref.rust_eh_personality").arg(work.join(m)))?;
    }
    run(Command::new(ar).current_dir(work).arg("r").arg(archive).args(&own))
}

/// Data objects `libm.so.6` exports (spec §10.3).
const LIBM_DATA: [&str; 2] = ["signgam", "__signgam"];

/// The `libm` products (spec §10, D-10.7).
///
/// 1. `crates/libm-so` as a static library (`target/product/release/libm.a`): the math exports
///    and everything they use, including private copies of the library code they call.
/// 2. One relocatable object of it, with unused sections dropped (roots: the exports). Only the
///    exports stay global; the symbols glibc exports from both libraries (`frexp`, ...) become
///    weak for `libm.so.6` and local in the copy archived as `target/liella/libm.a` (glibc's
///    `libm.a` does not define them; a static link takes `libc.a`'s, LIBC-A-PLACEMENT).
/// 3. Checks with `nm`: every export defined, nothing else global, every undefined symbol
///    defined by `libc.a`, and no heap (the math code does not allocate).
/// 4. `target/liella/libm.so.6`, linked from that object against `libc.so.6` with the version
///    script of `libm.so.6`'s default versions; its dynamic symbol table is checked against it.
fn run_libm(root: &Path, cargo: &str, cc: &str, ar: &str, profile: Profile) -> Result<(), String> {
    let manifest = root.join("crates/libm-so/Cargo.toml");
    let product = root.join("target/product");
    let release = root.join("target/liella");
    run(Command::new(cargo).current_dir(root).args(["rustc", "-q", "--profile", profile.name(), "--crate-type", "staticlib", "--manifest-path"]).arg(&manifest).arg("--target-dir").arg(&product).env("CARGO_ENCODED_RUSTFLAGS", crate::align::encoded_rustflags(root, None)))?;
    let raw = product.join(profile.name()).join("libm.a");
    let (_, both, libm_only) = crate::math::owners(root)?;
    // The data objects of libm.so.6 (`signgam`, D-10.4) are not generated functions.
    let mut exports: BTreeSet<String> = libm_only.union(&both).filter(|s| !s.starts_with("liella_")).cloned().collect();
    exports.extend(LIBM_DATA.iter().map(|s| (*s).to_string()));

    // 2. One object, unused sections dropped, only the exports global.
    let keep = product.join("libm.keep");
    let weak = product.join("libm.weak");
    fs::write(&keep, exports.iter().map(|s| format!("{s}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    let staged = product.join("libm-raw.a");
    fs::copy(&raw, &staged).map_err(|e| format!("copy libm.a: {e}"))?;
    localize_builtins(ar, &staged, &keep, &product.join("libm-work"))?;
    let all = product.join("libm-all.o");
    let mut ld = Command::new("ld");
    ld.args(["-r", "--gc-sections"]);
    for s in &exports {
        ld.arg("-u").arg(s);
    }
    run(ld.arg("--whole-archive").arg(&staged).arg("--no-whole-archive").arg("-o").arg(&all))?;
    fs::write(&weak, both.iter().filter(|s| !s.starts_with("liella_")).map(|s| format!("{s}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    let obj = product.join("libm.o");
    run(Command::new("objcopy").arg("--strip-unneeded").arg(format!("--keep-global-symbols={}", keep.display())).arg(format!("--weaken-symbols={}", weak.display())).arg(&all).arg(&obj))?;
    // The archive's copy: the names glibc exports from both libraries are libc.a's alone in a
    // static link (glibc's libm.a does not define them, LIBC-A-PLACEMENT), so they are local here.
    let obj_a = product.join("libm-archive.o");
    run(Command::new("objcopy").arg(format!("--localize-symbols={}", weak.display())).arg(&obj).arg(&obj_a))?;
    let archive = release.join("libm.a");
    let staged_archive = tmp_name(&archive);
    let _ = fs::remove_file(&staged_archive);
    run(Command::new(ar).arg("rcs").arg(&staged_archive).arg(&obj_a))?;
    check_archive_elf(&staged_archive)?;
    publish(&staged_archive, &archive)?;

    // 3. The symbol table.
    let out = Command::new("nm").args(["-P"]).arg(&obj_a).output().map_err(|e| format!("nm: {e}"))?;
    let mut defined = BTreeSet::new();
    let mut undefined = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            [name, "U", ..] => {
                undefined.insert((*name).to_string());
            }
            // Global (upper case) and weak (`W`, `V`) definitions.
            [name, kind, ..] if kind.chars().all(|c| c.is_ascii_uppercase()) => {
                defined.insert((*name).to_string());
            }
            _ => {}
        }
    }
    let mut errors = Vec::new();
    let archived: BTreeSet<String> = exports.iter().filter(|s| !both.contains(*s)).cloned().collect();
    for s in archived.difference(&defined) {
        errors.push(format!("libm.a does not define {s}"));
    }
    for s in defined.difference(&archived) {
        errors.push(format!("libm.a exports {s}, which is not a math symbol"));
    }
    let libc = self::defined(&release.join("libc.a"))?;
    // libm.a and libm.so.6 export the same name set, except the names both glibc libraries export
    // (`both`: local in libm.a by LIBC-A-PLACEMENT-2), which libc.a must define instead.
    for s in exports.iter().filter(|s| !defined.contains(*s) && !LIBM_DATA.contains(&s.as_str())) {
        if !(both.contains(s) && libc.contains(s)) {
            errors.push(format!("libm.so.6 exports {s}, which neither libm.a nor (as a shared name) libc.a defines"));
        }
    }
    // Symbols the static link itself provides (from the start files and the linker).
    let linker = ["_GLOBAL_OFFSET_TABLE_"];
    for s in undefined.iter().filter(|s| !libc.contains(*s) && !linker.contains(&s.as_str())) {
        errors.push(format!("libm.a needs {s}, which libc.a does not define"));
    }
    // No heap: sections keep the names of the functions they hold.
    let sections = Command::new("readelf").args(["-SW"]).arg(&obj).output().map_err(|e| format!("readelf: {e}"))?;
    if String::from_utf8_lossy(&sections.stdout).lines().any(|l| l.contains("__rust_alloc") || l.contains("4heap")) {
        errors.push("libm.a contains the allocator: a math path allocates".into());
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }

    // 4. The shared object, with the compat versions (LG-4, `compat.rs`): their stubs are
    //    assembled here and linked into libm.so.6 only, never archived (R15-2); every stub's
    //    target must be a libm export.
    let compat_obj = product.join("compat_aliases_libm.o");
    run(Command::new(cc).arg("-c").arg(root.join(crate::compat::LIBM_FILE)).arg("-o").arg(&compat_obj))?;
    let compat = crate::compat::libm_entries(root)?;
    let missing: Vec<String> = compat.iter().filter(|e| !defined.contains(&e.target)).map(|e| format!("{}@{} -> {}", e.name, e.version, e.target)).collect();
    if !missing.is_empty() {
        return Err(format!("libm.a does not define the target of {} compat versions: {}", missing.len(), missing.join(", ")));
    }
    let mut have = exports.clone();
    have.extend(self::defined(&compat_obj)?);
    let (script, count) = lib_version_script(root, &have, "libm.so.6", false)?;
    let map = product.join("libm.map");
    fs::write(&map, script).map_err(|e| e.to_string())?;
    let so_final = release.join("libm.so.6");
    let so = tmp_name(&so_final);
    run(Command::new(cc)
        .args(["-shared", "-nostdlib", "-Wl,--gc-sections", "-Wl,-z,now", "-Wl,-z,relro", "-Wl,--hash-style=gnu"])
        .arg(format!("-Wl,--version-script={}", map.display()))
        .arg("-Wl,-soname,libm.so.6")
        .arg(&obj)
        .arg(&compat_obj)
        .arg(release.join("libc.so.6"))
        .arg("-lgcc")
        .arg("-o")
        .arg(&so))?;
    let out = Command::new("readelf").args(["--dyn-syms", "-W"]).arg(&so).output().map_err(|e| format!("readelf: {e}"))?;
    let mut exported = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (Some(sym), Some(ndx)) = (f.get(7), f.get(6)) {
            if *ndx != "UND" && sym.contains("@@") {
                exported.insert((*sym).to_string());
            }
        }
    }
    if exported.len() != count {
        return Err(format!("libm.so.6 exports {} versioned symbols, the version script {count}", exported.len()));
    }
    let compat_count = check_compat_exports(root, &so, "libm.so.6", &compat)?;
    let versioned = check_version_sets(root, &so, "libm.so.6", &have, &BTreeSet::new())?;
    publish(&so, &so_final)?;
    // No libm hot path merges into a register it has not written (D-10.68); published first so a finding can be read.
    crate::partial_write::check_built(root, &so_final)?;
    println!("so: target/liella/libm.a ({} symbols, {} of them libc.a's and local there), libm.so.6 ({count} versioned symbols, {compat_count} compat versions as name@VERSION ({}); version set and default version of {versioned} names checked against spec/abi/versions.tsv; it cannot be loaded before phase 14)", exports.len(), both.iter().filter(|s| !s.starts_with("liella_")).count(), crate::compat::LIBM_FILE);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// D-27.10 (no spec clause: start-up internals): the static-PIE anchor's member passes with a
    /// PC-relative reference, and fails with a GOT-typed one, without the reference, or when no
    /// member defines the anchor.
    #[test]
    fn boot_anchor_reference_is_pc_relative() {
        let member = |reloc: &str| {
            format!(
                "\nFile: libc.a(csys_3.o)\n\nRelocation section '.rela.text.liella_link_image_header' at offset 0x1f0 contains 1 entry:\n    Offset             Info             Type               Symbol's Value  Symbol's Name + Addend\n0000000000000003  0000000600000002 {reloc}      0000000000000000 __ehdr_start - 4\n\nSymbol table '.symtab' contains 7 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n     5: 0000000000000000     8 FUNC    GLOBAL DEFAULT    4 liella_link_image_header\n     6: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   UND __ehdr_start\n"
            )
        };
        let other = "\nFile: libc.a(csys_2.o)\n\nSymbol table '.symtab' contains 2 entries:\n     1: 0000000000000000     8 FUNC    GLOBAL DEFAULT    4 liella_link_ehdr\n";
        assert_eq!(boot_anchor_errors(&format!("{other}{}", member("R_X86_64_PC32"))), Ok(()));
        let got = boot_anchor_errors(&format!("{other}{}", member("R_X86_64_REX_GOTPCRELX")));
        assert!(got.as_ref().is_err_and(|e| e.contains("through the GOT") && e.contains("csys_3.o")), "{got:?}");
        let unrelated = member("R_X86_64_PC32").replace("__ehdr_start - 4", "liella_other - 4");
        assert!(boot_anchor_errors(&unrelated).is_err_and(|e| e.contains("does not reference")));
        assert!(boot_anchor_errors(other).is_err_and(|e| e.contains("no member defines")));
        let undefined_only = member("R_X86_64_PC32").replace("FUNC    GLOBAL DEFAULT    4", "NOTYPE  GLOBAL DEFAULT  UND");
        assert!(boot_anchor_errors(&undefined_only).is_err(), "a reference to the anchor is not its definition");
    }

    /// D-27.10 (no spec clause: start-up internals): the static-PIE start object passes with
    /// PC-relative references to `main` and the relocating start, and fails with a GOT-typed
    /// one or when it calls `__libc_start_main` instead.
    #[test]
    fn static_pie_start_object_is_pc_relative() {
        let object = |main: &str, start: &str, target: &str| {
            format!(
                "\nRelocation section '.rela.text.startup._start' at offset 0x1c8 contains 2 entries:\n    Offset             Info             Type               Symbol's Value  Symbol's Name + Addend\n0000000000000013  0000000500000002 {main}      0000000000000000 main - 4\n0000000000000018  0000000600000004 {start}      0000000000000000 {target} - 4\n"
            )
        };
        assert_eq!(start_object_errors(&object("R_X86_64_PC32", "R_X86_64_PLT32", STATIC_PIE_START)), Ok(()));
        let got_main = start_object_errors(&object("R_X86_64_REX_GOTPCRELX", "R_X86_64_PLT32", STATIC_PIE_START));
        assert!(got_main.as_ref().is_err_and(|e| e.contains("through the GOT") && e.contains("main")), "{got_main:?}");
        assert!(start_object_errors(&object("R_X86_64_PC32", "R_X86_64_GOTPCRELX", STATIC_PIE_START)).is_err_and(|e| e.contains("through the GOT")));
        assert!(start_object_errors(&object("R_X86_64_PC32", "R_X86_64_PLT32", "__libc_start_main")).is_err_and(|e| e.contains("does not call")));
    }

    /// Planted mismatch: the binding check names a symbol whose binding differs from the table, in
    /// both directions, and passes the ones that agree.
    #[test]
    fn binding_check_catches_a_wrong_binding() {
        let scopes: BTreeMap<(String, String), String> = [(("fgetc", "GLIBC_2.2.5"), "WEAK"), (("fputc", "GLIBC_2.2.5"), "GLOBAL"), (("gets", "GLIBC_2.2.5"), "WEAK")]
            .into_iter()
            .map(|((n, v), s)| ((n.to_string(), v.to_string()), s.to_string()))
            .collect();
        let text = "Symbol table '.dynsym' contains 3 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n\
                    \x20   1: 0000000000001000    10 FUNC    GLOBAL DEFAULT   12 fgetc@@GLIBC_2.2.5\n\
                    \x20   2: 0000000000001010    10 FUNC    GLOBAL DEFAULT   12 fputc@@GLIBC_2.2.5\n\
                    \x20   3: 0000000000001020    10 FUNC    WEAK   DEFAULT   12 gets@@GLIBC_2.2.5\n\
                    \x20   4: 0000000000001030    10 FUNC    WEAK   DEFAULT   12 other@@GLIBC_2.2.5\n";
        let (errors, checked) = binding_errors(text, &scopes);
        assert_eq!(checked, 3);
        assert_eq!(errors, vec!["fgetc@GLIBC_2.2.5: GLOBAL in libc.so.6, glibc has WEAK".to_string()]);
        let wrong: BTreeMap<_, _> = [(("a".to_string(), "V".to_string()), "WEAK".to_string()), (("a".to_string(), "W".to_string()), "GLOBAL".to_string())].into();
        assert!(weak_names(&wrong).is_err());
    }

    /// Planted version-set mismatches (SO-CHECK-DEFAULT-VERSION): a lost default, a compat-only
    /// name that gained a default, a missing compat version, an extra version, a name the archive
    /// defines but the library does not export, and a name with no row; agreeing names and skipped
    /// (native) names pass.
    #[test]
    fn version_set_check_catches_lost_and_gained_defaults() {
        let t = |rows: &[(&str, &str, bool)]| {
            let mut m: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
            for (n, v, d) in rows {
                m.entry(n.to_string()).or_default().insert(v.to_string(), *d);
            }
            m
        };
        let table = t(&[
            ("fgetc", "GLIBC_2.2.5", true),
            ("cond", "GLIBC_2.3.2", true),
            ("cond", "GLIBC_2.2.5", false),
            ("stime", "GLIBC_2.2.5", false),
            ("two", "GLIBC_2.3.2", true),
            ("two", "GLIBC_2.2.5", false),
            ("extra", "GLIBC_2.2.5", true),
            ("gone", "GLIBC_2.2.5", true),
            ("dropped", "GLIBC_2.2.5", false),
            ("unbuilt", "GLIBC_2.2.5", true),
        ]);
        let text = "   Num:    Value          Size Type    Bind   Vis      Ndx Name\n\
                    \x20   1: 0000000000001000    10 FUNC    WEAK   DEFAULT   12 fgetc@@GLIBC_2.2.5\n\
                    \x20   2: 0000000000001010    10 FUNC    GLOBAL DEFAULT   12 cond@GLIBC_2.2.5\n\
                    \x20   3: 0000000000001020    10 FUNC    GLOBAL DEFAULT   12 stime@@GLIBC_2.2.5\n\
                    \x20   4: 0000000000001030    10 FUNC    GLOBAL DEFAULT   12 two@@GLIBC_2.3.2\n\
                    \x20   5: 0000000000001040    10 FUNC    GLOBAL DEFAULT   12 extra@@GLIBC_2.2.5\n\
                    \x20   6: 0000000000001040    10 FUNC    GLOBAL DEFAULT   12 extra@GLIBC_2.3\n\
                    \x20   7: 0000000000001050    10 FUNC    GLOBAL DEFAULT   12 norow@@GLIBC_2.2.5\n\
                    \x20   8: 0000000000001060    10 FUNC    GLOBAL DEFAULT   12 liella_x@@LIELLA_NATIVE_1\n\
                    \x20   9: 0000000000000000     0 OBJECT  GLOBAL DEFAULT  ABS GLIBC_2.2.5\n\
                    \x20  10: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND memcpy@GLIBC_2.14 (2)\n";
        let exported = exported_versions(text);
        let have: BTreeSet<String> = ["fgetc", "cond", "cond@GLIBC_2.2.5", "stime@GLIBC_2.2.5", "two", "two@GLIBC_2.2.5", "extra", "gone", "liella_x"].into_iter().map(String::from).collect();
        let skip: BTreeSet<String> = ["liella_x".to_string()].into();
        // A known gap (compat-missing.tsv) is not an error; the same gap unlisted is.
        let known: BTreeSet<(String, String)> = [("cond".to_string(), "GLIBC_2.9".to_string())].into();
        let (errors, checked) = version_set_errors("libc.so.6", &exported, &table, &have, &skip, &known);
        assert_eq!(checked, 8, "{errors:?}");
        assert_eq!(
            errors,
            vec![
                "cond: default cond@@GLIBC_2.3.2 missing".to_string(),
                "dropped: compat dropped@GLIBC_2.2.5 missing".to_string(),
                "extra: extra version extra@GLIBC_2.3 (not in versions.tsv)".to_string(),
                "gone: default gone@@GLIBC_2.2.5 missing".to_string(),
                "norow: exported by libc.so.6 as norow@@GLIBC_2.2.5 but has no libc.so.6 row in versions.tsv".to_string(),
                "stime: gained a default: stime@@GLIBC_2.2.5 exported, versions.tsv has only stime@GLIBC_2.2.5".to_string(),
                "two: compat two@GLIBC_2.2.5 missing".to_string(),
            ]
        );
        // A lost default: the default version exported without `@@`.
        let lost = exported_versions("     1: 0000000000001000    10 FUNC    WEAK   DEFAULT   12 fgetc@GLIBC_2.2.5\n");
        let (errors, _) = version_set_errors("libc.so.6", &lost, &table, &BTreeSet::new(), &BTreeSet::new(), &BTreeSet::new());
        // Compat-only names are compared even when nothing defines them (dropped, stime).
        assert_eq!(
            errors,
            vec![
                "dropped: compat dropped@GLIBC_2.2.5 missing".to_string(),
                "fgetc: lost its default: fgetc@GLIBC_2.2.5 exported, versions.tsv has fgetc@@GLIBC_2.2.5".to_string(),
                "stime: compat stime@GLIBC_2.2.5 missing".to_string(),
            ]
        );
        // The compat-missing.tsv ratchet: a listed version still missing passes; one now exported,
        // a listed default and a name with no row each fail.
        let table = t(&[("cond", "GLIBC_2.2.5", false), ("two", "GLIBC_2.2.5", false), ("fgetc", "GLIBC_2.2.5", true)]);
        let exported = exported_versions("     1: 0000000000001000    10 FUNC    GLOBAL DEFAULT   12 cond@GLIBC_2.2.5\n");
        let pair = |s: &str, v: &str| (s.to_string(), v.to_string());
        let known: BTreeSet<(String, String)> = [pair("two", "GLIBC_2.2.5")].into();
        let (libc_list, libm_list) = (compat_missing_list("libc.so.6").unwrap(), compat_missing_list("libm.so.6").unwrap());
        assert_eq!((libc_list, libm_list), ("spec/abi/compat-missing.tsv", "spec/abi/compat-missing-libm.tsv"));
        assert_eq!(compat_missing_list("libpthread.so.0"), None);
        assert!(stale_compat_missing(libc_list, &exported, &table, &known).is_empty());
        let known: BTreeSet<(String, String)> = [pair("cond", "GLIBC_2.2.5"), pair("fgetc", "GLIBC_2.2.5"), pair("nope", "GLIBC_2.2.5")].into();
        // The message names the library's own list (libm here).
        assert_eq!(
            stale_compat_missing(libm_list, &exported, &table, &known),
            vec![
                "cond@GLIBC_2.2.5: exported, remove it from spec/abi/compat-missing-libm.tsv".to_string(),
                "fgetc@GLIBC_2.2.5: listed in spec/abi/compat-missing-libm.tsv but not a compat version in versions.tsv".to_string(),
                "nope@GLIBC_2.2.5: listed in spec/abi/compat-missing-libm.tsv but not a compat version in versions.tsv".to_string(),
            ]
        );
    }

    /// The reference table is consistent: no name has both bindings, and `fgetc` (the symbol that
    /// started the ruling) is weak.
    #[test]
    fn reference_scopes_are_consistent() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let scopes = libc_scopes(&root).unwrap();
        assert!(weak_names(&scopes).unwrap().contains("fgetc"));
    }

    /// The marker is what keeps fast products out of the benchmarks: it must read back exactly
    /// the profile written, and anything else (absent, garbled, a half-built set) is unknown.
    #[test]
    fn built_profile_reads_the_marker() {
        let root = Dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/xtask-profile-test"));
        let live = root.0.join("target/liella");
        fs::create_dir_all(&live).unwrap();
        assert_eq!(built_profile(&root.0), None);
        for (text, want) in [("fast\n", Some(Profile::Fast)), ("release\n", Some(Profile::Release)), ("", None), ("fat\n", None)] {
            fs::write(live.join(".profile"), text).unwrap();
            assert_eq!(built_profile(&root.0), want, "marker {text:?}");
        }
        assert_ne!(Profile::Fast.name(), Profile::Release.name());
    }

    /// An `ar` archive of `(name, body)` members, as `ar` lays it out (60-byte headers, bodies padded to even).
    fn ar_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = b"!<arch>\n".to_vec();
        for (name, body) in members {
            out.extend(format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, 100644, body.len()).bytes());
            out.extend_from_slice(body);
            if body.len() % 2 == 1 {
                out.push(b'\n');
            }
        }
        out
    }

    /// XTASK-SO-ATOMIC: the product check passes ELF members (the symbol table and long-name members are
    /// not objects), and names a zeroed member, a truncated archive and a non-archive.
    #[test]
    fn archive_check_catches_a_zeroed_member() {
        let elf: &[u8] = b"\x7fELF-object-a";
        let good = ar_bytes(&[("/", &[0, 0, 0, 0]), ("//", b"long/\n"), ("a.o/", elf), ("b.o/", b"\x7fELFxyz")]);
        assert_eq!(archive_elf_errors(&good), Ok(()));
        assert_eq!(archive_elf_errors(&ar_bytes(&[])), Ok(()));
        let zeroed = ar_bytes(&[("a.o/", elf), ("b.o/", &[0u8; 24]), ("c.o/", &[0u8; 8])]);
        let err = archive_elf_errors(&zeroed).unwrap_err();
        assert!(err.contains("2 of 3 members are not ELF") && err.contains("b.o/"), "{err}");
        let cut = &good[..good.len() - 3];
        assert!(archive_elf_errors(cut).unwrap_err().contains("truncated"));
        assert!(archive_elf_errors(&[0u8; 64]).unwrap_err().contains("not an ar archive"));
    }

    /// XTASK-SO-ATOMIC: a second product lock on a directory waits for the first and gets it
    /// once the first is dropped; a product is renamed into place whole.
    #[test]
    fn product_lock_excludes_and_publish_renames() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let dir = Dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../target/xtask-lock-test-{}", std::process::id())));
        let first = lock_product_dir(&dir.0).unwrap();
        let got = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&got);
        let path = dir.0.clone();
        let waiter = std::thread::spawn(move || {
            let _second = lock_product_dir(&path).unwrap();
            flag.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!got.load(Ordering::SeqCst), "second lock taken while the first is held");
        drop(first);
        waiter.join().unwrap();
        assert!(got.load(Ordering::SeqCst));

        let dst = dir.0.join("libx.a");
        let tmp = tmp_name(&dst);
        assert_eq!(tmp.file_name().unwrap(), ".libx.a.tmp");
        fs::write(&dst, "old").unwrap();
        fs::write(&tmp, "new").unwrap();
        publish(&tmp, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
        assert!(!tmp.exists());
    }

    #[test]
    fn rust_std_definitions_are_found() {
        // `nm -g -C --defined-only` of an archive, as the unjoined fast libc.a printed it.
        let nm = "\nc-1.core-2.core.3-cgu.0.rcgu.o:\n\
                  0000000000000000 T <core::fmt::Formatter>::pad_integral\n\
                  0000000000000010 T core::str::count::do_count_chars\n\
                  0000000000000020 T <usize as core::fmt::Display>::fmt\n\
                  0000000000000030 W alloc::raw_vec::handle_error\n\
                  0000000000000040 T memcpy\n\
                  0000000000000050 T liella::bytes::copy\n\
                                   U core::panicking::panic\n";
        let found = exported_rust_std(nm);
        assert_eq!(found, ["<core::fmt::Formatter>::pad_integral", "core::str::count::do_count_chars", "<usize as core::fmt::Display>::fmt", "alloc::raw_vec::handle_error"]);
        assert!(exported_rust_std("x.o:\n0000000000000000 T memset\n0000000000000000 W liella_internal_memset\n").is_empty());
        let builtins = "compiler_builtins-1.compiler_builtins.2-cgu.000.rcgu.o:\n0000000000000000 T core::core_arch::x86::xsave::_xgetbv\n";
        assert!(exported_rust_std(builtins).is_empty());
        assert_eq!(exported_rust_std(&format!("{builtins}c-1.o:\n0000000000000000 T core::panicking::panic\n")), ["core::panicking::panic"]);
    }

    /// Design §1 / D-27.13 (no spec clause: loader start-up internals): over an `objdump -d`
    /// listing shaped like ld.so's, the pre-relocation check passes when nothing before the end of
    /// the relocation loop reads a relocated word (a later GOT call and taking a slot's address
    /// are fine), and fails for a read in a callee reached by a tail jump (unaligned inside the
    /// word), a read in the start before the relocator's call, an unfollowable call, and a start
    /// whose relocator call is gone (inlined).
    #[test]
    fn interp_prerelocation_reads_are_refused() {
        let clean = "\n\
            0000000000001000 <liella_machine_ld_entry>:\n    1000:\txor    %ebp,%ebp\n    1002:\tmov    %rsp,%rdi\n\
            \x20   1005:\tlea    -0x100c(%rip),%rsi        # 0 <__ehdr_start>\n    100c:\tcall   1100 <liella_ld_start>\n    1011:\thlt\n\n\
            0000000000001100 <liella_ld_start>:\n    1100:\tpush   %rbp\n    1101:\tlea    0x2f00(%rip),%rsi        # 4008 <ptr>\n\
            \x20   1108:\tcall   1200 <_RNvNtCs1_15liella_boundary9bootstrap14relocate_image>\n\
            \x20   110d:\tcall   *0x2eed(%rip)        # 4000 <_GLOBAL_OFFSET_TABLE_+0x18>\n    1113:\tret\n\n\
            0000000000001200 <_RNvNtCs1_15liella_boundary9bootstrap14relocate_image>:\n\
            \x20   1200:\tcmpb   $0x0,0x2ef9(%rip)        # 4100 <flag>\n\
            \x20   1207:\tje     1210 <_RNvNtCs1_15liella_boundary9bootstrap14relocate_image+0x10>\n\
            \x20   1209:\tnotrack jmp *%rax\n    120b:\taddr32 call 1300 <helper>\n    1210:\tret\n\n\
            0000000000001300 <helper>:\n    1300:\tjmp    1400 <tail>\n\n\
            0000000000001400 <tail>:\n    1400:\tret\n";
        let words: BTreeSet<u64> = [0x4000, 0x4008].into();
        assert_eq!(prerelocation_errors(clean, &words), Ok(()));
        let tail_read = clean.replace("1400:\tret", "1400:\tmov    0x2bfa(%rip),%rax        # 4004 <x>");
        assert!(prerelocation_errors(&tail_read, &words).is_err_and(|e| e.contains("tail 1400") && e.contains("reads a relocated word")));
        let start_read = clean.replace("1100:\tpush   %rbp", "1100:\tcall   *0x2efa(%rip)        # 4000 <_GLOBAL_OFFSET_TABLE_+0x18>");
        assert!(prerelocation_errors(&start_read, &words).is_err_and(|e| e.contains("liella_ld_start 1100")));
        let unfollowable = clean.replace("1300:\tjmp    1400 <tail>", "1300:\tcall   *%rax");
        assert!(prerelocation_errors(&unfollowable, &words).is_err_and(|e| e.contains("cannot follow")));
        let inlined = clean.replace("call   1200 <_RNvNtCs1_15liella_boundary9bootstrap14relocate_image>", "nop");
        assert!(prerelocation_errors(&inlined, &words).is_err_and(|e| e.contains("does not call relocate_image")));
    }
}
