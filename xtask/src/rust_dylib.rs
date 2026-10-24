//! `cargo xtask rust-dylib [<abi>]`: the Rust dylib `libliella_rust.so.<abi>` of a Rust ABI
//! version defined in `spec/abi/rust-abi.toml` (D-X.10; every field and command is explained
//! in docs/toolchain.md). Without `<abi>`, the version whose `status` is `current`.
//!
//! 1. The definition is read and its recorded `hash` recomputed (a mismatch means a field was
//!    changed in place: refused).
//! 2. The version's compiler is its **pinned toolchain**, `toolchain.rustup_toolchain` (for lr1
//!    the rustup toolchain `1.99.0`), installed beside the workspace's own: every rustc command
//!    below runs that toolchain's rustc (`rustup which --toolchain`), whatever
//!    `rust-toolchain.toml` and `stable` are. If it is not installed, the command fails and
//!    names the `rustup toolchain install` command. Its `rustc -vV` must report the version's
//!    `release`, `commit-hash`, `host` and `LLVM version` exactly, and `rustc --print cfg` its
//!    target features; otherwise the command refuses.
//! 3. `liella-native` with feature `dylib` passes clippy (`--all-targets`, so the program of
//!    step 5 is checked against the crate's rlib too).
//! 4. The crates of `[<abi>.crates]` are compiled with `rustc` directly (not Cargo, so the
//!    commands are exactly those docs/toolchain.md gives a third party), in their order, into
//!    `target/liella/rust/<abi>/`: rlibs, then `liella_native` as a `dylib` with
//!    `LIELLA_RUST_ABI` and `LIELLA_RUST_ABI_HASH` in its environment, linked by GNU ld
//!    against Liella's `libc.so.6`. The result is renamed to the version's file name
//!    (`libliella_rust.so.<abi>`), and `libliella_native.so`, the name `--extern` needs, is
//!    made a symbolic link to it.
//! 5. The program `crates/native/examples/native_rust_dylib.rs` is compiled against it
//!    (`--extern liella_native=.../libliella_native.so`), linked like the dynamic example.
//! 6. `readelf` checks the dylib: its soname and `DT_NEEDED` (only `libc.so.6`); the note
//!    `.note.liella.rustabi` (owner `Liella`, type 0x4c520001, the identifier and the hash);
//!    the exported `LIELLA_RUST_ABI` (an object holding the identifier and a NUL); every
//!    native function with a safe use referenced at its node, none newer than the version's
//!    `native_c_abi`. And the program: `DT_NEEDED` exactly the version's file and
//!    `libc.so.6`, its references to native functions (if rustc inlined any into it) at their
//!    nodes, `LIELLA_RUST_ABI` referenced, no allocator of its own. The program is not run:
//!    that needs Liella's dynamic loader (phase 14).
//! 7. Coexistence, for the current version: a synthetic variant `lr999-test` (the version's
//!    tables renamed, `opt_level` changed, the hash recomputed; written to
//!    `target/liella/rust-test/rust-abi.toml`) is built the same way into
//!    `target/liella/rust-test/`, with its own program, and checked the same way. Both files
//!    are then placed side by side in `target/liella/rust-test/lib`: their sonames, notes and
//!    strings must differ, and the only dynamic symbols both define may be `LIELLA_RUST_ABI`
//!    and the toolchain's own crates (`core`, `alloc`, `compiler_builtins`, the allocator
//!    shims), never one of Liella's crates (their names carry `-C metadata`, the identifier).
//!    The variant uses the same toolchain: it shows that versions coexist, not that a second
//!    toolchain works.
//!
//! `cargo xtask test` runs this command as a gate step, the same way: it always builds and
//! checks the current version with its pinned toolchain, and fails if that is absent.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFINITION: &str = "spec/abi/rust-abi.toml";
const NOTE_TYPE: &str = "0x4c520001";
const EXAMPLE: &str = "crates/native/examples/native_rust_dylib.rs";
const VARIANT: &str = "lr999-test";

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

fn output(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{cmd:?} failed: {}\n{}", out.status, String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ------------------------------------------------------------------------------ definition

/// One version's fields: (table, key, value as written), in file order.
#[derive(Clone)]
pub struct Def {
    pub id: String,
    entries: Vec<(String, String, String)>,
}

/// Remove a comment (`#` outside a string) and surrounding blanks.
fn uncomment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return line[..i].trim(),
            _ => {}
        }
    }
    line.trim()
}

/// The contents of a string value without escapes (`"..."`), or `None`.
fn string_value(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains(['"', '\\']) { None } else { Some(inner) }
}

