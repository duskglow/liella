//! The verdict cache of the layout contract check (LAYOUT-CHECK-VERDICT-CACHE, pending decision
//! `records/pending/LAYOUT-CHECK-VERDICT-CACHE-1.toml`).
//!
//! `cargo xtask so` checks every contract of `layout/contracts.tsv` after the last product (D-X.48). On products
//! that did not change, the verdict cannot change either, so a PASS is stored under a key that covers everything the
//! check reads, and a later check with the same key prints the stored lines and skips the work:
//!
//! * the checker: the bytes of the running `xtask` binary (every check, its constants and `jcc_pad.rs`);
//! * the products: name and sha256 of every entry of the products directory, and the profile checked;
//! * the tree inputs: `layout/contracts.tsv`, every file a contract's rule names (`keys=`, `text-keys=`, ...), the
//!   padded assembly files whose sections the JCC check reads, and the start-up probe `page_faults.c` preprocessed
//!   by the compiler (so its system headers count too);
//! * the tools: `cc`, `readelf`, `objdump`, `nm` as found on `PATH`, the compiler's `cc1`, `as`, `ld`, `collect2` and
//!   `libgcc.a`, the shared libraries `ldd` names for each, and each tool's `--version`;
//! * the environment the checks or the tools read: the workspace and products paths (the build-path needles of
//!   PRODUCT-PATH-REMAP), `PATH`, `CC`, `HOME`, `CARGO_HOME`, the loader and compiler search variables, and every
//!   `LIELLA_*` variable.
//!
//! Only a pass is stored (`target/layout-verdicts/<key>`, with a sha256 of its own text); a failure is checked in
//! full every time. An entry that does not read back exactly (another key, a changed line, a missing or wrong sum) is
//! a miss, never a pass. Any error while computing the key is a full check. Under the CI train (`LIELLA_TRAIN` set,
//! not `0`) and with `LIELLA_LAYOUT_CACHE=0` the cache is neither read nor written.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sha256::{sha256_files, sha256_text};

