//! sha256 helpers shared by the product build and the tools: the layout check's verdict cache
//! (`layout_verdict`, run by `cargo xtask so`) and the bench stage (`benchstage`).
//!
//! They live in their own module, not in `benchstage`, because `benchstage` is a tool module (`affected::XTASK_TOOLS`:
//! a change to it alone does not escalate the gate to FULL), and a module the product build calls must not be one.

use std::path::Path;
use std::process::{Command, Stdio};

/// The sha256 of each file (paths relative to `root`), in order, from `sha256sum` (coreutils, on every host).
pub(crate) fn sha256_files(root: &Path, rel: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(rel.len());
    for chunk in rel.chunks(512) {
        if let Some(p) = chunk.iter().find(|p| !root.join(p).is_file()) {
            return Err(format!("{}: missing", root.join(p).display()));
        }
        let o = Command::new("sha256sum").arg("--").args(chunk).current_dir(root).output().map_err(|e| format!("sha256sum: {e}"))?;
        if !o.status.success() {
            return Err(format!("sha256sum failed: {}", String::from_utf8_lossy(&o.stderr)));
        }
        let text = String::from_utf8_lossy(&o.stdout);
        let hashes: Vec<String> = text.lines().filter_map(|l| l.get(..64).map(str::to_string)).collect();
        if hashes.len() != chunk.len() {
            return Err(format!("sha256sum: {} hashes for {} files", hashes.len(), chunk.len()));
        }
        out.extend(hashes);
    }
    Ok(out)
}

/// The sha256 of `text`, from `sha256sum`.
pub(crate) fn sha256_text(text: &str) -> Result<String, String> {
    use std::io::Write;
    let mut c = Command::new("sha256sum").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(|e| format!("sha256sum: {e}"))?;
    c.stdin.take().ok_or("sha256sum: no stdin")?.write_all(text.as_bytes()).map_err(|e| format!("sha256sum: {e}"))?;
    let o = c.wait_with_output().map_err(|e| format!("sha256sum: {e}"))?;
    String::from_utf8_lossy(&o.stdout).get(..64).map(str::to_string).ok_or_else(|| "sha256sum: no output".into())
}