/// The strings of a one-line array value (`["a", "b,c"]`), or `None`.
fn array_value(v: &str) -> Option<Vec<String>> {
    let mut rest = v.strip_prefix('[')?.strip_suffix(']')?.trim();
    let mut out = Vec::new();
    while !rest.is_empty() {
        let body = rest.strip_prefix('"')?;
        let end = body.find('"')?;
        out.push(string_value(&rest[..end + 2])?.to_string());
        rest = body[end + 1..].trim_start();
        match rest.strip_prefix(',') {
            Some(r) => rest = r.trim_start(),
            None if rest.is_empty() => {}
            None => return None,
        }
    }
    Some(out)
}

/// Is `v` a value of the subset: a string without escapes, an integer, a boolean, or a
/// one-line array of such strings?
fn valid_value(v: &str) -> bool {
    string_value(v).is_some() || array_value(v).is_some() || v == "true" || v == "false" || (!v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
}

/// Parse the definitions: one per top-level table, in file order.
pub fn parse(text: &str, origin: &str) -> Result<Vec<Def>, String> {
    let mut defs: Vec<Def> = Vec::new();
    let mut table = String::new();
    for (n, raw) in text.lines().enumerate() {
        let line = uncomment(raw);
        let at = || format!("{origin}:{}", n + 1);
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            let name = name.trim();
            let (version, part) = name.split_once('.').unwrap_or((name, ""));
            if !version.starts_with("lr") || version.len() > 31 || !version.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
                return Err(format!("{}: table [{name}]: a version is lr<N> (a-z, 0-9, -; at most 31 bytes)", at()));
            }
            if part.is_empty() {
                if defs.iter().any(|d| d.id == version) {
                    return Err(format!("{}: version {version} defined twice", at()));
                }
                defs.push(Def { id: version.to_string(), entries: Vec::new() });
            } else if defs.last().map(|d| d.id.as_str()) != Some(version) {
                return Err(format!("{}: table [{name}] outside its version's tables", at()));
            }
            table = name.to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("{}: expected `key = value` or `[table]`", at()));
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || !valid_value(value) {
            return Err(format!("{}: `{line}` is outside the format (see the file's header)", at()));
        }
        let Some(def) = defs.last_mut() else {
            return Err(format!("{}: a field before the first table", at()));
        };
        if def.entries.iter().any(|(t, k, _)| *t == table && k == key) {
            return Err(format!("{}: {table}.{key} given twice", at()));
        }
        def.entries.push((table.clone(), key.to_string(), value.to_string()));
    }
    Ok(defs)
}

/// FNV-1a, 64 bits.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

impl Def {
    /// The raw value of `path` (`identifier`, `toolchain.rustc_release`, ...).
    fn raw(&self, path: &str) -> Result<&str, String> {
        let (table, key) = match path.rsplit_once('.') {
            Some((part, key)) => (format!("{}.{part}", self.id), key),
            None => (self.id.clone(), path),
        };
        self.entries.iter().find(|(t, k, _)| *t == table && k == key).map(|(_, _, v)| v.as_str()).ok_or_else(|| format!("{DEFINITION}: {}.{path} is missing", self.id))
    }

    pub fn string(&self, path: &str) -> Result<String, String> {
        string_value(self.raw(path)?).map(str::to_string).ok_or_else(|| format!("{}.{path}: expected a string", self.id))
    }

    pub fn strings(&self, path: &str) -> Result<Vec<String>, String> {
        array_value(self.raw(path)?).ok_or_else(|| format!("{}.{path}: expected an array of strings", self.id))
    }

    /// A boolean or an integer, as rustc writes it (`on`/`off` for booleans).
    fn flag(&self, path: &str) -> Result<String, String> {
        match self.raw(path)? {
            "true" => Ok("on".into()),
            "false" => Ok("off".into()),
            v if v.bytes().all(|b| b.is_ascii_digit()) => Ok(v.to_string()),
            v => Err(format!("{}.{path}: `{v}` is not a boolean or an integer", self.id)),
        }
    }

    /// The canonical form the hash is computed over.
    pub fn canonical(&self) -> String {
        let mut lines: Vec<String> = self
            .entries
            .iter()
            .filter(|(t, k, _)| !(*t == self.id && (k == "hash" || k == "status")))
            .map(|(t, k, v)| format!("{t}.{k} = {v}\n"))
            .collect();
        lines.sort();
        lines.concat()
    }

    pub fn hash(&self) -> String {
        format!("{:016x}", fnv1a(self.canonical().as_bytes()))
    }

    /// The definition as text (for a derived version).
    fn to_toml(&self) -> String {
        let mut out = String::new();
        let mut table = "";
        for (t, k, v) in &self.entries {
            if t != table {
                out.push_str(&format!("{}[{t}]\n", if out.is_empty() { "" } else { "\n" }));
                table = t;
            }
            out.push_str(&format!("{k} = {v}\n"));
        }
        out
    }