/// Where the verdicts live, relative to the workspace root.
pub const DIR: &str = "target/layout-verdicts";
const MAGIC: &str = "liella layout verdict 1";
/// The probe the start-up kinds link (`layout::link_startup_probe`).
const PROBE: &str = "crates/core/tests/oracle/page_faults.c";
/// Environment variables the checks or their tools read (the `LIELLA_*` ones are added by prefix).
const ENV: &[&str] = &["PATH", "CC", "HOME", "CARGO_HOME", "LD_LIBRARY_PATH", "LD_PRELOAD", "GCC_EXEC_PREFIX", "COMPILER_PATH", "LIBRARY_PATH", "CPATH", "C_INCLUDE_PATH", "TMPDIR"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mode {
    /// Read and write the cache.
    Use,
    /// The CI train: always the full check.
    Train,
    /// `LIELLA_LAYOUT_CACHE=0`: always the full check.
    Off,
}

pub(crate) fn mode(env: &dyn Fn(&str) -> Option<String>) -> Mode {
    let set = |k: &str| env(k).is_some_and(|v| !v.is_empty() && v != "0");
    if set("LIELLA_TRAIN") {
        Mode::Train
    } else if env("LIELLA_LAYOUT_CACHE").is_some_and(|v| v == "0") {
        Mode::Off
    } else {
        Mode::Use
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// A stored pass for this key was served.
    Hit,
    /// The full check ran (and passed).
    Checked,
}

/// Serve the verdict: in `Mode::Use` the entry `entry()` names (path, key), if it reads back as a pass; otherwise
/// `full()` (the ok lines of a pass), stored afterwards in `Mode::Use`.
pub(crate) fn verdict(mode: Mode, entry: impl FnOnce() -> Result<(PathBuf, String), String>, full: impl FnOnce() -> Result<Vec<String>, String>) -> Result<Outcome, String> {
    let at = match mode {
        Mode::Train => {
            println!("layout: LIELLA_TRAIN is set: every contract checked in full (the verdict cache serves local builds only)");
            None
        }
        Mode::Off => {
            println!("layout: LIELLA_LAYOUT_CACHE=0: every contract checked in full");
            None
        }
        Mode::Use => match entry() {
            Err(e) => {
                println!("layout: verdict cache not usable ({e}): every contract checked in full");
                None
            }
            Ok((path, key)) => match read_entry(&path, &key) {
                Ok(Some(lines)) => {
                    println!("layout: verdict cache hit {key} ({}): the products, contract inputs, tools and checker are those of a full check that passed; check skipped (LIELLA_LAYOUT_CACHE=0 runs it)", path.display());
                    for l in &lines {
                        println!("{l}");
                    }
                    return Ok(Outcome::Hit);
                }
                Ok(None) => {
                    println!("layout: verdict cache miss {key}: every contract checked");
                    Some((path, key))
                }
                Err(why) => {
                    println!("layout: verdict cache entry {} is not a valid pass ({why}): a miss, every contract checked", path.display());
                    Some((path, key))
                }
            },
        },
    };
    let lines = full()?;
    if let Some((path, key)) = at {
        if let Err(e) = store(&path, &key, &lines) {
            println!("layout: verdict cache: could not store {} ({e}); the check passed", path.display());
        }
    }
    Ok(Outcome::Checked)
}

/// The entry text for a pass: magic, key, line count, the lines, and the sha256 of all that.
fn entry_text(key: &str, lines: &[String]) -> Result<String, String> {
    let mut t = format!("{MAGIC}\nkey {key}\ncontracts {}\n", lines.len());
    for l in lines {
        if l.contains('\n') {
            return Err("an ok line holds a newline".into());
        }
        t.push_str(l);
        t.push('\n');
    }
    let sum = sha256_text(&t)?;
    Ok(format!("{t}sum {sum}\n"))
}

fn store(path: &Path, key: &str, lines: &[String]) -> Result<(), String> {
    let text = entry_text(key, lines)?;
    let dir = path.parent().ok_or("no parent directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{key}.{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The stored ok lines of a pass under `key` (`Ok(None)`: no entry), or why the entry is not one.
fn read_entry(path: &Path, key: &str) -> Result<Option<Vec<String>>, String> {
    let text = match std::fs::read(path) {
        Ok(b) => String::from_utf8(b).map_err(|_| "not UTF-8".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let body = text.strip_suffix('\n').ok_or("no final newline")?;
    let (head, sum_line) = body.rsplit_once('\n').ok_or("too short")?;
    let sum = sum_line.strip_prefix("sum ").ok_or("no sum line")?;
    let lines: Vec<&str> = head.lines().collect();
    let [magic, key_line, count, ok @ ..] = lines.as_slice() else { return Err("too short".into()) };
    if *magic != MAGIC {
        return Err("not a verdict entry".into());
    }
    if key_line.strip_prefix("key ") != Some(key) {
        return Err("the entry is for another key".into());
    }
    if count.strip_prefix("contracts ").and_then(|n| n.parse::<usize>().ok()) != Some(ok.len()) || ok.is_empty() {
        return Err("the line count does not match".into());
    }
    if sha256_text(&format!("{head}\n"))? != sum {
        return Err("the sum does not match the text".into());
    }
    Ok(Some(ok.iter().map(|l| l.to_string()).collect()))
}

/// `name` resolved on `path` (a `PATH` value); a name with a slash as it is.
fn which(name: &str, path: &str) -> Result<PathBuf, String> {
    if name.contains('/') {
        return Ok(PathBuf::from(name));
    }
    path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(name)).find(|p| p.is_file()).ok_or_else(|| format!("{name} is not on PATH"))
}

fn output(cmd: &mut Command, what: &str) -> Result<String, String> {
    let o = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{what} failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The shared libraries (and interpreter) `ldd` lists for `exe`; none for a static executable.
fn shared_libs(exe: &Path, env_path: &str) -> Result<Vec<PathBuf>, String> {
    let o = Command::new("ldd").arg(exe).env("PATH", env_path).output().map_err(|e| format!("ldd: {e}"))?;
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    if !o.status.success() {
        return if text.contains("not a dynamic executable") { Ok(Vec::new()) } else { Err(format!("ldd {} failed: {}", exe.display(), text.trim())) };
    }
    let mut v = Vec::new();
    for l in text.lines() {
        let l = l.trim();
        let p = match l.split_once("=> ") {
            Some((_, r)) => r.split(" (").next().unwrap_or(""),
            None => l.split(" (").next().unwrap_or(""),
        };
        if p.starts_with('/') {
            v.push(PathBuf::from(p));
        } else if l.contains("not found") {
            return Err(format!("ldd {}: {l}", exe.display()));
        }
    }
    Ok(v)
}

/// The key lines of what does not depend on the tree: the checker, the environment and the tools.
pub(crate) fn host_inputs(env: &dyn Fn(&str) -> Option<String>, env_names: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let exe = std::env::current_exe().map_err(|e| format!("the checker's path: {e}"))?;
    let path = env("PATH").unwrap_or_default();
    let mut names: Vec<String> = ENV.iter().map(|s| s.to_string()).chain(env_names.iter().filter(|k| k.starts_with("LIELLA_")).cloned()).collect();
    names.sort();
    names.dedup();
    for k in &names {
        out.push(format!("env {k}={:?}", env(k)));
    }
    let cc = which("cc", &path)?;
    let mut tools: Vec<(String, PathBuf)> = vec![("checker".into(), exe), ("cc".into(), cc.clone())];
    for t in ["readelf", "objdump", "nm"] {
        tools.push((t.into(), which(t, &path)?));
    }
    for p in ["cc1", "as", "ld", "collect2"] {
        let named = output(Command::new(&cc).arg(format!("-print-prog-name={p}")).env("PATH", &path), "cc -print-prog-name")?;
        tools.push((p.into(), which(named.trim(), &path)?));
    }
    let libgcc = output(Command::new(&cc).arg("-print-libgcc-file-name").env("PATH", &path), "cc -print-libgcc-file-name")?;
    tools.push(("libgcc".into(), PathBuf::from(libgcc.trim())));
    let mut libs = Vec::new();
    for (name, p) in &tools {
        if !matches!(name.as_str(), "checker" | "libgcc") {
            libs.extend(shared_libs(p, &path)?);
        }
    }
    libs.sort();
    libs.dedup();
    tools.extend(libs.into_iter().map(|l| ("lib".to_string(), l)));
    let files: Vec<String> = tools.iter().map(|(_, p)| p.canonicalize().map(|c| c.display().to_string()).map_err(|e| format!("{}: {e}", p.display()))).collect::<Result<_, _>>()?;
    for ((name, p), (f, h)) in tools.iter().zip(files.iter().zip(sha256_files(Path::new("/"), &files)?)) {
        out.push(format!("tool {name} {} -> {f} {h}", p.display()));
    }
    for t in ["cc", "readelf", "objdump", "nm"] {
        let v = output(Command::new(which(t, &path)?).arg("--version").env("PATH", &path), &format!("{t} --version"))?;
        out.push(format!("version {t} {}", sha256_text(&v)?));
    }
    Ok(out)
}

/// The key lines of the tree and the products: paths, profile, every product, the manifest and the files its rules
/// name, the padded assembly and the preprocessed start-up probe.
pub(crate) fn tree_inputs(root: &Path, dir: &Path, profile: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Result<Vec<String>, String> {
    let canon = |p: &Path| p.canonicalize().map(|c| c.display().to_string()).unwrap_or_else(|e| format!("({e})"));
    let mut out = vec![format!("root {} {}", root.display(), canon(root)), format!("products {} {}", dir.display(), canon(dir)), format!("profile {}", profile.unwrap_or("-"))];
    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.map(|e| e.map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))).collect::<Result<_, _>>().map_err(|e| format!("{}: {e}", dir.display()))?;
    entries.sort();
    let (files, other): (Vec<_>, Vec<_>) = entries.into_iter().partition(|(_, p)| p.is_file());
    let product_paths: Vec<String> = files.iter().map(|(_, p)| p.display().to_string()).collect();
    for ((name, _), h) in files.iter().zip(sha256_files(root, &product_paths)?) {
        out.push(format!("product {name} {h}"));
    }
    for (name, p) in &other {
        out.push(format!("product-other {name} {}", if p.is_dir() { "directory" } else { "other" }));
    }
    let contracts = crate::layout::load(root)?;
    let mut rel: Vec<String> = vec![crate::layout::MANIFEST.to_string()];
    for c in &contracts {
        for v in c.params()?.into_values() {
            if !v.is_empty() && !v.contains(char::is_whitespace) && root.join(&v).is_file() {
                rel.push(v);
            }
        }
    }
    rel.extend(crate::jcc::JCC_PAD_FILES.iter().map(|f| format!("csys/src/machine/{f}")));
    rel.retain(|f| root.join(f).is_file());
    rel.sort();
    rel.dedup();
    for (f, h) in rel.iter().zip(sha256_files(root, &rel)?) {
        out.push(format!("input {f} {h}"));
    }
    let path = env("PATH").unwrap_or_default();
    let probe = root.join(PROBE);
    let pre = output(Command::new(which("cc", &path)?).args(["-E", "-std=gnu17", "-O1", "-fno-builtin"]).arg(&probe).env("PATH", &path), "cc -E page_faults.c")?;
    out.push(format!("probe {PROBE} preprocessed {}", sha256_text(&pre)?));
    Ok(out)
}

/// The key: the sha256 of every input line.
pub(crate) fn key(lines: &[String]) -> Result<String, String> {
    sha256_text(&(lines.join("\n") + "\n"))
}

/// `layout::check_products` with the cache: `full()` is the full check (its ok lines on a pass).
pub fn check(root: &Path, dir: &Path, profile: Option<&str>, full: impl FnOnce() -> Result<Vec<String>, String>) -> Result<(), String> {
    let env = |k: &str| std::env::var(k).ok();
    let names: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
    let entry = || -> Result<(PathBuf, String), String> {
        let mut lines = host_inputs(&env, &names)?;
        lines.extend(tree_inputs(root, dir, profile, &env)?);
        let k = key(&lines)?;
        Ok((root.join(DIR).join(&k), k))
    };
    verdict(mode(&env), entry, full).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xtask-layout-verdict-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn pass() -> Result<Vec<String>, String> {
        Ok(vec!["layout: A ok: 3 things (D-X.48)".into(), "layout: all 1 contracts of layout/contracts.tsv hold in x".into()])
    }

    #[test]
    fn the_train_and_the_opt_out_never_use_the_cache() {
        // no spec clause: tooling (LAYOUT-CHECK-VERDICT-CACHE): LIELLA_TRAIN set (not 0) is the train, which always
        // checks in full, even with a valid entry for the key; LIELLA_LAYOUT_CACHE=0 does too.
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert_eq!(mode(&env(&[("LIELLA_TRAIN", "1")])), Mode::Train);
        assert_eq!(mode(&env(&[("LIELLA_TRAIN", "yes"), ("LIELLA_LAYOUT_CACHE", "1")])), Mode::Train);
        assert_eq!(mode(&env(&[("LIELLA_TRAIN", "0")])), Mode::Use);
        assert_eq!(mode(&env(&[])), Mode::Use);
        assert_eq!(mode(&env(&[("LIELLA_LAYOUT_CACHE", "0")])), Mode::Off);
        let d = scratch("train");
        let at = || Ok((d.join("k1"), "k1".to_string()));
        assert_eq!(verdict(Mode::Use, at, pass).unwrap(), Outcome::Checked, "first run: a miss");
        assert_eq!(verdict(Mode::Use, at, || panic!("a valid entry must be served")).unwrap(), Outcome::Hit);
        for m in [Mode::Train, Mode::Off] {
            let ran = Cell::new(false);
            let r = verdict(m, || panic!("{m:?} must not compute the key"), || {
                ran.set(true);
                pass()
            });
            assert!(ran.get() && r == Ok(Outcome::Checked), "{m:?} served a cached verdict");
        }
        let failed = verdict(Mode::Use, || Ok((d.join("k2"), "k2".to_string())), || Err("layout: 1 of 1 contracts broken: A".into()));
        assert!(failed.is_err() && !d.join("k2").exists(), "a failure is never stored");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_corrupt_or_foreign_entry_is_a_miss_never_a_pass() {
        // no spec clause: tooling (LAYOUT-CHECK-VERDICT-CACHE): every change to a stored entry makes it a miss.
        let d = scratch("corrupt");
        let (p, k) = (d.join("k"), "k".to_string());
        assert_eq!(verdict(Mode::Use, || Ok((p.clone(), k.clone())), pass).unwrap(), Outcome::Checked);
        let good = std::fs::read_to_string(&p).unwrap();
        assert_eq!(read_entry(&p, &k).unwrap().map(|l| l.len()), Some(2));
        let other_key = entry_text("j", &pass().unwrap()).unwrap();
        let broken = [
            good.replacen("3 things", "4 things", 1),
            good.replacen("contracts 2", "contracts 1", 1),
            good.lines().filter(|l| !l.starts_with("sum ")).map(|l| format!("{l}\n")).collect(),
            good.replacen("layout: all", "layout: alL", 1),
            format!("{good}extra\n"),
            good[..good.len() - 1].to_string(),
            other_key,
            String::new(),
            format!("{MAGIC}\nkey k\ncontracts 0\nsum {}\n", sha256_text(&format!("{MAGIC}\nkey k\ncontracts 0\n")).unwrap()),
        ];
        for (i, b) in broken.iter().enumerate() {
            std::fs::write(&p, b).unwrap();
            assert!(!matches!(read_entry(&p, &k), Ok(Some(_))), "corrupt entry {i} read as a pass");
            let ran = Cell::new(false);
            let r = verdict(Mode::Use, || Ok((p.clone(), k.clone())), || {
                ran.set(true);
                pass()
            });
            assert!(ran.get() && r == Ok(Outcome::Checked), "corrupt entry {i} was served as a pass");
            assert_eq!(std::fs::read_to_string(&p).unwrap(), good, "the full check stores a valid entry again");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_key_changes_with_every_input() {
        // no spec clause: tooling (LAYOUT-CHECK-VERDICT-CACHE): each input of the check, changed alone, changes the
        // key; an unchanged tree keeps it.
        let root = scratch("key-root");
        let write = |rel: &str, text: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(crate::layout::MANIFEST, "id\tkind\tsubjects\trule\treason\tdecision\nA\tstartup-text-windows\tlibc.a\tkeys=k/s.keys; window=65536\tr\tD-X.49\n");
        write("k/s.keys", "fast f1\n");
        write(PROBE, "int main(void) { return 0; }\n");
        for f in crate::jcc::JCC_PAD_FILES {
            write(&format!("csys/src/machine/{f}"), "ret\n");
        }
        write("target/liella/libc.a", "archive");
        let dir = root.join("target/liella");
        let path = std::env::var("PATH").unwrap();
        let env_with = |cc: &'static str, extra: Option<(&'static str, &'static str)>| {
            let path = path.clone();
            move |k: &str| match k {
                "PATH" => Some(path.clone()),
                "CC" => Some(cc.to_string()),
                _ => extra.filter(|(n, _)| *n == k).map(|(_, v)| v.to_string()),
            }
        };
        let env = env_with("cc", None);
        let host = host_inputs(&env, &[]).unwrap();
        let k = |profile: Option<&str>| key(&[host.clone(), tree_inputs(&root, &dir, profile, &env).unwrap()].concat()).unwrap();
        let k0 = k(Some("fast"));
        assert_eq!(k(Some("fast")), k0, "the same inputs, the same key");
        assert_ne!(k(Some("release")), k0, "profile");
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        let exe_hash = sha256_files(Path::new("/"), &[exe.display().to_string()]).unwrap().remove(0);
        assert!(host.iter().any(|l| l.starts_with("tool checker ") && l.ends_with(&exe_hash)), "the checker's bytes: {host:?}");
        for t in ["cc", "readelf", "objdump", "nm", "cc1", "as", "ld", "collect2", "libgcc"] {
            assert!(host.iter().any(|l| l.starts_with(&format!("tool {t} "))), "tool {t}: {host:?}");
        }
        assert!(host.iter().any(|l| l.starts_with("tool lib ")), "the tools' shared libraries: {host:?}");
        assert_ne!(host_inputs(&env_with("gcc-other", None), &[]).unwrap(), host, "CC");
        assert_ne!(host_inputs(&env_with("cc", Some(("LIELLA_X", "1"))), &["LIELLA_X".to_string()]).unwrap(), host, "a LIELLA_ variable");
        let changes: [(&str, &str, &str); 6] = [
            ("the manifest", crate::layout::MANIFEST, "id\tkind\tsubjects\trule\treason\tdecision\nA\tstartup-text-windows\tlibc.a\tkeys=k/s.keys; window=4096\tr\tD-X.49\n"),
            ("a file a rule names", "k/s.keys", "fast f2\n"),
            ("the probe", PROBE, "int main(void) { return 1; }\n"),
            ("a padded assembly file", &format!("csys/src/machine/{}", crate::jcc::JCC_PAD_FILES[0]), "nop\n"),
            ("a product", "target/liella/libc.a", "archivf"),
            ("a new product", "target/liella/libm.a", "m"),
        ];
        let mut seen = vec![k0];
        for (what, rel, text) in changes {
            write(rel, text);
            let kn = k(Some("fast"));
            assert!(!seen.contains(&kn), "the key ignores {what} ({rel})");
            seen.push(kn);
        }
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(!seen.contains(&k(Some("fast"))), "the key ignores a new directory among the products");
        let _ = std::fs::remove_dir_all(&root);
    }
}
