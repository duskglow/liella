//! `cargo xtask locks`: the tracked `Cargo.lock` files stay fresh (FUZZ-LOCK-FRESH).
//!
//! The repository has three workspaces, each with a tracked lock file: the main one (`Cargo.lock`),
//! the product workspace (`crates/Cargo.lock`) and the fuzz harnesses (`fuzz/Cargo.lock`, built
//! with the nightly of `fuzz/rust-toolchain.toml`). A manifest change that leaves a lock behind
//! (a new path dependency, for example) is invisible until a build rewrites the lock in the
//! working tree; then `cargo xtask affected --base <ref>` sees a changed build file and escalates
//! every later gate in that tree to FULL. The check runs `cargo metadata --locked` against each
//! manifest: cargo resolves the graph and fails, writing nothing, when the lock would change.
//!
//! ```text
//! cargo xtask locks --check     fail naming each stale lock (also a `static` check, `lock-fresh`)
//! cargo xtask locks --update    let cargo rewrite the stale locks, then commit them
//! ```

use std::path::Path;
use std::process::{Command, Stdio};

/// The workspaces: manifest, and whether it builds with the fuzz nightly.
const WORKSPACES: [(&str, bool); 3] = [("Cargo.toml", false), ("crates/Cargo.toml", false), ("fuzz/Cargo.toml", true)];

/// The cargo that resolves `manifest` (`fuzz/` uses its own pinned nightly, as `fuzz-check` does).
fn cargo(root: &Path, nightly: bool) -> Result<Command, String> {
    if nightly {
        return Ok(crate::fuzz::nightly_cargo(root, &crate::fuzz::toolchain(root)?));
    }
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root);
    Ok(c)
}

/// `cargo metadata` for one manifest; `locked` makes cargo fail instead of rewriting the lock.
/// Ok(None) when cargo succeeded, Ok(Some(stderr)) when it failed.
fn metadata(mut c: Command, manifest: &Path, locked: bool) -> Result<Option<String>, String> {
    c.args(["metadata", "--format-version", "1", "--manifest-path"]).arg(manifest);
    if locked {
        c.arg("--locked");
    }
    let out = c.stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| format!("cannot run {c:?}: {e}"))?;
    Ok(if out.status.success() { None } else { Some(String::from_utf8_lossy(&out.stderr).trim().to_string()) })
}

/// cargo's own reason from its stderr: the `error:` line (a `help:` line about `--offline` follows it).
fn reason(stderr: &str) -> &str {
    stderr.lines().find(|l| l.starts_with("error")).or_else(|| stderr.lines().last()).unwrap_or("").trim()
}

/// Check one manifest's lock: Err names the lock and cargo's reason.
fn check_one(c: Command, manifest: &Path, name: &str) -> Result<(), String> {
    match metadata(c, manifest, true)? {
        None => Ok(()),
        Some(why) => Err(format!("{name}: the lock file is stale (cargo would rewrite it; `cargo xtask locks --update`, then commit it): {}", reason(&why))),
    }
}

/// The `static` check `lock-fresh`: every tracked lock file matches its manifest.
pub fn static_check(root: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for (manifest, nightly) in WORKSPACES {
        let lock = manifest.replace("Cargo.toml", "Cargo.lock");
        if let Err(e) = cargo(root, nightly).and_then(|c| check_one(c, &root.join(manifest), &lock)) {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        println!("xtask lock-fresh: {} lock files match their manifests", WORKSPACES.len());
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("--check") => static_check(root),
        Some("--update") => {
            for (manifest, nightly) in WORKSPACES {
                if let Some(why) = metadata(cargo(root, nightly)?, &root.join(manifest), false)? {
                    return Err(format!("{manifest}: cargo metadata failed: {why}"));
                }
            }
            static_check(root)
        }
        _ => Err("usage: cargo xtask locks --check | --update".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    /// no spec clause (build hygiene): a lock that is behind its manifest fails the check, the
    /// check leaves the lock as it was, and `--update`'s resolution makes it pass again.
    #[test]
    fn stale_lock_is_reported_and_left_alone() {
        let dir = std::env::temp_dir().join(format!("xtask-locks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let pkg = |name: &str, deps: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}");
        write(&dir.join("Cargo.toml"), &format!("[workspace]\nmembers = [\".\"]\n\n{}", pkg("a", "b = { path = \"b\" }\n")));
        write(&dir.join("src/lib.rs"), "");
        write(&dir.join("b/Cargo.toml"), &pkg("b", ""));
        write(&dir.join("b/src/lib.rs"), "");
        write(&dir.join("c/Cargo.toml"), &pkg("c", ""));
        write(&dir.join("c/src/lib.rs"), "");
        let manifest = dir.join("Cargo.toml");
        let lock = dir.join("Cargo.lock");
        let stable = || cargo(&dir, false).unwrap();
        assert_eq!(metadata(stable(), &manifest, false).unwrap(), None, "the first resolution writes the lock");
        assert!(check_one(stable(), &manifest, "Cargo.lock").is_ok(), "a fresh lock passes");
        let fresh = fs::read_to_string(&lock).unwrap();
        // the manifest gains a path dependency the lock does not list (the FUZZ-LOCK-FRESH case)
        write(&manifest, &format!("[workspace]\nmembers = [\".\"]\n\n{}", pkg("a", "b = { path = \"b\" }\nc = { path = \"c\" }\n")));
        let err = check_one(stable(), &manifest, "Cargo.lock").expect_err("a stale lock must fail the check");
        assert!(err.contains("Cargo.lock: the lock file is stale"), "{err}");
        assert!(err.contains("error"), "cargo's error line is quoted: {err}");
        assert_eq!(fs::read_to_string(&lock).unwrap(), fresh, "the check must not rewrite the lock");
        assert_eq!(metadata(stable(), &manifest, false).unwrap(), None);
        assert!(fs::read_to_string(&lock).unwrap().contains("name = \"c\""), "--update adds the new package");
        assert!(check_one(stable(), &manifest, "Cargo.lock").is_ok(), "the updated lock passes");
        let _ = fs::remove_dir_all(&dir);
    }
}