    fn set(&mut self, path: &str, value: &str) {
        let (table, key) = match path.rsplit_once('.') {
            Some((part, key)) => (format!("{}.{part}", self.id), key.to_string()),
            None => (self.id.clone(), path.to_string()),
        };
        for e in &mut self.entries {
            if e.0 == table && e.1 == key {
                e.2 = value.to_string();
            }
        }
    }

    /// Its consistency: the identifier, the names derived from it, the hash.
    fn check(&self, root: &Path) -> Result<(), String> {
        let id = &self.id;
        let mut errors = Vec::new();
        let want = |path: &str, value: String, errors: &mut Vec<String>| match self.string(path) {
            Ok(v) if v == value => {}
            Ok(v) => errors.push(format!("{id}.{path} is \"{v}\", expected \"{value}\"")),
            Err(e) => errors.push(e),
        };
        want("identifier", id.clone(), &mut errors);
        want("codegen.metadata", id.clone(), &mut errors);
        want("product.file", format!("libliella_rust.so.{id}"), &mut errors);
        want("product.soname", format!("libliella_rust.so.{id}"), &mut errors);
        want("product.link_name", "libliella_native.so".into(), &mut errors);
        want("product.note_section", ".note.liella.rustabi".into(), &mut errors);
        want("product.note_owner", "Liella".into(), &mut errors);
        want("product.note_type", NOTE_TYPE.into(), &mut errors);
        want("product.symbol", "LIELLA_RUST_ABI".into(), &mut errors);
        match self.string("status") {
            Ok(s) if ["current", "supported", "retired"].contains(&s.as_str()) || s.starts_with("synthetic") => {}
            Ok(s) => errors.push(format!("{id}.status \"{s}\": expected current, supported or retired")),
            Err(e) => errors.push(e),
        }
        let nodes: BTreeSet<String> = crate::native::symbols(root)?.into_iter().map(|(_, n)| n).collect();
        match self.string("native_c_abi") {
            Ok(n) if nodes.contains(&n) => {}
            Ok(n) => errors.push(format!("{id}.native_c_abi {n} is not a node of spec/abi/native.tsv")),
            Err(e) => errors.push(e),
        }
        let recorded = self.string("hash")?;
        let computed = self.hash();
        if recorded != computed {
            errors.push(format!("{id}.hash is {recorded}, but the fields hash to {computed}: a defined version is never changed (define a new one); a new version records the computed value"));
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }
}

// ------------------------------------------------------------------------------ toolchain

/// The rustc of `def`'s pinned toolchain (`toolchain.rustup_toolchain`), installed with rustup
/// beside the workspace's own (`rust-toolchain.toml`). An absent toolchain is an error naming
/// the command that installs it; it is never installed here, and no other rustc is used.
fn pinned_rustc(root: &Path, def: &Def) -> Result<String, String> {
    let name = def.string("toolchain.rustup_toolchain")?;
    let host = def.string("toolchain.rustc_host")?;
    let full = format!("{name}-{host}");
    let install = format!("rustup toolchain install {name} --profile minimal --target {}", def.string("target.triple")?);
    // `rustup toolchain list` only reads: `rustup which` would install a missing toolchain.
    let list = output(Command::new("rustup").current_dir(root).args(["toolchain", "list"]))
        .map_err(|e| format!("Rust ABI version {} is built only by its pinned toolchain {name}, through rustup, which cannot be run: {e}\nInstall rustup, then: {install}", def.id))?;
    if !list.lines().any(|l| l.split_whitespace().next() == Some(full.as_str())) {
        return Err(format!(
            "Rust ABI version {} is built only by its pinned toolchain {full}, which is not installed.\nInstall it with: {install}\n(The workspace's own toolchain, rust-toolchain.toml, is not used for the dylib and is not affected.)",
            def.id
        ));
    }
    let path = output(Command::new("rustup").current_dir(root).env("RUSTUP_AUTO_INSTALL", "0").args(["which", "--toolchain", &full, "rustc"]))?;
    Ok(path.trim().to_string())
}

/// `rustc -vV`'s fields.
fn rustc_version(root: &Path, rustc: &str) -> Result<BTreeMap<String, String>, String> {
    let text = output(Command::new(rustc).current_dir(root).arg("-vV"))?;
    Ok(text.lines().filter_map(|l| l.split_once(": ")).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect())
}

/// Does the running compiler build `def`? `Err` names every difference.
fn check_toolchain(root: &Path, rustc: &str, def: &Def) -> Result<(), String> {
    let have = rustc_version(root, rustc)?;
    let mut diffs = Vec::new();
    for (field, key) in [("toolchain.rustc_release", "release"), ("toolchain.rustc_commit_hash", "commit-hash"), ("toolchain.rustc_host", "host"), ("toolchain.llvm_version", "LLVM version")] {
        let want = def.string(field)?;
        let got = have.get(key).cloned().unwrap_or_default();
        if want != got {
            diffs.push(format!("{key}: the pinned toolchain's rustc has `{got}`, {} requires `{want}`", def.id));
        }
    }
    if diffs.is_empty() {
        let mut cmd = Command::new(rustc);
        cmd.current_dir(root).args(["--print", "cfg", "--target"]).arg(def.string("target.triple")?).arg(format!("-Ctarget-cpu={}", def.string("target.target_cpu")?));
        let flags = def.strings("target.target_feature_flags")?;
        if !flags.is_empty() {
            cmd.arg(format!("-Ctarget-feature={}", flags.join(",")));
        }
        let cfg = output(&mut cmd)?;
        let got: BTreeSet<String> = cfg.lines().filter_map(|l| l.strip_prefix("target_feature=\"")?.strip_suffix('"').map(str::to_string)).collect();
        let want: BTreeSet<String> = def.strings("target.target_features")?.into_iter().collect();
        if got != want {
            diffs.push(format!("target features: the pinned toolchain's rustc gives {got:?}, {} requires {want:?}", def.id));
        }
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the pinned toolchain does not match Rust ABI version {}:\n  {}\nA Rust dylib is compatible only with programs of the exact toolchain that built it, so {} is never rebuilt by another one. The rustup toolchain {} is not the compiler {} records; reinstall it (rustup toolchain uninstall {}; then rustup toolchain install {} --profile minimal), or correct the definition only if {} was never released (docs/toolchain.md, \"Introducing a new Rust ABI version\").",
            def.id,
            diffs.join("\n  "),
            def.id,
            def.string("toolchain.rustup_toolchain")?,
            def.id,
            def.string("toolchain.rustup_toolchain")?,
            def.string("toolchain.rustup_toolchain")?,
            def.id
        ))
    }
}

// ------------------------------------------------------------------------------ build

/// What a build produced.
struct Built {
    dir: PathBuf,
    file: PathBuf,
    program: PathBuf,
    commands: usize,
}

/// The codegen options every crate of `def` is compiled with.
fn codegen(def: &Def) -> Result<Vec<String>, String> {
    let mut a = vec!["--edition".to_string(), def.string("codegen.edition")?, "--target".into(), def.string("target.triple")?];
    let mut c = |o: String| {
        a.push("-C".into());
        a.push(o);
    };
    c(format!("opt-level={}", def.string("codegen.opt_level")?));
    c(format!("debug-assertions={}", def.flag("codegen.debug_assertions")?));
    c(format!("overflow-checks={}", def.flag("codegen.overflow_checks")?));
    c(format!("panic={}", def.string("codegen.panic")?));
    c(format!("codegen-units={}", def.flag("codegen.codegen_units")?));
    c(format!("lto={}", def.string("codegen.lto")?));
    c(format!("debuginfo={}", def.string("codegen.debuginfo")?));
    c(format!("target-cpu={}", def.string("target.target_cpu")?));
    let flags = def.strings("target.target_feature_flags")?;
    if !flags.is_empty() {
        c(format!("target-feature={}", flags.join(",")));
    }
    Ok(a)
}

/// Build `def` into `dir` (emptied first) and its program; `libc_so`, `nounwind` and `driver`
/// are the link inputs.
fn build(root: &Path, rustc: &str, def: &Def, dir: &Path, libc_so: &Path, nounwind: &Path, driver: &Path) -> Result<Built, String> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let common = codegen(def)?;
    let order = def.strings("crates.order")?;
    let Some(last) = order.last() else {
        return Err(format!("{}.crates.order is empty", def.id));
    };
    let mut commands = 0;
    for name in &order {
        let spec = def.strings(&format!("crates.{name}"))?;
        let Some((src, deps)) = spec.split_first() else {
            return Err(format!("{}.crates.{name} is empty", def.id));
        };
        let mut cmd = Command::new(rustc);
        cmd.current_dir(root).args(["--crate-name", name]).arg(src).args(&common);
        cmd.arg("-C").arg(format!("relocation-model={}", def.string("target.relocation_model")?));
        cmd.arg("-C").arg(format!("code-model={}", def.string("target.code_model")?));
        cmd.arg("-C").arg(format!("metadata={}", def.string("codegen.metadata")?));
        cmd.arg(format!("--remap-path-prefix={}={}", root.display(), def.string("codegen.remap_path_prefix")?));
        cmd.arg("--out-dir").arg(dir).arg("-L").arg(format!("dependency={}", dir.display()));
        for d in deps {
            cmd.arg("--extern").arg(format!("{d}={}", dir.join(format!("lib{d}.rlib")).display()));
        }
        if name == last {
            cmd.args(["--crate-type", "dylib"]);
            for f in def.strings(&format!("crates.{name}_features"))? {
                cmd.arg("--cfg").arg(format!("feature=\"{f}\""));
            }
            cmd.env("LIELLA_RUST_ABI", &def.id).env("LIELLA_RUST_ABI_HASH", def.string("hash")?);
            cmd.arg("-C").arg(format!("linker={}", def.string("toolchain.linker")?));
            cmd.arg("-C").arg(format!("linker-features={}", def.string("toolchain.linker_features")?));
            for a in def.strings("product.link_args")? {
                let a = match a.as_str() {
                    "nounwind.o" => nounwind.display().to_string(),
                    "libc.so.6" => libc_so.display().to_string(),
                    _ => a,
                };
                cmd.arg("-C").arg(format!("link-arg={a}"));
            }
        } else {
            cmd.args(["--crate-type", "rlib"]);
        }
        run(&mut cmd)?;
        commands += 1;
    }
    // The file under its version's name, and the name --extern needs as a link to it.
    let file = dir.join(def.string("product.file")?);
    let link = dir.join(def.string("product.link_name")?);
    fs::rename(&link, &file).map_err(|e| format!("rename {} to {}: {e}", link.display(), file.display()))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(def.string("product.file")?, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    #[cfg(not(unix))]
    fs::copy(&file, &link).map_err(|e| format!("{}: {e}", link.display()))?;

    // The program.
    let program = dir.join("native_rust_dylib");
    let mut cmd = Command::new(rustc);
    cmd.current_dir(root).args(["--crate-name", "native_rust_dylib"]).arg(EXAMPLE).args(["--crate-type", "bin"]).args(&common);
    cmd.arg("--extern").arg(format!("liella_native={}", link.display())).arg("-L").arg(format!("dependency={}", dir.display()));
    cmd.arg("-C").arg(format!("linker={}", driver.display()));
    cmd.arg("-C").arg(format!("linker-features={}", def.string("toolchain.linker_features")?));
    for a in ["-nostartfiles".to_string(), "-nostdlib".into(), nounwind.display().to_string(), libc_so.display().to_string(), "-lgcc".into()] {
        cmd.arg("-C").arg(format!("link-arg={a}"));
    }
    cmd.arg("-o").arg(&program);
    run(&mut cmd)?;
    commands += 1;
    Ok(Built { dir: dir.to_path_buf(), file, program, commands })
}

// ------------------------------------------------------------------------------ checks

/// `DT_NEEDED` entries and the soname.
fn dynamic(path: &Path) -> Result<(Vec<String>, Option<String>), String> {
    let text = output(Command::new("readelf").arg("-dW").arg(path))?;
    let value = |l: &str| l.split('[').nth(1).and_then(|r| r.split(']').next()).map(str::to_string);
    let needed = text.lines().filter(|l| l.contains("(NEEDED)")).filter_map(value).collect();
    let soname = text.lines().find(|l| l.contains("(SONAME)")).and_then(value);
    Ok((needed, soname))
}

/// The note `.note.liella.rustabi`: owner, type and descriptor bytes.
fn note(path: &Path) -> Result<(String, String, Vec<u8>), String> {
    let text = output(Command::new("readelf").arg("-nW").arg(path))?;
    let mut lines = text.lines().skip_while(|l| !l.contains("notes found in: .note.liella.rustabi"));
    if lines.next().is_none() {
        return Err(format!("{}: no section .note.liella.rustabi", path.display()));
    }
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.first() == Some(&"Owner") {
            continue;
        }
        let owner = f.first().map(|s| s.to_string()).unwrap_or_default();
        let kind = l.split("note type: (").nth(1).and_then(|r| r.split(')').next()).unwrap_or("").to_string();
        let desc = l.split("description data:").nth(1).unwrap_or("");
        let bytes = desc.split_whitespace().map(|h| u8::from_str_radix(h, 16).map_err(|e| format!("readelf -n: `{h}`: {e}"))).collect::<Result<Vec<u8>, String>>()?;
        return Ok((owner, kind, bytes));
    }
    Err(format!("{}: .note.liella.rustabi holds no note", path.display()))
}

/// A dynamic symbol, as `readelf --dyn-syms` shows it.
struct Sym {
    name: String,
    /// The version node, or empty.
    version: String,
    defined: bool,
    kind: String,
    size: u64,
    value: u64,
}

/// The dynamic symbols of `path`.
fn dyn_syms(path: &Path) -> Result<Vec<Sym>, String> {
    let text = output(Command::new("readelf").args(["--dyn-syms", "-W"]).arg(path))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (Some(value), Some(size), Some(ty), Some(ndx), Some(sym)) = (f.get(1), f.get(2), f.get(3), f.get(6), f.get(7)) else { continue };
        // Symbol rows only (`<n>: <hex value> ...`), not the column titles.
        let Ok(value) = u64::from_str_radix(value, 16) else { continue };
        if !f.first().is_some_and(|n| n.ends_with(':')) {
            continue;
        }
        let (name, ver) = sym.split_once('@').map(|(n, v)| (n, v.trim_start_matches('@'))).unwrap_or((sym, ""));
        let size = size.parse::<u64>().or_else(|_| u64::from_str_radix(size.trim_start_matches("0x"), 16)).unwrap_or(0);
        out.push(Sym { name: name.to_string(), version: ver.to_string(), defined: *ndx != "UND", kind: (*ty).to_string(), size, value });
    }
    Ok(out)
}

/// `len` bytes at virtual address `addr` of the file, through its `LOAD` segments.
fn read_at(path: &Path, addr: u64, len: u64) -> Result<Vec<u8>, String> {
    let text = output(Command::new("readelf").arg("-lW").arg(path))?;
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.first() != Some(&"LOAD") {
            continue;
        }
        let num = |i: usize| f.get(i).and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
        let (offset, vaddr, filesz) = (num(1), num(2), num(4));
        if addr >= vaddr && addr.saturating_add(len) <= vaddr.saturating_add(filesz) {
            let start = usize::try_from(offset + (addr - vaddr)).map_err(|e| e.to_string())?;
            let end = start + usize::try_from(len).map_err(|e| e.to_string())?;
            return data.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| format!("{}: {addr:#x} outside the file", path.display()));
        }
    }
    Err(format!("{}: {addr:#x} is in no LOAD segment", path.display()))
}

/// Every reference to a native function at its node, the newest no newer than `newest`.
/// The functions referenced.
fn native_refs(root: &Path, what: &str, syms: &[Sym], newest: &str, errors: &mut Vec<String>) -> Result<BTreeSet<String>, String> {
    let table: BTreeMap<String, String> = crate::native::symbols(root)?.into_iter().collect();
    let order = crate::native::node_order(&table.values().cloned().collect());
    let rank = |n: &str| order.iter().position(|o| o == n);
    let mut refs = BTreeSet::new();
    for Sym { name, version: ver, defined, .. } in syms {
        if *defined || !name.starts_with("liella_") {
            continue;
        }
        match table.get(name) {
            Some(node) if node == ver => {
                if rank(node) > rank(newest) {
                    errors.push(format!("{what}: references {name}@{node}, newer than the version's native_c_abi {newest}"));
                }
                refs.insert(name.clone());
            }
            Some(node) => errors.push(format!("{what}: references {name}@{ver}, expected {name}@{node}")),
            None => errors.push(format!("{what}: references {name}, which is not a native function")),
        }
    }
    Ok(refs)
}

/// Check a build of `def`: the dylib and the program. A summary line.
fn check(root: &Path, def: &Def, built: &Built) -> Result<String, String> {
    let mut errors = Vec::new();
    let id = &def.id;
    let file = &built.file;
    let name = def.string("product.file")?;
    let what = name.as_str();

    // The dylib: soname, needed.
    let (needed, soname) = dynamic(file)?;
    if soname.as_deref() != Some(def.string("product.soname")?.as_str()) {
        errors.push(format!("{what}: soname {soname:?}, expected {}", def.string("product.soname")?));
    }
    if needed != def.strings("product.needed")? {
        errors.push(format!("{what}: DT_NEEDED {needed:?}, expected {:?}", def.strings("product.needed")?));
    }
    // The note.
    let (owner, kind, desc) = note(file)?;
    let want_desc = format!("{id}\0{}\0", def.string("hash")?).into_bytes();
    if owner != "Liella" || kind != NOTE_TYPE || desc != want_desc {
        errors.push(format!("{what}: note owner {owner:?} type {kind:?} descriptor {:?}, expected Liella {NOTE_TYPE} {:?}", String::from_utf8_lossy(&desc), String::from_utf8_lossy(&want_desc)));
    }
    // The exported string.
    let syms = dyn_syms(file)?;
    let symbol = syms.iter().find(|s| s.name == "LIELLA_RUST_ABI" && s.defined);
    let mut text = String::new();
    match symbol {
        Some(Sym { kind: ty, size, value, .. }) => {
            let bytes = read_at(file, *value, *size)?;
            text = String::from_utf8_lossy(&bytes).replace('\0', "\\0");
            if ty != "OBJECT" || bytes != format!("{id}\0").into_bytes() {
                errors.push(format!("{what}: LIELLA_RUST_ABI is {ty} \"{text}\", expected OBJECT \"{id}\\0\""));
            }
        }
        None => errors.push(format!("{what}: does not export LIELLA_RUST_ABI")),
    }
    // Its native references: every native function with a safe use.
    let newest = def.string("native_c_abi")?;
    let refs = native_refs(root, what, &syms, &newest, &mut errors)?;
    let want: BTreeSet<String> = crate::native::symbols(root)?.into_iter().map(|(s, _)| s).filter(|s| !crate::native_examples::UNUSED.contains(&s.as_str())).collect();
    for s in want.difference(&refs) {
        errors.push(format!("{what}: does not reference {s}"));
    }

    // The program.
    let prog = "native_rust_dylib";
    let (needed, _) = dynamic(&built.program)?;
    let want_needed: BTreeSet<String> = def.strings("consumer.links")?.into_iter().collect();
    if needed.iter().cloned().collect::<BTreeSet<_>>() != want_needed || needed.len() != want_needed.len() {
        errors.push(format!("{prog}: DT_NEEDED {needed:?}, expected {want_needed:?}"));
    }
    let psyms = dyn_syms(&built.program)?;
    let prefs = native_refs(root, prog, &psyms, &newest, &mut errors)?;
    if !psyms.iter().any(|s| s.name == "LIELLA_RUST_ABI" && !s.defined) {
        errors.push(format!("{prog}: does not reference LIELLA_RUST_ABI"));
    }
    if psyms.iter().any(|s| s.defined && s.name.contains("__rust_alloc")) {
        errors.push(format!("{prog}: defines an allocator symbol; the dylib's must be the only one"));
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let own = if prefs.is_empty() { "no native references of its own: its calls go through the dylib".to_string() } else { format!("{} native references of its own (inlined), each at its node", prefs.len()) };
    Ok(format!(
        "{id}: {} (soname {what}, NEEDED libc.so.6; note Liella {NOTE_TYPE} \"{id}\" hash {}; LIELLA_RUST_ABI = \"{text}\"; references {} native functions, each at its node, the newest {newest}); its program: NEEDED {}, {own}",
        file.strip_prefix(root).unwrap_or(file).display(),
        def.string("hash")?,
        refs.len(),
        needed.join(", "),
    ))
}

/// Two builds side by side: different sonames, notes and symbols, no common symbol of
/// Liella's crates.
fn coexist(root: &Path, a: (&Def, &Built), b: (&Def, &Built), lib: &Path) -> Result<String, String> {
    let _ = fs::remove_dir_all(lib);
    fs::create_dir_all(lib).map_err(|e| format!("{}: {e}", lib.display()))?;
    let mut placed = Vec::new();
    for (def, built) in [a, b] {
        let to = lib.join(def.string("product.file")?);
        fs::copy(&built.file, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        placed.push(to);
    }
    let mut errors = Vec::new();
    let (sa, sb) = (dynamic(&placed[0])?.1, dynamic(&placed[1])?.1);
    if sa == sb {
        errors.push(format!("both files have the soname {sa:?}"));
    }
    let (na, nb) = (note(&placed[0])?.2, note(&placed[1])?.2);
    if na == nb {
        errors.push("both files carry the same note".into());
    }
    let defined = |p: &Path| -> Result<BTreeSet<String>, String> { Ok(dyn_syms(p)?.into_iter().filter(|s| s.defined).map(|s| s.name).collect()) };
    let (da, db) = (defined(&placed[0])?, defined(&placed[1])?);
    // A symbol both define may only be the version string or code of the toolchain's own
    // crates (v0 names carry the crate name: `4core`, `5alloc`, `17compiler_builtins`, and
    // `7___rustc` for the allocator shims), which the same toolchain compiles identically.
    let common: Vec<&String> = da.intersection(&db).collect();
    let toolchain = |s: &str| s == "LIELLA_RUST_ABI" || ["4core", "5alloc", "17compiler_builtins", "7___rustc"].iter().any(|c| s.contains(c));
    let other: Vec<&&String> = common.iter().filter(|s| !toolchain(s.as_str())).collect();
    if let Some(first) = other.first() {
        errors.push(format!("{} symbols defined by both that are not the toolchain's, for example {first}", other.len()));
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(format!(
        "{} and {} side by side in {}: different sonames, notes and LIELLA_RUST_ABI; {} of their {} and {} defined symbols in common, all LIELLA_RUST_ABI or the toolchain's core, alloc and allocator shims, none of Liella's crates",
        a.0.id,
        b.0.id,
        lib.strip_prefix(root).unwrap_or(lib).display(),
        common.len(),
        da.len(),
        db.len()
    ))
}

// ------------------------------------------------------------------------------ command

/// Read the definitions, checked.
pub fn definitions(root: &Path) -> Result<Vec<Def>, String> {
    let text = fs::read_to_string(root.join(DEFINITION)).map_err(|e| format!("{DEFINITION}: {e}"))?;
    let defs = parse(&text, DEFINITION)?;
    for d in &defs {
        d.check(root)?;
    }
    let current = defs.iter().filter(|d| d.string("status").is_ok_and(|s| s == "current")).count();
    if current != 1 {
        return Err(format!("{DEFINITION}: {current} versions have status \"current\"; exactly one must"));
    }
    Ok(defs)
}

/// The synthetic variant of `def`: renamed to [`VARIANT`], `opt_level` changed, rehashed.
fn variant(def: &Def) -> Def {
    let mut v = def.clone();
    v.id = VARIANT.to_string();
    for (t, _, value) in &mut v.entries {
        if let Some(rest) = t.strip_prefix(&def.id) {
            *t = format!("{VARIANT}{rest}");
        }
        *value = value.replace(&def.id, VARIANT);
    }
    let opt = if def.string("codegen.opt_level").as_deref() == Ok("1") { "\"2\"" } else { "\"1\"" };
    v.set("codegen.opt_level", opt);
    v.set("status", &format!("\"synthetic test variant of {} (cargo xtask rust-dylib); never installed\"", def.id));
    let hash = format!("\"{}\"", v.hash());
    v.set("hash", &hash);
    v
}

/// Build and check version `which` (default: the current one) with its pinned toolchain.
pub fn run_rust_dylib(root: &Path, which: Option<&str>) -> Result<(), String> {
    let defs = definitions(root)?;
    let def = match which {
        Some(id) => defs.iter().find(|d| d.id == id).ok_or_else(|| format!("{DEFINITION} defines no version {id}"))?,
        None => defs.iter().find(|d| d.string("status").is_ok_and(|s| s == "current")).ok_or("no current version")?,
    };
    let rustc = pinned_rustc(root, def)?;
    check_toolchain(root, &rustc, def)?;
    let (_, libc_so, _) = crate::native_examples::products(root)?;
    let (_, driver, nounwind) = crate::native_examples::link_helpers(root)?;

    // The crate with feature `dylib` under clippy (the dylib's own root, and its program
    // checked against the rlib).
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(&cargo).current_dir(root).args(["clippy", "-q", "-p", "liella-native", "--all-targets", "--features", "dylib", "--", "-D", "warnings"]))?;

    let built = build(root, &rustc, def, &root.join("target/liella/rust").join(&def.id), &libc_so, &nounwind, &driver)?;
    let line = check(root, def, &built)?;
    println!("rust-dylib: {line}");

    // Coexistence, with a synthetic second version.
    if def.string("status").is_ok_and(|s| s == "current") {
        let test = root.join("target/liella/rust-test");
        let _ = fs::remove_dir_all(&test);
        fs::create_dir_all(&test).map_err(|e| format!("{}: {e}", test.display()))?;
        let v = variant(def);
        let text = format!("# @generated by `cargo xtask rust-dylib`: a synthetic variant of {} for the coexistence check. Never installed.\n\n{}", def.id, v.to_toml());
        fs::write(test.join("rust-abi.toml"), &text).map_err(|e| format!("rust-abi.toml: {e}"))?;
        let parsed = parse(&text, "target/liella/rust-test/rust-abi.toml")?;
        let Some(v) = parsed.first() else { return Err("the variant did not parse".into()) };
        v.check(root)?;
        let vbuilt = build(root, &rustc, v, &test.join(VARIANT), &libc_so, &nounwind, &driver)?;
        println!("rust-dylib: {}", check(root, v, &vbuilt)?);
        println!("rust-dylib: {}", coexist(root, (def, &built), (v, &vbuilt), &test.join("lib"))?);
    }
    println!(
        "rust-dylib: {} built with {} rustc commands into {}; its program is linked, not run: running it needs Liella's dynamic loader (phase 14), and the host's loader must never load Liella's libc.so.6",
        def.id,
        built.commands,
        built.dir.strip_prefix(root).unwrap_or(&built.dir).display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_reference_values() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn parse_and_canonical_form() {
        let text = "# c\n[lr1]\nidentifier = \"lr1\" # c\nstatus = \"current\"\nhash = \"x\"\n\n[lr1.product]\nlink_args = [\"-a\", \"-Wl,-x,y\"]\nn = 1\n";
        let defs = parse(text, "t").unwrap();
        assert_eq!(defs.len(), 1);
        let d = &defs[0];
        assert_eq!(d.strings("product.link_args").unwrap(), ["-a", "-Wl,-x,y"]);
        assert_eq!(d.string("identifier").unwrap(), "lr1");
        // `hash` and `status` are left out; lines sorted; values as written.
        assert_eq!(d.canonical(), "lr1.identifier = \"lr1\"\nlr1.product.link_args = [\"-a\", \"-Wl,-x,y\"]\nlr1.product.n = 1\n");
        assert_eq!(parse(&d.to_toml(), "t").unwrap()[0].canonical(), d.canonical());
    }

    #[test]
    fn parse_rejects_outside_the_format() {
        assert!(parse("[lr1]\na = 'single'\n", "t").is_err());
        assert!(parse("[lr1]\na = \"esc\\\"\"\n", "t").is_err());
        assert!(parse("[x1]\n", "t").is_err());
        assert!(parse("[lr1]\na = 1\na = 2\n", "t").is_err());
        assert!(parse("[lr1]\n[lr2.part]\n", "t").is_err());
    }
}
